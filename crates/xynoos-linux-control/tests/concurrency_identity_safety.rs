//! Comprehensive regression tests for CRITICAL-02 (TOCTOU & Concurrency Safety)
//! and CRITICAL-03 (Cgroup Identity Recycling & Stale Lease Invalidation).

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
    transaction::{check, run, Backend, Identity, Outcome, SustainedLease},
    Action,
};
use xynoos_pressureiq::system::SystemSnapshot;

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(500);

fn temp_test_dir(name: &str) -> PathBuf {
    let count = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "xynoos-test-concurrency-{name}-{}-{}",
        std::process::id(),
        count
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

#[derive(Debug, Clone)]
struct DynamicCgroupBackend {
    pub id: Identity,
    pub current_low: u64,
    pub current_high: u64,
    pub current_max: u64,
    pub writes_applied: Vec<(Action, u64)>,
    pub concurrent_mutation_on_write: Option<u64>,
}

impl DynamicCgroupBackend {
    pub fn new(inode: u64, initial_low: u64) -> Self {
        Self {
            id: Identity {
                device: 200,
                inode,
                control_inode: inode + 999,
                mount_id: Some(88),
            },
            current_low: initial_low,
            current_high: u64::MAX,
            current_max: u64::MAX,
            writes_applied: Vec::new(),
            concurrent_mutation_on_write: None,
        }
    }
}

impl Backend for DynamicCgroupBackend {
    fn identity(&mut self) -> Result<Identity, String> {
        Ok(self.id)
    }

    fn read_low(&mut self) -> Result<u64, String> {
        Ok(self.current_low)
    }

    fn write_low(&mut self, value: u64) -> Result<(), String> {
        if let Some(other) = self.concurrent_mutation_on_write {
            // Simulate another writer clobbering immediately after or during write
            self.current_low = other;
        } else {
            self.current_low = value;
        }
        self.writes_applied
            .push((Action::MemoryLow { bytes: value }, value));
        Ok(())
    }

    fn read_high(&mut self) -> Result<u64, String> {
        Ok(self.current_high)
    }

    fn write_high(&mut self, value: u64) -> Result<(), String> {
        self.current_high = value;
        self.writes_applied
            .push((Action::MemoryHigh { bytes: value }, value));
        Ok(())
    }

    fn read_max(&mut self) -> Result<u64, String> {
        Ok(self.current_max)
    }

    fn write_max(&mut self, value: u64) -> Result<(), String> {
        self.current_max = value;
        self.writes_applied
            .push((Action::MemoryMax { bytes: value }, value));
        Ok(())
    }
}

#[test]
fn test_cgroup_inode_recycling_detection_in_recovery() {
    let test_dir = temp_test_dir("inode-recycling");
    let journal_path = test_dir.join("lease_journal.json");

    // Original cgroup had inode 5001
    let original_id = Identity {
        device: 200,
        inode: 5001,
        control_inode: 6001,
        mount_id: Some(88),
    };

    let journal = TransactionJournal {
        schema_version: TransactionJournal::CURRENT_SCHEMA_VERSION,
        lease_id: "test-recycled-inode".into(),
        timestamp_epoch_secs: 1000,
        boot_id: current_boot_id(),
        owner_uid: unsafe { libc::geteuid() },
        process_pid: std::process::id(),
        process_start_ticks: None,
        authorized_root: None,
        identity: original_id,
        action: Action::MemoryLow {
            bytes: 32 * 1024 * 1024,
        },
        original_value: 16 * 1024 * 1024,
        requested_value: 32 * 1024 * 1024,
        status: LeasePhase::Applied,
        integrity_crc: 0,
    };
    journal.persist(&journal_path).unwrap();

    // Now cgroup was deleted and recreated at same path with new inode 5002!
    let mut recreated_backend = DynamicCgroupBackend::new(5002, 0);

    let rep = recover_durable_lease(&mut recreated_backend, &journal_path).unwrap();
    assert_eq!(rep.outcome, RecoveryOutcome::Invalidated);

    // CRITICAL-03: Must refuse to apply old values to a recreated cgroup with different inode!
    assert_eq!(recreated_backend.current_low, 0);
    assert!(rep.events.iter().any(|e| e.contains("identity mismatch")));

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_toctou_concurrent_writer_detected_as_conflict_without_blind_rollback() {
    let mut backend = DynamicCgroupBackend::new(7001, 16 * 1024 * 1024);
    let snap = xynoos_pressureiq::system::collect_system();

    // Configure backend so that during write, an external concurrent writer sets value to 999
    backend.concurrent_mutation_on_write = Some(999 * 1024 * 1024);

    let action = Action::MemoryLow {
        bytes: 32 * 1024 * 1024,
    };
    let rep = run(&mut backend, &snap, action, true);

    // CRITICAL-02 INVARIANT:
    // When readback verification detects divergence, outcome must be Conflict and must NOT
    // perform blind rollback clobbering the external writer's value!
    assert_eq!(rep.outcome, Outcome::Conflict);
    assert_eq!(backend.current_low, 999 * 1024 * 1024);
}

#[test]
fn test_sustained_lease_conflict_before_restore_refuses_blind_rollback() {
    let test_dir = temp_test_dir("restore-conflict");
    let journal_path = test_dir.join("lease_journal.json");
    let mut backend = DynamicCgroupBackend::new(7002, 16 * 1024 * 1024);
    let snap = xynoos_pressureiq::system::collect_system();

    let action = Action::MemoryLow {
        bytes: 32 * 1024 * 1024,
    };
    let mut lease =
        SustainedLease::acquire_durable(&mut backend, &snap, action, true, &journal_path)
            .expect("acquire must succeed");
    assert_eq!(backend.current_low, 32 * 1024 * 1024);

    // External administrator concurrently modifies the control file during active lease window
    backend.current_low = 64 * 1024 * 1024;

    // Call release: verification check before restore must catch the conflict!
    let rep = lease.release(&mut backend);
    assert_eq!(rep.outcome, Outcome::Conflict);

    // Baseline value must NOT blindly overwrite administrator's 64 MiB!
    assert_eq!(backend.current_low, 64 * 1024 * 1024);

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_cgroup_mount_id_change_detected() {
    let test_dir = temp_test_dir("mount-id");
    let journal_path = test_dir.join("lease_journal.json");

    let original_id = Identity {
        device: 200,
        inode: 8001,
        control_inode: 9001,
        mount_id: Some(101),
    };

    let journal = TransactionJournal {
        schema_version: TransactionJournal::CURRENT_SCHEMA_VERSION,
        lease_id: "test-mount-id".into(),
        timestamp_epoch_secs: 1000,
        boot_id: current_boot_id(),
        owner_uid: unsafe { libc::geteuid() },
        process_pid: std::process::id(),
        process_start_ticks: None,
        authorized_root: None,
        identity: original_id,
        action: Action::MemoryLow {
            bytes: 32 * 1024 * 1024,
        },
        original_value: 16 * 1024 * 1024,
        requested_value: 32 * 1024 * 1024,
        status: LeasePhase::Applied,
        integrity_crc: 0,
    };
    journal.persist(&journal_path).unwrap();

    // Backend on a different mount ID
    let mut diff_mount_backend = DynamicCgroupBackend::new(8001, 32 * 1024 * 1024);
    diff_mount_backend.id.mount_id = Some(202);

    let rep = recover_durable_lease(&mut diff_mount_backend, &journal_path).unwrap();
    assert_eq!(rep.outcome, RecoveryOutcome::Invalidated);

    let _ = fs::remove_dir_all(&test_dir);
}
