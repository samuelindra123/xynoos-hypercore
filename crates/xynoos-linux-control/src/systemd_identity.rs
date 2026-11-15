//! Read-only, point-in-time binding to a systemd user service. No controller backend.
//! `busctl --json=short` transports typed D-Bus replies; every property is required.
use serde_json::Value;
use std::{
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Component, PathBuf},
    process::Command,
};
use xynoos_pressureiq::{cgroup_pressure, workload};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Unit {
    object: String,
    id: String,
    invocation: Vec<u8>,
    active: String,
    load: String,
    transient: bool,
    delegate: bool,
    subgroup: String,
    cgroup: String,
    pid: u32,
}

#[derive(Clone, Copy)]
enum ReplyKind {
    Method,
    Property,
}

fn bus(args: &[&str], signature: &str, kind: ReplyKind) -> Result<Value, String> {
    let out = Command::new("busctl")
        .args(["--user", "--json=short"])
        .args(args)
        .output()
        .map_err(|e| format!("D-Bus inspection: {e}"))?;
    if !out.status.success() {
        return Err(format!("D-Bus inspection failed (user bus unavailable, unit exited, or property lookup failed): {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    parse_reply(&out.stdout, signature, kind)
}
fn parse_reply(bytes: &[u8], signature: &str, kind: ReplyKind) -> Result<Value, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("D-Bus JSON: {e}"))?;
    if v.get("type").and_then(Value::as_str) != Some(signature) {
        return Err("unexpected D-Bus signature".into());
    }
    let data = v.get("data").ok_or("missing D-Bus data")?;
    let value = match (kind, signature) {
        (ReplyKind::Method, "o") => {
            let args = data.as_array().ok_or("invalid method reply framing")?;
            if args.len() != 1 {
                return Err("invalid method reply framing".into());
            }
            let path = args[0].as_str().ok_or("invalid object path")?;
            if !path.starts_with('/')
                || (path != "/"
                    && path[1..].split('/').any(|part| {
                        part.is_empty()
                            || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    }))
            {
                return Err("invalid object path".into());
            }
            &args[0]
        }
        (ReplyKind::Property, "s") if data.is_string() => data,
        (ReplyKind::Property, "b") if data.is_boolean() => data,
        (ReplyKind::Property, "u") if data.as_u64().is_some_and(|n| u32::try_from(n).is_ok()) => {
            data
        }
        (ReplyKind::Property, "ay") => {
            let bytes = data.as_array().ok_or("invalid invocation array")?;
            if bytes.len() != 16
                || bytes
                    .iter()
                    .any(|v| !v.as_u64().is_some_and(|n| u8::try_from(n).is_ok()))
            {
                return Err("invalid invocation bytes".into());
            }
            data
        }
        _ => return Err("unexpected D-Bus data shape or signature".into()),
    };
    Ok(value.clone())
}
fn unit(name: &str) -> Result<Unit, String> {
    const DEST: &str = "org.freedesktop.systemd1";
    let object = bus(
        &[
            "call",
            DEST,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "GetUnit",
            "s",
            name,
        ],
        "o",
        ReplyKind::Method,
    )?
    .as_str()
    .ok_or("invalid unit object")?
    .to_owned();
    let prop = |interface: &str, key: &str, sig: &str| {
        bus(
            &["get-property", DEST, &object, interface, key],
            sig,
            ReplyKind::Property,
        )
    };
    let text = |interface, key| -> Result<String, String> {
        Ok(prop(interface, key, "s")?
            .as_str()
            .ok_or("invalid string property")?
            .to_owned())
    };
    let boolean = |interface, key| -> Result<bool, String> {
        prop(interface, key, "b")?
            .as_bool()
            .ok_or("invalid boolean property".into())
    };
    let u = "org.freedesktop.systemd1.Unit";
    let s = "org.freedesktop.systemd1.Service";
    let invocation = prop(u, "InvocationID", "ay")?
        .as_array()
        .ok_or("invalid invocation")?
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or("invalid invocation byte".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Unit {
        object: object.clone(),
        id: text(u, "Id")?,
        invocation,
        active: text(u, "ActiveState")?,
        load: text(u, "LoadState")?,
        transient: boolean(u, "Transient")?,
        delegate: boolean(s, "Delegate")?,
        subgroup: text(s, "DelegateSubgroup")?,
        cgroup: text(s, "ControlGroup")?,
        pid: prop(s, "MainPID", "u")?
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("invalid MainPID")?,
    })
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Observation {
    unit: Unit,
    process: workload::Identity,
    membership: String,
    parent: (u64, u64),
    manager: (u64, u64),
}
fn validate(name: &str, o: &Observation) -> Result<(), String> {
    let u = &o.unit;
    if u.id != name || !name.ends_with(".service") || u.object.is_empty() {
        return Err("unit identity mismatch".into());
    }
    if u.invocation.len() != 16 || u.invocation.iter().all(|b| *b == 0) {
        return Err("missing invocation identity".into());
    }
    if u.active != "active"
        || u.load != "loaded"
        || !u.transient
        || !u.delegate
        || u.subgroup != "manager"
    {
        return Err("unit state or delegation mismatch".into());
    }
    let p = std::path::Path::new(&u.cgroup);
    if !p.is_absolute()
        || p == std::path::Path::new("/")
        || p.components()
            .skip(1)
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("invalid unit cgroup".into());
    }
    if u.pid == 0
        || u.pid != o.process.pid
        || o.membership != format!("{}/manager", u.cgroup)
        || o.parent.0 != o.manager.0
        || o.parent == o.manager
    {
        return Err("manager identity or membership mismatch".into());
    }
    Ok(())
}
fn identity(f: &File) -> Result<(u64, u64), String> {
    let m = f.metadata().map_err(|e| e.to_string())?;
    let mut st = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: valid descriptor and writable statfs storage.
    if unsafe { libc::fstatfs(f.as_raw_fd(), st.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: fstatfs succeeded.
    if unsafe { st.assume_init() }.f_type as u64 != 0x6367_7270 {
        return Err("not cgroup v2".into());
    }
    Ok((m.dev(), m.ino()))
}
fn member(pid: u32) -> Result<String, String> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).map_err(|e| e.to_string())?;
    let mut lines = s.lines().filter_map(|l| l.strip_prefix("0::"));
    let p = lines.next().ok_or("missing unified membership")?.to_owned();
    if lines.next().is_some() {
        return Err("ambiguous membership".into());
    }
    Ok(p)
}
/// Held descriptors and the original invocation/process identities. Revalidate immediately
/// before each future operation; this type does not itself authorize any mutation.
pub struct Binding {
    name: String,
    initial: Observation,
    parent: File,
    manager: File,
    parent_path: PathBuf,
}
impl Binding {
    pub fn discover(name: &str) -> Result<Self, String> {
        let u = unit(name)?;
        if u.pid != std::process::id() {
            return Err("MainPID is not this manager".into());
        }
        let process = workload::identify(u.pid).map_err(|e| format!("process: {e:?}"))?;
        let membership = member(u.pid)?;
        let cg = cgroup_pressure::collect(&process)
            .map_err(|e| format!("membership: {e:?}"))?
            .cgroup;
        let parent_path = cg.path.parent().ok_or("missing parent")?.to_path_buf();
        let parent = crate::transaction::open_absolute_dir(&parent_path)?;
        let manager = crate::transaction::openat(
            parent.as_raw_fd(),
            c"manager",
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let initial = Observation {
            unit: u,
            process,
            membership,
            parent: identity(&parent)?,
            manager: identity(&manager)?,
        };
        if (cg.device, cg.inode) != initial.manager {
            return Err("manager inode mismatch".into());
        }
        validate(name, &initial)?;
        let binding = Self {
            name: name.into(),
            initial,
            parent,
            manager,
            parent_path,
        };
        binding.revalidate()?;
        Ok(binding)
    }
    /// Diagnostic snapshot of the original, held binding; this grants no authority.
    pub fn diagnostic(&self) -> Value {
        diagnostic(&self.initial)
    }
    pub fn revalidate(&self) -> Result<(), String> {
        let u = unit(&self.name)?;
        let process =
            workload::identify(self.initial.process.pid).map_err(|e| format!("process: {e:?}"))?;
        let membership = member(process.pid)?;
        let cg = cgroup_pressure::collect(&process)
            .map_err(|e| format!("membership: {e:?}"))?
            .cgroup;
        let parent = crate::transaction::open_absolute_dir(&self.parent_path)?;
        let manager = crate::transaction::openat(
            parent.as_raw_fd(),
            c"manager",
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let observed = Observation {
            unit: u,
            process,
            membership,
            parent: identity(&parent)?,
            manager: identity(&manager)?,
        };
        validate(&self.name, &observed)?;
        compare(&self.name, &self.initial, &observed)?;
        if identity(&self.parent)? != observed.parent
            || identity(&self.manager)? != observed.manager
            || (cg.device, cg.inode) != observed.manager
        {
            return Err("stale binding".into());
        }
        Ok(())
    }
}

fn diagnostic(o: &Observation) -> Value {
    serde_json::json!({
        "unit_id": o.unit.id, "object": o.unit.object,
        "invocation_id": o.unit.invocation.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "active_state": o.unit.active, "load_state": o.unit.load,
        "transient": o.unit.transient, "delegate": o.unit.delegate,
        "delegate_subgroup": o.unit.subgroup, "control_group": o.unit.cgroup,
        "main_pid": o.unit.pid, "start_ticks": o.process.start_ticks,
        "actual_membership": o.membership,
        "parent_descriptor": {"device": o.parent.0, "inode": o.parent.1},
        "manager_descriptor": {"device": o.manager.0, "inode": o.manager.1}
    })
}

fn compare(name: &str, initial: &Observation, current: &Observation) -> Result<(), String> {
    validate(name, current)?;
    if initial != current {
        return Err("stale binding".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Observation {
        Observation {
            unit: Unit {
                object: "/unit/test".into(),
                id: "test.service".into(),
                invocation: vec![1; 16],
                active: "active".into(),
                load: "loaded".into(),
                transient: true,
                delegate: true,
                subgroup: "manager".into(),
                cgroup: "/test.service".into(),
                pid: 42,
            },
            process: workload::Identity {
                pid: 42,
                start_ticks: 100,
            },
            membership: "/test.service/manager".into(),
            parent: (1, 2),
            manager: (1, 3),
        }
    }
    #[test]
    fn identity_fixtures_fail_closed() {
        let base = fixture();
        validate("test.service", &base).unwrap();
        for case in 0..9 {
            let mut changed = base.clone();
            match case {
                0 => changed.unit.id = "other.service".into(),
                1 => changed.unit.invocation[0] = 2,
                2 => changed.unit.active = "inactive".into(),
                3 => changed.process.start_ticks += 1,
                4 => changed.unit.subgroup = "other".into(),
                5 => changed.parent.1 += 1,
                6 => changed.membership = "/other/manager".into(),
                7 => changed.unit.invocation.clear(),
                8 => changed.manager.1 += 1,
                _ => unreachable!(),
            }
            assert!(
                compare("test.service", &base, &changed).is_err(),
                "case {case}"
            );
        }
    }
    #[test]
    fn diagnostic_contains_bound_identities() {
        let d = diagnostic(&fixture());
        assert_eq!(d["invocation_id"], "01010101010101010101010101010101");
        assert_eq!(d["main_pid"], 42);
        assert_eq!(d["start_ticks"], 100);
        assert_eq!(d["actual_membership"], "/test.service/manager");
        assert_eq!(d["parent_descriptor"]["inode"], 2);
        assert_eq!(d["manager_descriptor"]["inode"], 3);
        assert_eq!(d["delegate"], true);
    }
    #[test]
    fn busctl_mx_reply_fixtures() {
        let parse = |s: &str, sig, kind| parse_reply(s.as_bytes(), sig, kind);
        assert_eq!(
            parse(
                r#"{"type":"o","data":["/org/freedesktop/systemd1/unit/test_2eservice"]}"#,
                "o",
                ReplyKind::Method
            )
            .unwrap(),
            "/org/freedesktop/systemd1/unit/test_2eservice"
        );
        assert_eq!(
            parse(
                r#"{"type":"ay","data":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,255]}"#,
                "ay",
                ReplyKind::Property
            )
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
            16
        );
        assert_eq!(
            parse(r#"{"type":"s","data":"active"}"#, "s", ReplyKind::Property).unwrap(),
            "active"
        );
        assert_eq!(
            parse(r#"{"type":"b","data":true}"#, "b", ReplyKind::Property).unwrap(),
            true
        );
        assert_eq!(
            parse(r#"{"type":"u","data":42}"#, "u", ReplyKind::Property).unwrap(),
            42
        );
        for (data, sig, kind) in [
            (r#"{}"#, "s", ReplyKind::Property),
            (r#"{"type":"s"}"#, "s", ReplyKind::Property),
            (r#"{"type":"b","data":true}"#, "s", ReplyKind::Property),
            (
                r#"{"type":"s","data":["active"]}"#,
                "s",
                ReplyKind::Property,
            ),
            (r#"{"type":"o","data":"/unit"}"#, "o", ReplyKind::Method),
            (
                r#"{"type":"o","data":["/unit","/other"]}"#,
                "o",
                ReplyKind::Method,
            ),
            (
                r#"{"type":"o","data":["/bad-path"]}"#,
                "o",
                ReplyKind::Method,
            ),
            (r#"{"type":"ay","data":[1]}"#, "ay", ReplyKind::Property),
            (
                r#"{"type":"ay","data":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,256]}"#,
                "ay",
                ReplyKind::Property,
            ),
            (r#"{"type":"u","data":-1}"#, "u", ReplyKind::Property),
            ("garbage", "s", ReplyKind::Property),
        ] {
            assert!(parse(data, sig, kind).is_err(), "accepted {data}");
        }
    }
    #[test]
    fn real_read_only_linux_discovery() {
        let pid = std::process::id();
        let process = workload::identify(pid).unwrap();
        let membership = member(pid).unwrap();
        assert!(membership.starts_with('/'));
        assert_eq!(workload::identify(pid).unwrap(), process);
        // A missing user bus or unit must never create a binding.
        assert!(unit("xynoos-nonexistent-identity-test.service").is_err());
        eprintln!("read-only discovery: pid={pid}, start_ticks={}, membership={membership}; nonexistent unit rejected", process.start_ticks);
    }
}
