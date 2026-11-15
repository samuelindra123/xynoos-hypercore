//! Guarded memory.low access for a session-owned empty child.
//! The production writer is deliberately disabled; the protocol is exercised with fixtures.
use crate::MAX_LOW_BYTES;

/// Identity of the registered child, derived from its held directory descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildIdentity {
    pub device: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    RefusedBeforeWrite,
    WriteOutcomeAmbiguous,
    ReadBackMismatch,
    IdentityConflict,
    CompetingValue,
    RestorationFailure,
}

/// Diagnostic state is retained even when a restoration wraps another failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryLowError {
    pub kind: ErrorKind,
    pub message: String,
    /// Last successfully observed value; it may predate an attempted write.
    pub observed: Option<u64>,
    pub cause: Option<Box<MemoryLowError>>,
}
impl MemoryLowError {
    fn new(kind: ErrorKind, message: impl Into<String>, observed: Option<u64>) -> Self {
        Self {
            kind,
            message: message.into(),
            observed,
            cause: None,
        }
    }
    fn restoration(self) -> Self {
        Self {
            kind: ErrorKind::RestorationFailure,
            message: "memory.low restoration failed".into(),
            observed: self.observed,
            cause: Some(Box::new(self)),
        }
    }
}

// Every method validates or uses the same held child; a fixture supplies deterministic I/O.
pub(crate) trait Access {
    fn verify(&mut self, child: ChildIdentity) -> Result<(), String>;
    fn read(&mut self) -> Result<u64, String>;
    fn prepare_write(&mut self) -> Result<(), String>;
    // An error or short write after this call begins is always ambiguous.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, String>;
    fn write_enabled(&self) -> bool;
}

pub(crate) fn read<A: Access>(io: &mut A, child: ChildIdentity) -> Result<u64, MemoryLowError> {
    io.verify(child)
        .map_err(|e| MemoryLowError::new(ErrorKind::IdentityConflict, e, None))?;
    let value = io
        .read()
        .map_err(|e| MemoryLowError::new(ErrorKind::RefusedBeforeWrite, e, None))?;
    io.verify(child)
        .map_err(|e| MemoryLowError::new(ErrorKind::IdentityConflict, e, Some(value)))?;
    Ok(value)
}

