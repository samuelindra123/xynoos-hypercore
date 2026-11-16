//! Private cgroup I/O boundary. Production construction requires a live Binding;
//! fixture implementations exist only in unit tests. No caller-supplied paths.
use super::ParentIdentity;
use crate::{bound_child::FileId, child_memory::ChildIdentity, systemd_identity::BoundIdentity};
use std::ffi::CStr;

// Policy, authorization, hierarchy checks and unresolved latches stay in the
// controller/session, above this interface. Child methods retain BoundChild checks.
pub(super) trait SessionIo: Send + Sync {
    fn bound_identity(&self) -> BoundIdentity;
    fn diagnostic(&self) -> serde_json::Value;
    fn revalidate(&self) -> Result<(), String>;
    fn read_parent(&self, name: &CStr) -> Result<String, String>;
    fn read_manager(&self, name: &CStr) -> Result<String, String>;
    fn write_controller(&mut self, command: &[u8]) -> Result<(), String>;
    fn registered_child(&self) -> Option<ChildIdentity>;
    fn child_identity_matches(&self) -> Result<bool, String>;
    fn verify_child(&self, parent: ParentIdentity) -> Result<(), String>;
    fn child_population(&self) -> Result<String, String>;
    fn read_low(&self) -> Result<u64, String>;
    fn child_controllers(&self, parent: ParentIdentity) -> Result<String, String>;
    fn create_child(&mut self, parent: ParentIdentity) -> Result<ChildIdentity, String>;
    fn remove_child(&mut self, parent: ParentIdentity) -> Result<(), String>;

    #[cfg(test)]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

pub(super) struct DescriptorIo {
    pub(super) binding: crate::systemd_identity::Binding,
    pub(super) child: Option<crate::bound_child::BoundChild>,
}
impl DescriptorIo {
    pub(super) fn new(binding: crate::systemd_identity::Binding) -> Self {
        Self {
            binding,
            child: None,
        }
    }
    fn parent_id(&self) -> FileId {
        let (dev, ino) = self.binding.parent_identity();
        FileId { dev, ino }
    }
    fn child(&self) -> Result<&crate::bound_child::BoundChild, String> {
        self.child.as_ref().ok_or("no registered child".into())
    }
}
fn file_id(parent: ParentIdentity) -> FileId {
    FileId {
        dev: parent.device,
        ino: parent.inode,
    }
}
impl SessionIo for DescriptorIo {
    fn bound_identity(&self) -> BoundIdentity {
        self.binding.bound_identity()
    }
    fn diagnostic(&self) -> serde_json::Value {
        self.binding.diagnostic()
    }
    fn revalidate(&self) -> Result<(), String> {
        self.binding.revalidate()
    }
    fn read_parent(&self, name: &CStr) -> Result<String, String> {
        crate::transaction::read_at(self.binding.parent(), name)
    }
    fn read_manager(&self, name: &CStr) -> Result<String, String> {
        crate::transaction::read_at(self.binding.manager(), name)
    }
    fn write_controller(&mut self, command: &[u8]) -> Result<(), String> {
        use std::os::{fd::AsRawFd, unix::fs::FileExt};
        let f = crate::transaction::openat(
            self.binding.parent().as_raw_fd(),
            c"cgroup.subtree_control",
            libc::O_WRONLY,
        )?;
        let n = f.write_at(command, 0).map_err(|e| e.to_string())?;
        if n != command.len() {
            return Err("short controller write".into());
        }
        Ok(())
    }
    fn registered_child(&self) -> Option<ChildIdentity> {
        self.child.as_ref().map(|child| child.identity())
    }
    fn child_identity_matches(&self) -> Result<bool, String> {
        self.child()?
            .identity_matches(self.binding.parent(), self.parent_id())
    }
    fn verify_child(&self, parent: ParentIdentity) -> Result<(), String> {
        self.child()?.verify(self.binding.parent(), file_id(parent))
    }
    fn child_population(&self) -> Result<String, String> {
        self.child()?
            .population(self.binding.parent(), self.parent_id())
    }
    fn read_low(&self) -> Result<u64, String> {
        self.child()?
            .read_low(self.binding.parent(), self.parent_id())
    }
    fn child_controllers(&self, parent: ParentIdentity) -> Result<String, String> {
        self.child()?
            .controllers(self.binding.parent(), file_id(parent))
    }
    fn create_child(&mut self, parent: ParentIdentity) -> Result<ChildIdentity, String> {
        let child = crate::bound_child::BoundChild::create(self.binding.parent(), file_id(parent))?;
        let identity = child.identity();
        self.child = Some(child);
        Ok(identity)
    }
    fn remove_child(&mut self, parent: ParentIdentity) -> Result<(), String> {
        self.child()?
            .remove(self.binding.parent(), file_id(parent))?;
        self.child = None;
        Ok(())
    }
    #[cfg(test)]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
