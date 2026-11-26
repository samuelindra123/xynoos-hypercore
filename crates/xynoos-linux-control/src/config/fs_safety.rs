//! Filesystem and path security primitives for HyperCore configuration and runtime.
#![forbid(unsafe_code)]

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Check if the current process is running with effective root privileges (UID 0) on Unix.
pub fn is_effective_root() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata("/proc/self")
            .map(|m| m.uid() == 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Verify that a path is safe and does not traverse through unexpected symlinks.
pub fn verify_not_symlink(path: &Path) -> io::Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("Refusing to operate on symlink target: {}", path.display()),
            ));
        }
    }
    Ok(())
}

/// Safely read a file with strict size bounding and permission validation.
pub fn safe_read_bounded(path: &Path, max_bytes: usize) -> io::Result<String> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Refusing to read through symlink: {}", path.display()),
        ));
    }
    if meta.len() > max_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "File '{}' size ({} bytes) exceeds maximum permitted limit ({} bytes)",
                path.display(),
                meta.len(),
                max_bytes
            ),
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Reject world-writable files to prevent untrusted local tampering
        if meta.permissions().mode() & 0o002 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "Insecure file permissions on '{}': world-writable",
                    path.display()
                ),
            ));
        }
    }

    let mut file = File::open(path)?;
    let mut buf = String::with_capacity(meta.len() as usize);
    let mut handle = (&mut file).take(max_bytes as u64);
    handle.read_to_string(&mut buf)?;
    Ok(buf)
}

/// Atomically write data to a target path with unique temporary file, fsync, and directory sync.
pub fn atomic_write_file(path: &Path, content: &[u8], mode: u32) -> io::Result<()> {
    verify_not_symlink(path)?;

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid path without parent directory: {}", path.display()),
        )
    })?;

    fs::create_dir_all(parent)?;
    verify_not_symlink(parent)?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("xynoos-data");

    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let temp_name = format!(".{file_name}.tmp.{pid}.{counter}");
    let temp_path = parent.join(temp_name);

    // If temp file somehow already exists, remove it safely
    if let Ok(meta) = fs::symlink_metadata(&temp_path) {
        if meta.file_type().is_symlink() || meta.is_file() {
            let _ = fs::remove_file(&temp_path);
        }
    }

    let mut open_opts = OpenOptions::new();
    open_opts.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open_opts.mode(mode);
    }

    {
        let mut file = open_opts.open(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }

    fs::rename(&temp_path, path)?;

    // Fsync parent directory on Unix to guarantee directory entry persistence
    #[cfg(unix)]
    {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }

    Ok(())
}

/// Validate that a drop-in target file path is strictly within approved configuration subdirectories.
pub fn sanitize_dropin_target_file(raw_path: &str, etc_root: &Path) -> Result<PathBuf, String> {
    if raw_path.contains("..") || raw_path.contains('\0') {
        return Err(format!(
            "Unsafe path components detected in drop-in path: {raw_path}"
        ));
    }

    let cleaned = raw_path
        .strip_prefix("/etc/")
        .or_else(|| raw_path.strip_prefix("etc/"))
        .or_else(|| raw_path.strip_prefix('/'))
        .unwrap_or(raw_path);

    // Drop-ins must only be installed in sysctl.d or udev/rules.d
    if !cleaned.starts_with("sysctl.d/") && !cleaned.starts_with("udev/rules.d/") {
        return Err(format!(
            "Drop-in target '{}' is outside permitted managed directories (sysctl.d, udev/rules.d)",
            cleaned
        ));
    }

    let file_name = Path::new(cleaned)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("Invalid file name in drop-in path: {cleaned}"))?;

    if !file_name.ends_with(".conf") && !file_name.ends_with(".rules") {
        return Err(format!(
            "Drop-in file '{file_name}' must have .conf or .rules extension"
        ));
    }

    // Verify allowed characters in filename
    if !file_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(format!(
            "Drop-in file name '{file_name}' contains disallowed characters"
        ));
    }

    let full_path = etc_root.join(cleaned);

    // Verify not a symlink
    if let Ok(meta) = fs::symlink_metadata(&full_path) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Refusing drop-in operation through existing symlink: {}",
                full_path.display()
            ));
        }
    }

    Ok(full_path)
}

