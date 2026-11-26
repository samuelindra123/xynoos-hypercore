//! Unified Init System Lifecycle Adapters (systemd, SysVinit, OpenRC, runit).
#![forbid(unsafe_code)]

#[cfg(not(feature = "debian-package"))]
use std::process::Command;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};
use xynoos_pressureiq::InitSystem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Active { pid: Option<u32> },
    Inactive,
    Failed,
    NotInstalled,
    Unknown(String),
}

#[derive(Debug, Clone)]
pub struct ServiceAdapter {
    pub init_system: InitSystem,
    pub etc_root: PathBuf,
    pub proc_root: PathBuf,
    pub run_root: PathBuf,
}

impl ServiceAdapter {
    /// Detect service manager and initialize adapter with specific roots.
    pub fn new(
        init_system: InitSystem,
        etc_root: PathBuf,
        proc_root: PathBuf,
        run_root: PathBuf,
    ) -> Self {
        Self {
            init_system,
            etc_root,
            proc_root,
            run_root,
        }
    }

    /// Read PID from file boundedly.
    fn read_pid_file(&self, pid_path: &Path) -> Option<u32> {
        let mut file = File::open(pid_path).ok()?;
        let mut s = String::new();
        file.read_to_string(&mut s).ok()?;
        s.trim().parse::<u32>().ok()
    }

    /// Check whether a process with given PID exists in `proc_root`.
    fn is_pid_alive(&self, pid: u32) -> bool {
        self.proc_root.join(pid.to_string()).is_dir()
    }

