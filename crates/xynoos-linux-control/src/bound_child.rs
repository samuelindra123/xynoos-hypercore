//! A session-owned empty child. All authority is derived from the held parent fd.
//! Checks and cgroup operations are sequential, so exclusive delegation is required.
use crate::transaction::{openat, read_at};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::Read,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileId {
    pub dev: u64,
    pub ino: u64,
}
fn id(f: &File) -> Result<FileId, String> {
    let m = f.metadata().map_err(|e| e.to_string())?;
    Ok(FileId {
        dev: m.dev(),
        ino: m.ino(),
    })
}
fn check_parent(parent: &File, expected: FileId) -> Result<(), String> {
    if id(parent)? != expected {
        return Err("parent descriptor identity changed".into());
    }
    let meta = parent.metadata().map_err(|e| e.to_string())?;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    if uid == 0 || meta.uid() != uid || meta.mode() & 0o022 != 0 {
        return Err("unsafe parent ownership or permissions".into());
    }
    Ok(())
}
fn no_descendants(stat: &str) -> bool {
    let lines: Vec<_> = stat
        .lines()
        .filter(|l| l.split_whitespace().next() == Some("nr_descendants"))
        .collect();
    lines.len() == 1 && lines[0].split_whitespace().collect::<Vec<_>>() == ["nr_descendants", "0"]
}