/// Validate canonical cgroup v2 path segments.
pub fn validate_cgroup_path(path: &str) -> Result<String, &'static str> {
    if path.is_empty() {
        return Err("Cgroup path cannot be empty");
    }
    if path.contains("..") || path.contains('\0') || path.contains("//") {
        return Err("Cgroup path contains illegal traversal or null characters");
    }
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() || trimmed == "sys/fs/cgroup" {
        return Err("Root cgroup mutation is strictly prohibited");
    }

    let without_prefix = trimmed.strip_prefix("sys/fs/cgroup/").unwrap_or(trimmed);

    if without_prefix.is_empty() {
        return Err("Root cgroup mutation is strictly prohibited");
    }

    for segment in without_prefix.split('/') {
        if segment.is_empty() {
            return Err("Cgroup path contains empty segment");
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return Err("Cgroup path segment contains invalid characters");
        }
    }

    Ok(without_prefix.to_string())
}

/// Validate sysctl key name against approved whitelist or format.
pub fn validate_sysctl_key(key: &str) -> Result<&str, &'static str> {
    if key.is_empty() {
        return Err("Sysctl key cannot be empty");
    }
    if key.contains("..") || key.contains('/') || key.contains('\0') {
        return Err("Sysctl key contains illegal characters");
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err("Sysctl key contains invalid characters");
    }

    const ALLOWED_KEYS: &[&str] = &[
        "vm.swappiness",
        "vm.page-cluster",
        "vm.vfs_cache_pressure",
        "kernel.sched_autogroup_enabled",
    ];

    if ALLOWED_KEYS.contains(&key) {
        Ok(key)
    } else {
        Err("Sysctl key is not in authorized kernel tuning allowlist")
    }
}

/// Validate device name (e.g. zram0, sda, nvme0n1).
pub fn validate_device_name(dev: &str) -> Result<&str, &'static str> {
    if dev.is_empty() {
        return Err("Device name cannot be empty");
    }
    if dev.contains('/') || dev.contains("..") || dev.contains('\0') {
        return Err("Device name contains path traversal or invalid characters");
    }
    if !dev
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Device name contains invalid characters");
    }
    Ok(dev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgroup_path_validation() {
        assert!(validate_cgroup_path("").is_err());
        assert!(validate_cgroup_path("/").is_err());
        assert!(validate_cgroup_path("/sys/fs/cgroup").is_err());
        assert!(validate_cgroup_path("user.slice/../root").is_err());
        assert!(validate_cgroup_path("user.slice/user-1000.slice").is_ok());
        assert_eq!(
            validate_cgroup_path("/user.slice/session-1.scope/").unwrap(),
            "user.slice/session-1.scope"
        );
    }

    #[test]
    fn sysctl_key_validation() {
        assert!(validate_sysctl_key("vm.swappiness").is_ok());
        assert!(validate_sysctl_key("kernel.sched_autogroup_enabled").is_ok());
        assert!(validate_sysctl_key("vm.swappiness/evil").is_err());
        assert!(validate_sysctl_key("net.ipv4.ip_forward").is_err());
        assert!(validate_sysctl_key("../sys/evil").is_err());
    }

    #[test]
    fn device_name_validation() {
        assert!(validate_device_name("zram0").is_ok());
        assert!(validate_device_name("nvme0n1").is_ok());
        assert!(validate_device_name("sda").is_ok());
        assert!(validate_device_name("../sda").is_err());
        assert!(validate_device_name("sda/evil").is_err());
    }

    #[test]
    fn dropin_path_validation() {
        let etc = PathBuf::from("/etc");
        assert!(sanitize_dropin_target_file("sysctl.d/99-xynoos-memory.conf", &etc).is_ok());
        assert!(sanitize_dropin_target_file("/etc/sysctl.d/99-xynoos-memory.conf", &etc).is_ok());
        assert!(sanitize_dropin_target_file("sysctl.d/../../etc/shadow", &etc).is_err());
        assert!(sanitize_dropin_target_file("cron.d/evil.conf", &etc).is_err());
        assert!(sanitize_dropin_target_file("sysctl.d/script.sh", &etc).is_err());
    }
}