    /// Validate service name against path traversal, option injection, and control characters.
    pub fn validate_service_name(name: &str) -> io::Result<()> {
        if name.is_empty()
            || name.starts_with('-')
            || name.contains('/')
            || name.contains("..")
            || name.contains('\0')
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid or unsafe service name: {name}"),
            ));
        }
        Ok(())
    }

    /// Check if unit or service definition is installed.
    pub fn is_installed(&self, service_name: &str) -> bool {
        if Self::validate_service_name(service_name).is_err() {
            return false;
        }
        match self.init_system {
            InitSystem::Systemd => {
                self.etc_root
                    .join("systemd/system")
                    .join(format!("{service_name}.service"))
                    .exists()
                    || Path::new("/usr/lib/systemd/system")
                        .join(format!("{service_name}.service"))
                        .exists()
                    || Path::new("/lib/systemd/system")
                        .join(format!("{service_name}.service"))
                        .exists()
            }
            InitSystem::SysVinit => self.etc_root.join("init.d").join(service_name).exists(),
            InitSystem::OpenRC => self.etc_root.join("init.d").join(service_name).exists(),
            InitSystem::Runit => {
                self.etc_root.join("sv").join(service_name).exists()
                    || Path::new("/etc/sv").join(service_name).exists()
            }
            _ => false,
        }
    }

    /// Query state of service.
    pub fn status(&self, service_name: &str) -> io::Result<ServiceState> {
        Self::validate_service_name(service_name)?;
        if !self.is_installed(service_name) {
            return Ok(ServiceState::NotInstalled);
        }

        // Check common PID file locations
        let common_pid_paths = [
            self.run_root.join("xynoos/xynoos.pid"),
            self.run_root.join("xynoos.pid"),
            self.run_root.join(format!("{service_name}.pid")),
        ];

        let discovered_pid = common_pid_paths.iter().find_map(|p| self.read_pid_file(p));

        if let Some(pid) = discovered_pid {
            if self.is_pid_alive(pid) {
                return Ok(ServiceState::Active { pid: Some(pid) });
            }
        }

        #[cfg(not(feature = "debian-package"))]
        match self.init_system {
            InitSystem::Systemd => {
                if Path::new("/run/systemd/system").exists() {
                    let out = Command::new("systemctl")
                        .args(["is-active", service_name])
                        .output();
                    if let Ok(res) = out {
                        let stdout = String::from_utf8_lossy(&res.stdout).trim().to_string();
                        return match stdout.as_str() {
                            "active" => Ok(ServiceState::Active {
                                pid: discovered_pid,
                            }),
                            "inactive" => Ok(ServiceState::Inactive),
                            "failed" => Ok(ServiceState::Failed),
                            _ => Ok(ServiceState::Unknown(stdout)),
                        };
                    }
                }
                Ok(ServiceState::Inactive)
            }
            InitSystem::SysVinit => {
                let init_script = self.etc_root.join("init.d").join(service_name);
                if init_script.exists() {
                    let out = Command::new("service")
                        .args([service_name, "status"])
                        .output();
                    if let Ok(res) = out {
                        if res.status.success() {
                            return Ok(ServiceState::Active {
                                pid: discovered_pid,
                            });
                        }
                    }
                }
                Ok(ServiceState::Inactive)
            }
            InitSystem::OpenRC => {
                let out = Command::new("rc-service")
                    .args([service_name, "status"])
                    .output();
                if let Ok(res) = out {
                    if res.status.success() {
                        return Ok(ServiceState::Active {
                            pid: discovered_pid,
                        });
                    }
                }
                Ok(ServiceState::Inactive)
            }
            InitSystem::Runit => {
                let out = Command::new("sv").args(["status", service_name]).output();
                if let Ok(res) = out {
                    let s = String::from_utf8_lossy(&res.stdout);
                    if s.starts_with("run:") {
                        return Ok(ServiceState::Active {
                            pid: discovered_pid,
                        });
                    }
                }
                Ok(ServiceState::Inactive)
            }
            _ => Ok(ServiceState::Unknown("Unsupported init system".into())),
        }

        #[cfg(feature = "debian-package")]
        Ok(ServiceState::Inactive)
    }

    /// Enable service for automatic boot start.
    pub fn enable(&self, service_name: &str) -> io::Result<()> {
        Self::validate_service_name(service_name)?;
        let _ = service_name;
        match self.init_system {
            InitSystem::Systemd => {
                let wants_dir = self.etc_root.join("systemd/system/multi-user.target.wants");
                fs::create_dir_all(&wants_dir)?;
                let symlink_path = wants_dir.join(format!("{service_name}.service"));
                let target_path = format!("/etc/systemd/system/{service_name}.service");
                #[cfg(unix)]
                {
                    if !symlink_path.exists() {
                        let _ = std::os::unix::fs::symlink(Path::new(&target_path), &symlink_path);
                    }
                }
                #[cfg(not(feature = "debian-package"))]
                let _ = Command::new("systemctl")
                    .args(["enable", service_name])
                    .output();
                Ok(())
            }
            InitSystem::SysVinit => {
                #[cfg(not(feature = "debian-package"))]
                let _ = Command::new("update-rc.d")
                    .args([service_name, "defaults"])
                    .output();
                Ok(())
            }
            InitSystem::OpenRC => {
                let runlevels_dir = self.etc_root.join("runlevels/default");
                fs::create_dir_all(&runlevels_dir)?;
                let link = runlevels_dir.join(service_name);
                #[cfg(unix)]
                {
                    if !link.exists() {
                        let _ = std::os::unix::fs::symlink(
                            format!("/etc/init.d/{service_name}"),
                            &link,
                        );
                    }
                }
                #[cfg(not(feature = "debian-package"))]
                let _ = Command::new("rc-update")
                    .args(["add", service_name, "default"])
                    .output();
                Ok(())
            }
            InitSystem::Runit => {
                let sv_dir = self.etc_root.join("service");
                fs::create_dir_all(&sv_dir)?;
                let link = sv_dir.join(service_name);
                #[cfg(unix)]
                {
                    if !link.exists() {
                        let _ =
                            std::os::unix::fs::symlink(format!("/etc/sv/{service_name}"), &link);
                    }
                }
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unsupported init system",
            )),
        }
    }

    /// Disable service from starting on boot.
    pub fn disable(&self, service_name: &str) -> io::Result<()> {
        Self::validate_service_name(service_name)?;
        let _ = service_name;
        match self.init_system {
            InitSystem::Systemd => {
                let symlink_path = self
                    .etc_root
                    .join("systemd/system/multi-user.target.wants")
                    .join(format!("{service_name}.service"));
                if symlink_path.exists() {
                    let _ = fs::remove_file(symlink_path);
                }
                #[cfg(not(feature = "debian-package"))]
                let _ = Command::new("systemctl")
                    .args(["disable", service_name])
                    .output();
                Ok(())
            }
            InitSystem::SysVinit => {
                #[cfg(not(feature = "debian-package"))]
                let _ = Command::new("update-rc.d")
                    .args(["-f", service_name, "remove"])
                    .output();
                Ok(())
            }
            InitSystem::OpenRC => {
                let link = self.etc_root.join("runlevels/default").join(service_name);
                if link.exists() {
                    let _ = fs::remove_file(link);
                }
                #[cfg(not(feature = "debian-package"))]
                let _ = Command::new("rc-update")
                    .args(["del", service_name, "default"])
                    .output();
                Ok(())
            }
            InitSystem::Runit => {
                let link = self.etc_root.join("service").join(service_name);
                if link.exists() {
                    let _ = fs::remove_file(link);
                }
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unsupported init system",
            )),
        }
    }

    /// Start service if installed and not already running.
    pub fn start(&self, service_name: &str) -> io::Result<()> {
        Self::validate_service_name(service_name)?;
        let _ = service_name;
        #[cfg(not(feature = "debian-package"))]
        match self.init_system {
            InitSystem::Systemd => {
                let _ = Command::new("systemctl")
                    .args(["start", service_name])
                    .output();
                Ok(())
            }
            InitSystem::SysVinit => {
                let _ = Command::new("service")
                    .args([service_name, "start"])
                    .output();
                Ok(())
            }
            InitSystem::OpenRC => {
                let _ = Command::new("rc-service")
                    .args([service_name, "start"])
                    .output();
                Ok(())
            }
            InitSystem::Runit => {
                let _ = Command::new("sv").args(["start", service_name]).output();
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unsupported init system",
            )),
        }
        #[cfg(feature = "debian-package")]
        Ok(())
    }

    /// Stop service.
    pub fn stop(&self, service_name: &str) -> io::Result<()> {
        Self::validate_service_name(service_name)?;
        let _ = service_name;
        #[cfg(not(feature = "debian-package"))]
        match self.init_system {
            InitSystem::Systemd => {
                let _ = Command::new("systemctl")
                    .args(["stop", service_name])
                    .output();
                Ok(())
            }
            InitSystem::SysVinit => {
                let _ = Command::new("service")
                    .args([service_name, "stop"])
                    .output();
                Ok(())
            }
            InitSystem::OpenRC => {
                let _ = Command::new("rc-service")
                    .args([service_name, "stop"])
                    .output();
                Ok(())
            }
            InitSystem::Runit => {
                let _ = Command::new("sv").args(["stop", service_name]).output();
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unsupported init system",
            )),
        }
        #[cfg(feature = "debian-package")]
        Ok(())
    }
}
