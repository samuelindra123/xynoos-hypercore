#![forbid(unsafe_code)]

use std::fs;
use xynoos_aura::{
    daemon::{JournalStatus, PidLock, TransactionJournal},
    AuraDaemon, DaemonConfig, DaemonMode,
};
use xynoos_linux_control::{
    transaction::{Backend, Identity, Outcome, SustainedLease},
    Action,
};
use xynoos_pressureiq::intelligence::Diagnosis;

struct SpyBackend {
    id: Identity,
    low: u64,
    high: u64,
    max: u64,
    writes: usize,
}

impl Backend for SpyBackend {
    fn identity(&mut self) -> Result<Identity, String> {
        Ok(self.id)
    }
    fn read_low(&mut self) -> Result<u64, String> {
        Ok(self.low)
    }
    fn write_low(&mut self, value: u64) -> Result<(), String> {
        self.writes += 1;
        self.low = value;
        Ok(())
    }
    fn read_high(&mut self) -> Result<u64, String> {
        Ok(self.high)
    }
    fn write_high(&mut self, value: u64) -> Result<(), String> {
        self.writes += 1;
        self.high = value;
        Ok(())
    }
    fn read_max(&mut self) -> Result<u64, String> {
        Ok(self.max)
    }
    fn write_max(&mut self, value: u64) -> Result<(), String> {
        self.writes += 1;
        self.max = value;
        Ok(())
    }
}

fn critical_diag() -> Diagnosis {
    Diagnosis {
        classes: vec!["swap_thrashing".into()],
        evidence: Vec::new(),
        limitations: Vec::new(),
        available_pct: Some(3.0),
        rates_per_second: std::collections::BTreeMap::new(),
        disk_bytes_per_second: std::collections::BTreeMap::new(),
        zram_ratios: std::collections::BTreeMap::new(),
        interval_seconds: Some(1.0),
        reliable: true,
    }
}

#[test]
fn observe_only_and_advisory_never_mutate_kernel_even_under_critical_pressure() {
    let mut snap = xynoos_pressureiq::system::collect_system();
    snap.memory.psi.result = Ok(xynoos_pressureiq::parse_psi(
        "some avg10=50.0 avg60=25.0 avg300=10.0 total=5000\n",
    )
    .unwrap());
    snap.memory.mem.result = Ok(xynoos_pressureiq::parse_meminfo(
        "MemTotal: 4096000 kB\nMemAvailable: 128000 kB\n",
    )
    .unwrap());

    for mode in [DaemonMode::ObserveOnly, DaemonMode::Advisory] {
        let mut daemon = AuraDaemon::new(DaemonConfig {
            mode,
            journal_file: None,
            ..Default::default()
        })
        .unwrap();
        let mut spy = SpyBackend {
            id: Identity {
                device: 1,
                inode: 10,
                control_inode: 20,
            },
            low: 8 * 1024 * 1024,
            high: 64 * 1024 * 1024,
            max: 128 * 1024 * 1024,
            writes: 0,
        };
        let d = daemon
            .step(&critical_diag(), Some(&snap), Some(&mut spy))
            .unwrap();
        assert_eq!(d.mode, mode);
        assert_eq!(spy.writes, 0, "mode {mode:?} wrote to kernel backend!");
    }
}

#[test]
fn sustained_lease_and_crash_journal_lifecycle_and_fail_closed_cases() {
    let dir = std::env::temp_dir().join(format!("xynoos_h_safety_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let journal_path = dir.join("lease_journal.json");
    let pid_path = dir.join("daemon.pid");

    let mut snap = xynoos_pressureiq::system::collect_system();
    snap.memory.psi.result =
        Ok(xynoos_pressureiq::parse_psi("some avg10=1.0 avg60=0.5 avg300=0.0 total=10\n").unwrap());
    snap.memory.mem.result = Ok(xynoos_pressureiq::parse_meminfo(
        "MemTotal: 4096000 kB\nMemAvailable: 2048000 kB\n",
    )
    .unwrap());

    let id = Identity {
        device: 42,
        inode: 100,
        control_inode: 200,
    };
    let mut backend = SpyBackend {
        id,
        low: 0,
        high: u64::MAX,
        max: u64::MAX,
        writes: 0,
    };

    // Unauthorized SustainedLease must fail without writing
    let err = SustainedLease::acquire(
        &mut backend,
        &snap,
        Action::MemoryLow {
            bytes: 16 * 1024 * 1024,
        },
        false,
    )
    .unwrap_err();
    assert_eq!(err.outcome, Outcome::Declined);
    assert_eq!(backend.writes, 0);

    // Authorized SustainedLease acquires, writes journal Applied, then simulates crash & recovery
    let mut lease = SustainedLease::acquire(
        &mut backend,
        &snap,
        Action::MemoryLow {
            bytes: 16 * 1024 * 1024,
        },
        true,
    )
    .unwrap();
    assert_eq!(backend.low, 16 * 1024 * 1024);

    let journal = TransactionJournal {
        schema_version: 1,
        timestamp_epoch_secs: 1_700_000_000,
        boot_id: None,
        process_pid: std::process::id(),
        process_start_ticks: None,
        identity: lease.identity(),
        action: lease.action(),
        original_value: lease.original(),
        requested_value: lease.requested(),
        status: JournalStatus::Applied,
        integrity_crc: 0,
    };
    journal.persist(&journal_path).unwrap();

    // Boot ID mismatch must fail-closed
    let mut wrong_boot = journal.clone();
    wrong_boot.boot_id = Some("00000000-0000-0000-0000-000000000000".into());
    wrong_boot.persist(&journal_path).unwrap();
    let mut daemon = AuraDaemon::new(DaemonConfig {
        journal_file: Some(journal_path.clone()),
        pid_file: Some(pid_path.clone()),
        ..Default::default()
    })
    .unwrap();
    if fs::read_to_string("/proc/sys/kernel/random/boot_id").is_ok() {
        assert!(daemon.reconcile_crash_recovery(&mut backend).is_err());
        assert_eq!(backend.low, 16 * 1024 * 1024);
    }

    // Valid boot_id + matching identity recovers original value
    journal.persist(&journal_path).unwrap();
    let recovered = daemon.reconcile_crash_recovery(&mut backend).unwrap();
    assert!(recovered);
    assert_eq!(backend.low, 0);
    assert!(!journal_path.exists());

    // Normal release on unrecovered active lease detects external restoration cleanly
    let rep = lease.release(&mut backend);
    assert_eq!(rep.outcome, Outcome::Conflict);

    // Repeated PidLock acquire/drop across restarts
    for _ in 0..3 {
        let lock = PidLock::acquire(pid_path.clone()).unwrap();
        assert!(pid_path.exists());
        drop(lock);
        assert!(!pid_path.exists());
    }

    let _ = fs::remove_dir_all(&dir);
}