pub(crate) fn change<A: Access>(
    io: &mut A,
    unresolved: &mut bool,
    child: ChildIdentity,
    expected: u64,
    value: u64,
    authorized: bool,
    restoring: bool,
) -> Result<u64, MemoryLowError> {
    let result = change_inner(io, unresolved, child, expected, value, authorized);
    if restoring {
        result.map_err(MemoryLowError::restoration)
    } else {
        result
    }
}
fn change_inner<A: Access>(
    io: &mut A,
    unresolved: &mut bool,
    child: ChildIdentity,
    expected: u64,
    value: u64,
    authorized: bool,
) -> Result<u64, MemoryLowError> {
    if *unresolved
        || !authorized
        || !io.write_enabled()
        || value > MAX_LOW_BYTES
        || !value.is_multiple_of(4096)
    {
        return Err(MemoryLowError::new(
            ErrorKind::RefusedBeforeWrite,
            "authorization, bounds, disabled writer, or unresolved previous write",
            None,
        ));
    }
    let observed = read(io, child)?;
    if observed != expected {
        return Err(MemoryLowError::new(
            ErrorKind::CompetingValue,
            "memory.low differs from expected value",
            Some(observed),
        ));
    }
    io.prepare_write()
        .map_err(|e| MemoryLowError::new(ErrorKind::RefusedBeforeWrite, e, Some(observed)))?;
    io.verify(child)
        .map_err(|e| MemoryLowError::new(ErrorKind::IdentityConflict, e, Some(observed)))?;
    let current = io
        .read()
        .map_err(|e| MemoryLowError::new(ErrorKind::RefusedBeforeWrite, e, Some(observed)))?;
    if current != expected {
        return Err(MemoryLowError::new(
            ErrorKind::CompetingValue,
            "memory.low changed before write",
            Some(current),
        ));
    }
    io.verify(child)
        .map_err(|e| MemoryLowError::new(ErrorKind::IdentityConflict, e, Some(current)))?;
    // Latch before entering the syscall: all errors after this point require investigation.
    *unresolved = true;
    let text = value.to_string();
    let n = io
        .write(text.as_bytes())
        .map_err(|e| MemoryLowError::new(ErrorKind::WriteOutcomeAmbiguous, e, Some(observed)))?;
    if n != text.len() {
        return Err(MemoryLowError::new(
            ErrorKind::WriteOutcomeAmbiguous,
            "short memory.low write",
            Some(observed),
        ));
    }
    io.verify(child)
        .map_err(|e| MemoryLowError::new(ErrorKind::IdentityConflict, e, Some(observed)))?;
    let actual = io.read().map_err(|e| {
        MemoryLowError::new(
            ErrorKind::WriteOutcomeAmbiguous,
            format!("read-back failed: {e}"),
            Some(observed),
        )
    })?;
    io.verify(child)
        .map_err(|e| MemoryLowError::new(ErrorKind::IdentityConflict, e, Some(actual)))?;
    if actual != value {
        return Err(MemoryLowError::new(
            ErrorKind::ReadBackMismatch,
            "memory.low read-back differs",
            Some(actual),
        ));
    }
    *unresolved = false;
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    const CHILD: ChildIdentity = ChildIdentity {
        device: 1,
        inode: 2,
    };
    struct Fixture {
        parent_ok: bool,
        child: ChildIdentity,
        value: Option<u64>,
        permission: bool,
        mode: &'static str,
        writes: usize,
        reads: usize,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                parent_ok: true,
                child: CHILD,
                value: Some(4096),
                permission: true,
                mode: "ok",
                writes: 0,
                reads: 0,
            }
        }
    }
    impl Access for Fixture {
        fn verify(&mut self, child: ChildIdentity) -> Result<(), String> {
            if !self.parent_ok || self.child != child {
                Err("parent or child identity changed".into())
            } else {
                Ok(())
            }
        }
        fn read(&mut self) -> Result<u64, String> {
            self.reads += 1;
            if self.mode == "compete" && self.reads == 2 {
                self.value = Some(16384);
            }
            self.value.ok_or("missing memory.low".into())
        }
        fn prepare_write(&mut self) -> Result<(), String> {
            if self.permission {
                Ok(())
            } else {
                Err("permission denied".into())
            }
        }
        fn write(&mut self, bytes: &[u8]) -> Result<usize, String> {
            self.writes += 1;
            self.value = Some(std::str::from_utf8(bytes).unwrap().parse().unwrap());
            match self.mode {
                "error" => Err("injected failure after possible mutation".into()),
                "short" => Ok(bytes.len() - 1),
                "mismatch" => {
                    self.value = Some(12288);
                    Ok(bytes.len())
                }
                _ => Ok(bytes.len()),
            }
        }
        fn write_enabled(&self) -> bool {
            self.mode != "disabled"
        }
    }
    fn apply(f: &mut Fixture, latch: &mut bool) -> Result<u64, MemoryLowError> {
        change(f, latch, CHILD, 4096, 8192, true, false)
    }
    #[test]
    fn successful_read_and_write_readback() {
        let mut f = Fixture::new();
        assert_eq!(read(&mut f, CHILD), Ok(4096));
        let mut latch = false;
        assert_eq!(apply(&mut f, &mut latch), Ok(8192));
        assert!(!latch);
        assert_eq!(f.writes, 1);
    }
    #[test]
    fn refuses_authorization_identity_missing_and_permission() {
        let mut f = Fixture::new();
        let mut latch = false;
        assert_eq!(
            change(&mut f, &mut latch, CHILD, 4096, 8192, false, false)
                .unwrap_err()
                .kind,
            ErrorKind::RefusedBeforeWrite
        );
        assert_eq!(
            read(&mut f, ChildIdentity { inode: 3, ..CHILD })
                .unwrap_err()
                .kind,
            ErrorKind::IdentityConflict
        );
        f.parent_ok = false;
        assert_eq!(
            read(&mut f, CHILD).unwrap_err().kind,
            ErrorKind::IdentityConflict
        );
        f.parent_ok = true;
        f.child.inode = 3;
        assert_eq!(
            read(&mut f, CHILD).unwrap_err().kind,
            ErrorKind::IdentityConflict
        );
        f.child = CHILD;
        f.value = None;
        assert_eq!(
            read(&mut f, CHILD).unwrap_err().kind,
            ErrorKind::RefusedBeforeWrite
        );
        f.value = Some(4096);
        f.permission = false;
        assert_eq!(
            apply(&mut f, &mut latch).unwrap_err().kind,
            ErrorKind::RefusedBeforeWrite
        );
        assert_eq!(f.writes, 0);
        assert!(!latch);
        f.permission = true;
        f.mode = "disabled";
        assert_eq!(
            apply(&mut f, &mut latch).unwrap_err().kind,
            ErrorKind::RefusedBeforeWrite
        );
        assert_eq!(f.writes, 0);
        f.mode = "ok";
        assert_eq!(
            change(
                &mut f,
                &mut latch,
                CHILD,
                4096,
                MAX_LOW_BYTES + 4096,
                true,
                false
            )
            .unwrap_err()
            .kind,
            ErrorKind::RefusedBeforeWrite
        );
        assert_eq!(f.writes, 0);
    }
    #[test]
    fn ambiguous_short_mismatch_and_no_retry() {
        for (mode, kind) in [
            ("error", ErrorKind::WriteOutcomeAmbiguous),
            ("short", ErrorKind::WriteOutcomeAmbiguous),
            ("mismatch", ErrorKind::ReadBackMismatch),
        ] {
            let mut f = Fixture::new();
            f.mode = mode;
            let mut latch = false;
            let error = apply(&mut f, &mut latch).unwrap_err();
            assert_eq!(error.kind, kind, "{mode}");
            assert!(latch);
            assert_eq!(
                apply(&mut f, &mut latch).unwrap_err().kind,
                ErrorKind::RefusedBeforeWrite
            );
            assert_eq!(f.writes, 1);
            if mode == "mismatch" {
                assert_eq!(error.observed, Some(12288));
            }
        }
    }
    #[test]
    fn competing_value_and_restoration_failure_are_reported() {
        let mut f = Fixture::new();
        f.mode = "compete";
        let mut latch = false;
        let e = apply(&mut f, &mut latch).unwrap_err();
        assert_eq!(
            (e.kind, e.observed),
            (ErrorKind::CompetingValue, Some(16384))
        );
        assert_eq!(f.writes, 0);
        let mut f = Fixture::new();
        f.mode = "error";
        let e = change(&mut f, &mut latch, CHILD, 4096, 8192, true, true).unwrap_err();
        assert_eq!(e.kind, ErrorKind::RestorationFailure);
        assert_eq!(e.cause.unwrap().kind, ErrorKind::WriteOutcomeAmbiguous);
        assert!(latch);
    }
}