pub(crate) struct BoundChild {
    name: CString,
    dir: File,
    identity: FileId,
    uid: u32,
}
impl BoundChild {
    #[cfg(test)]
    pub(crate) fn fixture(parent: &File, name: &CStr) -> Self {
        let dir = openat(parent.as_raw_fd(), name, libc::O_RDONLY | libc::O_DIRECTORY).unwrap();
        let identity = id(&dir).unwrap();
        let uid = dir.metadata().unwrap().uid();
        Self {
            name: name.to_owned(),
            dir,
            identity,
            uid,
        }
    }
    pub(crate) fn population(&self, parent: &File, expected: FileId) -> Result<String, String> {
        if !self.identity_matches(parent, expected)? {
            return Err("child identity changed".into());
        }
        let population = read_at(&self.dir, c"cgroup.procs")?;
        if !self.identity_matches(parent, expected)? {
            return Err("child identity changed during read".into());
        }
        Ok(population)
    }
    pub(crate) fn identity(&self) -> crate::child_memory::ChildIdentity {
        crate::child_memory::ChildIdentity {
            device: self.identity.dev,
            inode: self.identity.ino,
        }
    }
    /// Compare the held child with the entry under the held parent. Other validation
    /// remains the responsibility of `verify` before any mutation.
    pub(crate) fn identity_matches(&self, parent: &File, expected: FileId) -> Result<bool, String> {
        check_parent(parent, expected)?;
        let current = openat(
            parent.as_raw_fd(),
            &self.name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        Ok(id(&current)? == self.identity && id(&self.dir)? == self.identity)
    }
    pub(crate) fn read_low(&self, parent: &File, expected: FileId) -> Result<u64, String> {
        self.verify(parent, expected)?;
        let control = openat(self.dir.as_raw_fd(), c"memory.low", libc::O_RDONLY)?;
        let meta = control.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.dev() != self.identity.dev || meta.uid() != self.uid {
            return Err("unsafe memory.low file identity or type".into());
        }
        let mut text = String::new();
        (&control)
            .take(33)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() > 32 {
            return Err("memory.low read too large".into());
        }
        let value = text
            .trim()
            .parse()
            .map_err(|_| "invalid memory.low".to_string())?;
        self.verify(parent, expected)?;
        let current = openat(self.dir.as_raw_fd(), c"memory.low", libc::O_RDONLY)?;
        if id(&current)? != id(&control)? {
            return Err("memory.low identity changed".into());
        }
        Ok(value)
    }
    // Only called after the containing session validates its held binding and hierarchy.
    pub(crate) fn create(parent: &File, expected: FileId) -> Result<Self, String> {
        crate::transaction::physical_mutation_disabled()?;
        let mut random = [0u8; 16];
        File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(|e| e.to_string())?;
        let name = CString::new(format!(
            "xynoos-test-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
        .unwrap();
        Self::create_named(parent, expected, name, |_, _| Ok(()))
    }
    fn create_named(
        parent: &File,
        expected: FileId,
        name: CString,
        after_mkdir: impl FnOnce(&File, &CStr) -> Result<(), String>,
    ) -> Result<Self, String> {
        check_parent(parent, expected)?;
        let pm = parent.metadata().map_err(|e| e.to_string())?;
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        if uid == 0 || pm.uid() != uid || pm.mode() & 0o022 != 0 {
            return Err("unsafe parent ownership or permissions".into());
        }
        // SAFETY: held directory and single-component name are valid.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(format!(
                "child creation failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // No cleanup on failure: identity/ownership may be unknown or another writer may race us.
        let child_name = name.to_string_lossy().into_owned();
        let result: Result<Self, String> = (|| {
            after_mkdir(parent, &name)?;
            let dir = openat(
                parent.as_raw_fd(),
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY,
            )?;
            let m = dir.metadata().map_err(|e| e.to_string())?;
            if m.uid() != uid || m.dev() != expected.dev || m.mode() & 0o077 != 0 {
                return Err("unsafe child ownership, device or permissions".into());
            }
            let child = Self {
                name,
                dir,
                identity: FileId {
                    dev: m.dev(),
                    ino: m.ino(),
                },
                uid,
            };
            child.verify(parent, expected)?;
            Ok(child)
        })();
        result.map_err(|e| format!("child {child_name} may remain; cleanup not attempted: {e}"))
    }
    pub(crate) fn verify(&self, parent: &File, expected: FileId) -> Result<(), String> {
        check_parent(parent, expected)?;
        let current = openat(
            parent.as_raw_fd(),
            &self.name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let m = current.metadata().map_err(|e| e.to_string())?;
        let held = self.dir.metadata().map_err(|e| e.to_string())?;
        if id(&self.dir)? != self.identity
            || id(&current)? != self.identity
            || m.uid() != self.uid
            || held.uid() != self.uid
            || m.mode() & 0o077 != 0
            || held.mode() & 0o077 != 0
            || m.dev() != expected.dev
        {
            return Err("child identity or ownership changed".into());
        }
        if !read_at(&current, c"cgroup.procs")?.trim().is_empty()
            || !no_descendants(&read_at(&current, c"cgroup.stat")?)
        {
            return Err("child populated or has descendants".into());
        }
        Ok(())
    }
    pub(crate) fn controllers(&self, parent: &File, expected: FileId) -> Result<String, String> {
        self.verify(parent, expected)?;
        let value = read_at(&self.dir, c"cgroup.controllers")?;
        self.verify(parent, expected)?;
        Ok(value)
    }
    pub(crate) fn remove(&self, parent: &File, expected: FileId) -> Result<(), String> {
        crate::transaction::physical_mutation_disabled()?;
        self.remove_with(parent, expected, |parent, name| {
            // SAFETY: checked parent and name; rmdir also refuses populated cgroups.
            if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0
            {
                return Err(format!(
                    "child removal failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(())
        })
    }
    fn remove_with(
        &self,
        parent: &File,
        expected: FileId,
        remove: impl FnOnce(&File, &CStr) -> Result<(), String>,
    ) -> Result<(), String> {
        self.verify(parent, expected)?;
        remove(parent, &self.name)?;
        match openat(
            parent.as_raw_fd(),
            &self.name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        ) {
            Ok(_) => Err("child name present after removal".into()),
            Err(_) => {
                // Distinguish ENOENT from permission errors, symlinks and other failures.
                let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
                // SAFETY: valid descriptor, name and output buffer.
                let rc = unsafe {
                    libc::fstatat(
                        parent.as_raw_fd(),
                        self.name.as_ptr(),
                        st.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                };
                if rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
                {
                    Ok(())
                } else {
                    Err("child removal read-back ambiguous".into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        path: PathBuf,
        parent: File,
        id: FileId,
    }
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "xynoos-bound-child-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            let parent = File::open(&path).unwrap();
            let id = id(&parent).unwrap();
            Self { path, parent, id }
        }
        fn create(&self) -> Result<BoundChild, String> {
            let path = self.path.join("child");
            BoundChild::create_named(
                &self.parent,
                self.id,
                CString::new("child").unwrap(),
                move |_, _| {
                    fs::write(path.join("cgroup.procs"), "").map_err(|e| e.to_string())?;
                    fs::write(path.join("cgroup.stat"), "nr_descendants 0\n")
                        .map_err(|e| e.to_string())?;
                    fs::write(path.join("cgroup.controllers"), "cpu memory pids\n")
                        .map_err(|e| e.to_string())?;
                    Ok(())
                },
            )
        }
        fn remove(&self, child: &BoundChild) -> Result<(), String> {
            let path = self.path.join("child");
            child.remove_with(&self.parent, self.id, move |_, _| {
                fs::remove_dir_all(path).map_err(|e| e.to_string())
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    #[test]
    fn creation_inspection_and_verified_removal() {
        let f = Fixture::new();
        let child = f.create().unwrap();
        assert!(child
            .controllers(&f.parent, f.id)
            .unwrap()
            .split_whitespace()
            .any(|c| c == "memory"));
        f.remove(&child).unwrap();
        assert!(!f.path.join("child").exists());
    }
    #[test]
    fn memory_low_read_is_descriptor_bound_and_rejects_bad_files() {
        let f = Fixture::new();
        let child = f.create().unwrap();
        let path = f.path.join("child/memory.low");
        assert_eq!(child.identity_matches(&f.parent, f.id), Ok(true));
        assert!(child.read_low(&f.parent, f.id).is_err());
        fs::write(&path, "8192\n").unwrap();
        assert_eq!(child.read_low(&f.parent, f.id).unwrap(), 8192);
        fs::write(&path, "1".repeat(33)).unwrap();
        assert!(child.read_low(&f.parent, f.id).is_err());
        fs::remove_file(&path).unwrap();
        symlink("cgroup.procs", &path).unwrap();
        assert!(child.read_low(&f.parent, f.id).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(child.read_low(&f.parent, f.id).is_err());
        fs::remove_dir(&path).unwrap();
        fs::write(&path, "4096").unwrap();
        fs::rename(f.path.join("child"), f.path.join("old")).unwrap();
        fs::create_dir(f.path.join("child")).unwrap();
        assert_eq!(child.identity_matches(&f.parent, f.id), Ok(false));
        assert!(child.read_low(&f.parent, f.id).is_err());
    }
    #[test]
    fn wrong_parent_and_duplicate_refused() {
        let f = Fixture::new();
        let other = Fixture::new();
        assert!(BoundChild::create_named(
            &other.parent,
            f.id,
            CString::new("child").unwrap(),
            |_, _| Ok(())
        )
        .is_err());
        assert!(BoundChild::create_named(
            &f.parent,
            FileId {
                ino: f.id.ino + 1,
                ..f.id
            },
            CString::new("child").unwrap(),
            |_, _| Ok(())
        )
        .is_err());
        let _child = f.create().unwrap();
        assert!(f.create().is_err());
    }
    #[test]
    fn population_descendants_and_identity_changes_refuse_cleanup() {
        let f = Fixture::new();
        let child = f.create().unwrap();
        let path = f.path.join("child");
        fs::write(path.join("cgroup.procs"), "42\n").unwrap();
        assert!(f.remove(&child).is_err());
        fs::write(path.join("cgroup.procs"), "").unwrap();
        fs::write(path.join("cgroup.stat"), "nr_descendants 1\n").unwrap();
        assert!(f.remove(&child).is_err());
        fs::write(path.join("cgroup.stat"), "nr_descendants 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(f.remove(&child).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&f.path, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(f.remove(&child).is_err());
        fs::set_permissions(&f.path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::rename(&path, f.path.join("old")).unwrap();
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(f.remove(&child).is_err());
        fs::remove_dir(&path).unwrap();
        symlink(f.path.join("old"), &path).unwrap();
        assert!(f.remove(&child).is_err());
    }
    #[test]
    fn symlink_duplicate_and_removal_failure_are_preserved() {
        let f = Fixture::new();
        symlink("elsewhere", f.path.join("child")).unwrap();
        assert!(f.create().is_err());
        fs::remove_file(f.path.join("child")).unwrap();
        let child = f.create().unwrap();
        assert!(child
            .remove_with(&f.parent, f.id, |_, _| Err(
                "injected removal failure".into()
            ))
            .is_err());
        assert!(f.path.join("child").is_dir());
        // Real rmdir against fixture files fails; no automatic retry occurs.
        assert!(child.remove(&f.parent, f.id).is_err());
    }
}
