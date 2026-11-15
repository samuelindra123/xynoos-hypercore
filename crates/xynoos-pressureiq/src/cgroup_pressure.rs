//! Read-only cgroup v2 PSI for an explicitly identified process.
//! Membership alone never establishes that a cgroup is exclusive to a workload.
use crate::{parse_psi, read, workload, Error, Psi};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupIdentity {
    pub path: PathBuf,
    pub device: u64,
    pub inode: u64,
}
#[derive(Debug, Clone)]
pub struct PressureSource {
    pub path: PathBuf,
    pub at: Instant,
    pub result: Result<Psi, Error>,
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub process: workload::Identity,
    pub cgroup: CgroupIdentity,
    pub cpu: PressureSource,
    pub memory: PressureSource,
    pub io: PressureSource,
    /// Point-in-time checks, not proof against concurrent membership changes.
    pub isolation_check: workload::Reading<()>,
}

fn io_error(e: std::io::Error) -> Error {
    match e.kind() {
        std::io::ErrorKind::NotFound => Error::Unsupported,
        std::io::ErrorKind::PermissionDenied => Error::Permission,
        _ => Error::Io(e.kind().to_string()),
    }
}
fn checked_identity(path: &Path) -> Result<CgroupIdentity, Error> {
    let root = Path::new("/sys/fs/cgroup");
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::Malformed("outside cgroup mount".into()))?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(Error::Malformed("cgroup path component".into()));
        };
        cursor.push(part);
        let m = fs::symlink_metadata(&cursor).map_err(io_error)?;
        if m.file_type().is_symlink() || !m.is_dir() {
            return Err(Error::Malformed("cgroup path is not a directory".into()));
        }
    }
    let m = fs::symlink_metadata(&cursor).map_err(io_error)?;
    if m.file_type().is_symlink() || !m.is_dir() {
        return Err(Error::Malformed("cgroup root".into()));
    }
    Ok(CgroupIdentity {
        path: cursor,
        device: m.dev(),
        inode: m.ino(),
    })
}
fn membership(id: &workload::Identity) -> Result<CgroupIdentity, Error> {
    if workload::identify(id.pid)? != *id {
        return Err(Error::Malformed("process identity changed".into()));
    }
    let own_ns = fs::metadata("/proc/self/ns/cgroup").map_err(io_error)?;
    let target_ns = fs::metadata(format!("/proc/{}/ns/cgroup", id.pid)).map_err(io_error)?;
    if (own_ns.dev(), own_ns.ino()) != (target_ns.dev(), target_ns.ino()) {
        return Err(Error::Malformed("different cgroup namespace".into()));
    }
    let member = read(&PathBuf::from(format!("/proc/{}/cgroup", id.pid)))?;
    if member.lines().filter(|l| l.starts_with("0::")).count() != 1 {
        return Err(Error::Malformed("ambiguous cgroup membership".into()));
    }
    let mounts = read(Path::new("/proc/self/mountinfo"))?;
    if mounts
        .lines()
        .filter(|l| {
            l.split_once(" - ")
                .is_some_and(|(_, r)| r.starts_with("cgroup2 "))
        })
        .count()
        != 1
    {
        return Err(Error::Malformed("ambiguous cgroup2 mount".into()));
    }
    let path = crate::system::parse_cgroup_path(&member, &mounts)?;
    let cg = checked_identity(&path)?;
    if workload::identify(id.pid)? != *id {
        return Err(Error::Malformed("process identity changed".into()));
    }
    Ok(cg)
}
fn source(id: &workload::Identity, cg: &CgroupIdentity, name: &str) -> PressureSource {
    let path = cg.path.join(name);
    let at = Instant::now();
    let result = (|| {
        if membership(id)? != *cg {
            return Err(Error::Malformed("cgroup identity changed".into()));
        }
        let result = read(&path).and_then(|v| parse_psi(&v));
        if membership(id)? != *cg {
            return Err(Error::Malformed("cgroup identity changed".into()));
        }
        result
    })();
    PressureSource { path, at, result }
}
fn isolation_check(id: &workload::Identity, cg: &CgroupIdentity) -> workload::Reading<()> {
    let at = Instant::now();
    let result = (|| {
        if membership(id)? != *cg {
            return Err(Error::Malformed("cgroup identity changed".into()));
        }
        let procs = read(&cg.path.join("cgroup.procs"))?;
        let pids: Vec<_> = procs.lines().collect();
        if pids.len() != 1 || pids[0].parse::<u32>().ok() != Some(id.pid) {
            return Err(Error::Malformed(
                "cgroup not exclusively populated by target process".into(),
            ));
        }
        for (i, entry) in fs::read_dir(&cg.path).map_err(io_error)?.enumerate() {
            if i >= 4096 {
                return Err(Error::Bounds);
            }
            let kind = entry.map_err(io_error)?.file_type().map_err(io_error)?;
            if kind.is_dir() || kind.is_symlink() {
                return Err(Error::Malformed("cgroup descendants or symlink".into()));
            }
        }
        if membership(id)? != *cg {
            return Err(Error::Malformed("cgroup identity changed".into()));
        }
        Ok(())
    })();
    workload::Reading { at, result }
}
/// Source timestamps are taken separately; PSI values are percent avg10 and total microseconds.
pub fn collect(id: &workload::Identity) -> Result<Snapshot, Error> {
    let cgroup = membership(id)?;
    Ok(Snapshot {
        process: id.clone(),
        cpu: source(id, &cgroup, "cpu.pressure"),
        memory: source(id, &cgroup, "memory.pressure"),
        io: source(id, &cgroup, "io.pressure"),
        isolation_check: isolation_check(id, &cgroup),
        cgroup,
    })
}
#[derive(Debug)]
pub struct PressureDelta {
    pub interval: Duration,
    pub some_us: u64,
    pub full_us: Option<u64>,
}
/// Deltas are independent per PSI source; reset and missing data are errors.
pub fn delta(a: &Snapshot, b: &Snapshot, source_name: &str) -> Result<PressureDelta, Error> {
    if a.process != b.process || a.cgroup != b.cgroup {
        return Err(Error::Malformed("identity mismatch".into()));
    }
    let expected = a.cgroup.path.join(format!("{source_name}.pressure"));
    let (a, b) = match source_name {
        "cpu" => (&a.cpu, &b.cpu),
        "memory" => (&a.memory, &b.memory),
        "io" => (&a.io, &b.io),
        _ => return Err(Error::Malformed("source".into())),
    };
    if a.path != b.path || a.path != expected {
        return Err(Error::Malformed("source mismatch".into()));
    }
    let interval =
        b.at.checked_duration_since(a.at)
            .filter(|v| !v.is_zero())
            .ok_or_else(|| Error::Malformed("monotonic interval".into()))?;
    let x = a.result.as_ref().map_err(Clone::clone)?;
    let y = b.result.as_ref().map_err(Clone::clone)?;
    let sub = |a: u64, b: u64| {
        b.checked_sub(a)
            .ok_or_else(|| Error::CounterReset(source_name.into()))
    };
    Ok(PressureDelta {
        interval,
        some_us: sub(x.some.total_us, y.some.total_us)?,
        full_us: match (&x.full, &y.full) {
            (Some(x), Some(y)) => Some(sub(x.total_us, y.total_us)?),
            _ => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_independent_deltas() {
        assert!(checked_identity(Path::new("/sys/fs/cgroup/../etc")).is_err());
        assert!(checked_identity(Path::new("/tmp")).is_err());
        let t = Instant::now();
        let cg = CgroupIdentity {
            path: "/sys/fs/cgroup/test".into(),
            device: 1,
            inode: 2,
        };
        let make = |n| {
            let source = |name| PressureSource {
                path: cg.path.join(name),
                at: t + Duration::from_secs(n),
                result: Ok(
                    parse_psi(&format!("some avg10=0 avg60=0 avg300=0 total={}", n * 10)).unwrap(),
                ),
            };
            Snapshot {
                process: workload::Identity {
                    pid: 1,
                    start_ticks: 1,
                },
                cpu: source("cpu.pressure"),
                memory: source("memory.pressure"),
                io: source("io.pressure"),
                isolation_check: workload::Reading {
                    at: t,
                    result: Ok(()),
                },
                cgroup: cg.clone(),
            }
        };
        let a = make(1);
        let mut b = make(2);
        b.memory.result = Err(Error::Permission);
        assert_eq!(delta(&a, &b, "cpu").unwrap().some_us, 10);
        assert!(matches!(delta(&a, &b, "memory"), Err(Error::Permission)));
        b.cpu.result = a.cpu.result.clone();
        let mut old = a.clone();
        old.cpu.result = make(2).cpu.result;
        assert!(matches!(
            delta(&old, &b, "cpu"),
            Err(Error::CounterReset(_))
        ));
        b.cgroup.inode += 1;
        assert!(delta(&a, &b, "io").is_err());
        b.cgroup.inode -= 1;
        let mut wrong = a.clone();
        wrong.io.path = wrong.cgroup.path.join("cpu.pressure");
        b.io.path = wrong.io.path.clone();
        assert!(delta(&wrong, &b, "io").is_err());
    }
}
