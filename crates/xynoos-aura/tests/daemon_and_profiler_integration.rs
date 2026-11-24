#![forbid(unsafe_code)]
use std::fs;
use xynoos_aura::{
    AdaptiveTieringPolicy, AuraDaemon, AuraKernelCoordinator, AuraTieringCoordinator, DaemonConfig,
    DaemonMode, ShadowPlayer, TraceRecord, TransactionJournal, WorkloadTrace,
};
use xynoos_core::MemoryPressure;
use xynoos_hypermemory::PageStore;
use xynoos_linux_control::{
    transaction::{Backend, Identity},
    Action,
};
use xynoos_pressureiq::intelligence::Diagnosis;

struct MockBackend {
    pub id: Identity,
    pub low: u64,
    pub high: u64,
    pub max: u64,
}

impl Backend for MockBackend {
    fn identity(&mut self) -> Result<Identity, String> {
        Ok(self.id)
    }
    fn read_low(&mut self) -> Result<u64, String> {
        Ok(self.low)
    }
    fn write_low(&mut self, val: u64) -> Result<(), String> {
        self.low = val;
        Ok(())
    }
    fn read_high(&mut self) -> Result<u64, String> {
        Ok(self.high)
    }
    fn write_high(&mut self, val: u64) -> Result<(), String> {
        self.high = val;
        Ok(())
    }
    fn read_max(&mut self) -> Result<u64, String> {
        Ok(self.max)
    }
    fn write_max(&mut self, val: u64) -> Result<(), String> {
        self.max = val;
        Ok(())
    }
}

#[test]
fn test_milestone_f_daemon_modes_and_safety_invariants() {
    let config = DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        ..Default::default()
    };
    let mut daemon = AuraDaemon::new(config).unwrap();

    let diag = Diagnosis {
        classes: vec!["swap_thrashing".into()],
        evidence: Vec::new(),
        limitations: Vec::new(),
        available_pct: Some(15.0),
        rates_per_second: std::collections::BTreeMap::new(),
        disk_bytes_per_second: std::collections::BTreeMap::new(),
        zram_ratios: std::collections::BTreeMap::new(),
        interval_seconds: Some(1.0),
        reliable: true,
    };

    let mut backend = MockBackend {
        id: Identity {
            device: 10,
            inode: 20,
            control_inode: 30,
        },
        low: 10 * 1024 * 1024,
        high: 100 * 1024 * 1024,
        max: 500 * 1024 * 1024,
    };

    // 1. Observe-only: never mutates backend
    let diag_obs = daemon.step(&diag, None, Some(&mut backend)).unwrap();
    assert_eq!(diag_obs.mode, DaemonMode::ObserveOnly);
    assert_eq!(diag_obs.cycles_completed, 1);
    assert_eq!(backend.low, 10 * 1024 * 1024);
    assert_eq!(backend.high, 100 * 1024 * 1024);

    // 2. Advisory: emits recommendation, still does not write cgroup
    daemon.config.mode = DaemonMode::Advisory;
    let diag_adv = daemon.step(&diag, None, Some(&mut backend)).unwrap();
    assert_eq!(diag_adv.mode, DaemonMode::Advisory);
    assert_eq!(diag_adv.cycles_completed, 2);
    assert!(diag_adv.last_recommendation.is_some());
    assert_eq!(backend.low, 10 * 1024 * 1024);
}

#[test]
fn test_milestone_f_crash_recovery_and_rollback() {
    let temp_dir = std::env::temp_dir().join(format!("xynoos_f_crash_{}", std::process::id()));
    let journal_file = temp_dir.join("recovery_journal.json");

    let config = DaemonConfig {
        journal_file: Some(journal_file.clone()),
        ..Default::default()
    };
    let mut daemon = AuraDaemon::new(config).unwrap();

    let target_id = Identity {
        device: 100,
        inode: 200,
        control_inode: 300,
    };
    let mut backend = MockBackend {
        id: target_id,
        low: 64 * 1024 * 1024, // modified state from ungraceful exit
        high: 0,
        max: 0,
    };

    // Tulis jurnal simulasi crash
    let journal = TransactionJournal {
        timestamp_epoch_secs: 1700000000,
        identity: target_id,
        action: Action::MemoryLow {
            bytes: 64 * 1024 * 1024,
        },
        original_value: 16 * 1024 * 1024, // baseline awal
        requested_value: 64 * 1024 * 1024,
        status: xynoos_aura::daemon::JournalStatus::Applied,
    };
    journal.persist(&journal_file).unwrap();

    // Rekonsiliasi startup
    let recovered = daemon.reconcile_crash_recovery(&mut backend).unwrap();
    assert!(recovered);
    assert_eq!(backend.low, 16 * 1024 * 1024); // baseline pulih
    assert!(!journal_file.exists()); // jurnal bersih

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_milestone_f_shadow_profiler_and_counterfactual_replay() {
    let mut trace = WorkloadTrace::new("production-mock-trace".into());
    for i in 0..10 {
        let (avail, swap) = if i < 5 { (60.0, false) } else { (12.0, true) };
        trace.append(TraceRecord {
            step_index: i,
            relative_timestamp_ms: i * 5000,
            mem_total_bytes: 4 * 1024 * 1024 * 1024,
            mem_available_bytes: (avail * 40.0 * 1024.0 * 1024.0) as u64,
            available_pct: avail,
            psi_some_avg10: if swap { 22.0 } else { 1.0 },
            psi_full_avg10: if swap { 10.0 } else { 0.0 },
            swap_thrashing: swap,
            severe_psi: swap,
            high_page_fault_rate: swap,
            cgroup_usage_bytes: 128 * 1024 * 1024,
        });
    }

    assert_eq!(trace.total_steps, 10);
    let json = trace.to_json().unwrap();
    let loaded = WorkloadTrace::from_json(&json).unwrap();
    assert_eq!(loaded.records.len(), 10);

    let tier_coord = AuraTieringCoordinator::new(AdaptiveTieringPolicy::default());
    let kernel_coord = AuraKernelCoordinator::new(tier_coord);
    let mut store = PageStore::with_capacity(64).unwrap();

    // Populasi data uji
    for _ in 0..8 {
        let page = [123u8; 4096];
        store.insert(&page).unwrap();
    }

    let report = ShadowPlayer::replay(&loaded, &kernel_coord, &mut store).unwrap();
    assert_eq!(report.total_steps_replayed, 10);
    assert_eq!(report.simulated_duration_ms, 45000);
    assert_eq!(report.peak_pressure, MemoryPressure::Critical);
    assert!(report.advisory_tuning_recommendations_count > 0);
}
