//! Comprehensive regression tests for CRITICAL-01: Durable SustainedLease crash recovery.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use xynoos_linux_control::{
    journal::{
        current_boot_id, recover_durable_lease, JournalStatus, LeasePhase, RecoveryOutcome,
        TransactionJournal,
    },
    transaction::{Backend, Identity, SustainedLease},
    Action,
};
use xynoos_pressureiq::system::SystemSnapshot;

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(100);

fn temp_test_dir(name: &str) -> PathBuf {
    let count = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "xynoos-test-durable-{name}-{}-{}",
        std::process::id(),
        count
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Mock cgroup backend tracking operations, current value, and simulated mutations.
#[derive(Debug, Clone)]
struct MockCgroupBackend {
    pub id: Identity,
    pub current_low: u64,
    pub current_high: u64,
    pub current_max: u64,
    pub write_calls: Vec<(Action, u64)>,
    pub fail_writes: bool,
    pub fail_identity: bool,
}

impl MockCgroupBackend {
    pub fn new(inode: u64, initial_low: u64) -> Self {
        Self {
            id: Identity {
                device: 100,
                inode,
                control_inode: inode + 500,
                mount_id: Some(42),
            },
            current_low: initial_low,
            current_high: u64::MAX,
            current_max: u64::MAX,
            write_calls: Vec::new(),
            fail_writes: false,
            fail_identity: false,
        }
    }
}

impl Backend for MockCgroupBackend {
    fn identity(&mut self) -> Result<Identity, String> {
        if self.fail_identity {
            return Err("simulated identity failure".into());
        }
        Ok(self.id)
    }

    fn read_low(&mut self) -> Result<u64, String> {
        Ok(self.current_low)
    }

    fn write_low(&mut self, value: u64) -> Result<(), String> {
        if self.fail_writes {
            return Err("simulated kernel write error".into());
        }
        self.write_calls
            .push((Action::MemoryLow { bytes: value }, value));
        self.current_low = value;
        Ok(())
    }

    fn read_high(&mut self) -> Result<u64, String> {
        Ok(self.current_high)
    }

    fn write_high(&mut self, value: u64) -> Result<(), String> {
        if self.fail_writes {
            return Err("simulated kernel write error".into());
        }
        self.write_calls
            .push((Action::MemoryHigh { bytes: value }, value));
        self.current_high = value;
        Ok(())
    }

    fn read_max(&mut self) -> Result<u64, String> {
        Ok(self.current_max)
    }

    fn write_max(&mut self, value: u64) -> Result<(), String> {
        if self.fail_writes {
            return Err("simulated kernel write error".into());
        }
        self.write_calls
            .push((Action::MemoryMax { bytes: value }, value));
        self.current_max = value;
        Ok(())
    }
}

#[test]
fn test_normal_durable_lease_lifecycle() {
    let test_dir = temp_test_dir("normal-lifecycle");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = MockCgroupBackend::new(1001, 16 * 1024 * 1024);
    let snap = xynoos_pressureiq::system::collect_system();

    // 1. Acquire durable lease
    let action = Action::MemoryLow {
        bytes: 32 * 1024 * 1024,
    };
    let mut lease =
        SustainedLease::acquire_durable(&mut backend, &snap, action, true, &journal_path)
            .expect("durable acquire must succeed");

    assert_eq!(backend.current_low, 32 * 1024 * 1024);
    assert!(
        journal_path.exists(),
        "journal file must exist after acquire"
    );

    // Inspect persisted journal
    let loaded = TransactionJournal::load(&journal_path)
        .expect("load must succeed")
        .expect("journal must exist");
    assert_eq!(loaded.status, LeasePhase::Applied);
    assert_eq!(loaded.original_value, 16 * 1024 * 1024);
    assert_eq!(loaded.requested_value, 32 * 1024 * 1024);

    // 2. Release durable lease
    let report = lease.release(&mut backend);
    assert_eq!(
        report.outcome,
        xynoos_linux_control::transaction::Outcome::Restored
    );
    assert_eq!(backend.current_low, 16 * 1024 * 1024);
    assert!(
        !journal_path.exists(),
        "journal must be cleaned up after clean release"
    );

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_durable_lease_prevents_mutation_if_journal_fails() {
    let test_dir = temp_test_dir("journal-fail");
    // Point journal to a non-creatable path (e.g. child of a regular file)
    let barrier_file = test_dir.join("barrier");
    fs::write(&barrier_file, b"data").unwrap();
    let invalid_journal = barrier_file.join("sub").join("lease_journal.json");

    let mut backend = MockCgroupBackend::new(1002, 16 * 1024 * 1024);
    let snap = xynoos_pressureiq::system::collect_system();

    let action = Action::MemoryLow {
        bytes: 32 * 1024 * 1024,
    };
    let res = SustainedLease::acquire_durable(&mut backend, &snap, action, true, &invalid_journal);
    assert!(res.is_err(), "must fail if journal persistence fails");

    // CRITICAL-01 INVARIANT: Kernel state must remain unchanged if journal cannot be written!
    assert_eq!(backend.current_low, 16 * 1024 * 1024);
    assert_eq!(
        backend.write_calls.len(),
        0,
        "no kernel mutation permitted if journal fails"
    );

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_crash_recovery_after_process_termination() {
    let test_dir = temp_test_dir("crash-recovery");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = MockCgroupBackend::new(1003, 16 * 1024 * 1024);
    let snap = xynoos_pressureiq::system::collect_system();

    // Acquire durable lease
    let action = Action::MemoryLow {
        bytes: 32 * 1024 * 1024,
    };
    let lease = SustainedLease::acquire_durable(&mut backend, &snap, action, true, &journal_path)
        .expect("acquire must succeed");

    assert_eq!(backend.current_low, 32 * 1024 * 1024);

    // Simulate process termination (e.g. SIGKILL): lease is dropped without release()
    drop(lease);

    // The kernel remains modified at 32 MiB, but durable journal was written to disk
    assert_eq!(backend.current_low, 32 * 1024 * 1024);
    assert!(journal_path.exists());

    // Next startup or supervisor recovery executes:
    let rep = recover_durable_lease(&mut backend, &journal_path).expect("recovery must succeed");
    assert_eq!(rep.outcome, RecoveryOutcome::Restored);
    assert_eq!(
        backend.current_low,
        16 * 1024 * 1024,
        "baseline must be restored"
    );
    assert!(!journal_path.exists(), "journal cleaned up after recovery");

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_recovery_is_idempotent() {
    let test_dir = temp_test_dir("idempotent");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = MockCgroupBackend::new(1004, 16 * 1024 * 1024);
    let snap = xynoos_pressureiq::system::collect_system();

    let action = Action::MemoryLow {
        bytes: 32 * 1024 * 1024,
    };
    let lease = SustainedLease::acquire_durable(&mut backend, &snap, action, true, &journal_path)
        .expect("acquire must succeed");
    drop(lease);

    // First recovery pass
    let rep1 = recover_durable_lease(&mut backend, &journal_path).unwrap();
    assert_eq!(rep1.outcome, RecoveryOutcome::Restored);
    assert_eq!(backend.current_low, 16 * 1024 * 1024);

    // Second recovery pass: journal is already removed, returns NoJournal safely
    let rep2 = recover_durable_lease(&mut backend, &journal_path).unwrap();
    assert_eq!(rep2.outcome, RecoveryOutcome::NoJournal);
    assert_eq!(backend.current_low, 16 * 1024 * 1024);

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_recovery_rejects_boot_id_mismatch_fail_closed() {
    let test_dir = temp_test_dir("boot-mismatch");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = MockCgroupBackend::new(1005, 16 * 1024 * 1024);

    // Manually create a journal record from a different previous boot
    let journal = TransactionJournal {
        schema_version: TransactionJournal::CURRENT_SCHEMA_VERSION,
        lease_id: "test-lease-old-boot".into(),
        timestamp_epoch_secs: 1000,
        boot_id: Some("00000000-0000-0000-0000-000000000000".into()),
        owner_uid: 1000,
        process_pid: 9999,
        process_start_ticks: Some(1234),
        authorized_root: None,
        identity: backend.id,
        action: Action::MemoryLow {
            bytes: 32 * 1024 * 1024,
        },
        original_value: 16 * 1024 * 1024,
        requested_value: 32 * 1024 * 1024,
        status: LeasePhase::Applied,
        integrity_crc: 0,
    };
    journal.persist(&journal_path).unwrap();

    backend.current_low = 32 * 1024 * 1024;

    // Recovery must detect that boot_id does not match current system boot
    let rep = recover_durable_lease(&mut backend, &journal_path).unwrap();
    assert_eq!(rep.outcome, RecoveryOutcome::Invalidated);

    // CRITICAL INVARIANT: must NOT restore old configuration from previous boot!
    assert_eq!(backend.current_low, 32 * 1024 * 1024);

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_recovery_rejects_external_conflict_fail_closed() {
    let test_dir = temp_test_dir("external-conflict");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = MockCgroupBackend::new(1006, 16 * 1024 * 1024);

    let journal = TransactionJournal {
        schema_version: TransactionJournal::CURRENT_SCHEMA_VERSION,
        lease_id: "test-lease-conflict".into(),
        timestamp_epoch_secs: 1000,
        boot_id: current_boot_id(),
        owner_uid: unsafe { libc::geteuid() },
        process_pid: std::process::id(),
        process_start_ticks: None,
        authorized_root: None,
        identity: backend.id,
        action: Action::MemoryLow {
            bytes: 32 * 1024 * 1024,
        },
        original_value: 16 * 1024 * 1024,
        requested_value: 32 * 1024 * 1024,
        status: LeasePhase::Applied,
        integrity_crc: 0,
    };
    journal.persist(&journal_path).unwrap();

    // External administrator changed memory.low to 48 MiB!
    backend.current_low = 48 * 1024 * 1024;

    let rep = recover_durable_lease(&mut backend, &journal_path).unwrap();
    assert_eq!(rep.outcome, RecoveryOutcome::Conflict);

    // Invariant: external admin modification must NOT be overwritten!
    assert_eq!(backend.current_low, 48 * 1024 * 1024);

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_recovery_quarantines_corrupted_journal() {
    let test_dir = temp_test_dir("corrupt-journal");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = MockCgroupBackend::new(1007, 16 * 1024 * 1024);

    // Write corrupted non-JSON bytes
    fs::write(&journal_path, b"{{{{invalid-corrupt-data").unwrap();

    let res = recover_durable_lease(&mut backend, &journal_path);
    assert!(res.is_err(), "must return error on corrupted journal");

    // Corrupted file must be quarantined
    let entries: Vec<_> = fs::read_dir(&test_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.iter().any(|name| name.contains("corrupt")));

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_recovery_quarantines_crc_mismatch() {
    let test_dir = temp_test_dir("crc-mismatch");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = MockCgroupBackend::new(1008, 16 * 1024 * 1024);

    let mut journal = TransactionJournal {
        schema_version: TransactionJournal::CURRENT_SCHEMA_VERSION,
        lease_id: "test-crc".into(),
        timestamp_epoch_secs: 1000,
        boot_id: current_boot_id(),
        owner_uid: unsafe { libc::geteuid() },
        process_pid: std::process::id(),
        process_start_ticks: None,
        authorized_root: None,
        identity: backend.id,
        action: Action::MemoryLow {
            bytes: 32 * 1024 * 1024,
        },
        original_value: 16 * 1024 * 1024,
        requested_value: 32 * 1024 * 1024,
        status: LeasePhase::Applied,
        integrity_crc: 0,
    };
    journal.persist(&journal_path).unwrap();

    // Tamper with content on disk while leaving old CRC
    let mut text = fs::read_to_string(&journal_path).unwrap();
    text = text.replace("33554432", "33554433"); // Change requested value
    fs::write(&journal_path, text).unwrap();

    let res = recover_durable_lease(&mut backend, &journal_path);
    assert!(res.is_err(), "must fail CRC check");

    let entries: Vec<_> = fs::read_dir(&test_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.iter().any(|name| name.contains("corrupt.crc")));

    let _ = fs::remove_dir_all(&test_dir);
}
