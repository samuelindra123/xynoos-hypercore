#![forbid(unsafe_code)]

use std::fs;
use std::sync::{atomic::AtomicBool, Arc};
use xynoos_aura::{
    daemon::{JournalStatus, PidLock, TransactionJournal},
    AdaptiveCompressionConfig, AuraAdaptiveCoordinator, AuraDaemon, DaemonConfig, DaemonMode,
    MultiResourceState, PolicyConfig, PressureStabilizer, ShadowCalibrator, TraceRecord,
    WorkloadTrace,
};
use xynoos_core::{
    CodecId, EvictionPolicy, MemoryPressure, MigrationBudget, ResourcePressureLevel,
};
use xynoos_hypermemory::PageStore;
use xynoos_linux_control::{
    transaction::{Backend, Identity},
    Action,
};

struct MockCgroupBackend {
    id: Identity,
    low: u64,
    high: u64,
    max: u64,
}

impl Backend for MockCgroupBackend {
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

fn structured_page(seed_idx: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    for i in 0..128 {
        let s = format!(
            "{{\"idx\":{},\"row\":{},\"module\":\"xynoos_aura\",\"status\":\"ok\"}}\n",
            seed_idx, i
        );
        out.extend_from_slice(s.as_bytes());
    }
    out.resize(4096, b' ');
    out
}

fn random_page(mut seed: u64) -> Vec<u8> {
    (0..4096)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect()
}

#[test]
fn milestone_g_adaptive_compression_and_cpu_throttling_end_to_end() {
    let mut store = PageStore::adaptive(4096, 16, 512 * 1024).unwrap();
    let mut ids = Vec::new();
    for i in 0..6 {
        ids.push(
            store
                .insert_with_codec(&structured_page(i), CodecId::Raw, EvictionPolicy::Reject)
                .unwrap(),
        );
    }
    let rand_id = store
        .insert_with_codec(
            &random_page(0xCAFE_1234),
            CodecId::Raw,
            EvictionPolicy::Reject,
        )
        .unwrap();

    // Advance tick so pages become cold and cooldown clears
    for _ in 0..80 {
        let _ = store.get(ids[0]).unwrap();
    }

    let mut coord = AuraAdaptiveCoordinator::new(
        AdaptiveCompressionConfig::default(),
        PressureStabilizer::new(3, 0),
        MigrationBudget::UNLIMITED,
    );

    // 1. High CPU + High Memory pressure -> must NOT use Lz4Hc
    let high_cpu_mem = MultiResourceState {
        memory_pressure: MemoryPressure::High,
        cpu_pressure: ResourcePressureLevel::High,
        io_pressure: ResourcePressureLevel::Low,
        available_ram_bytes: Some(256 * 1024 * 1024),
        telemetry_valid: true,
    };
    let rep1 = coord.step_adaptive(&mut store, &high_cpu_mem).unwrap();
    assert_eq!(rep1.migrated_to_lz4hc, 0);
    assert!(rep1.migrated_to_lz4 >= 1);

    // 2. Low CPU + Critical Memory pressure after cooldown -> cold pages can use Lz4Hc
    for _ in 0..10 {
        let _ = store.get(ids[0]).unwrap();
    }
    let low_cpu_crit_mem = MultiResourceState {
        memory_pressure: MemoryPressure::Critical,
        cpu_pressure: ResourcePressureLevel::Low,
        io_pressure: ResourcePressureLevel::Low,
        available_ram_bytes: Some(64 * 1024 * 1024),
        telemetry_valid: true,
    };
    let rep2 = coord.step_adaptive(&mut store, &low_cpu_crit_mem).unwrap();
    assert!(
        rep2.migrated_to_lz4hc >= 1 || rep2.retained_or_preserved >= 1,
        "rep2={rep2:?}"
    );

    // 3. Incompressible random page must stay Raw
    assert_eq!(store.page_codec(rand_id).unwrap(), CodecId::Raw);

    // 4. Overhead accounting reflects real metadata and scratch
    let acct = store.overhead_accounting();
    assert_eq!(
        acct.total_accounted_bytes,
        acct.allocated_capacity_bytes + acct.metadata_bytes + acct.scratch_buffer_bytes
    );
    assert!(acct.net_savings_bytes > 0);
}

#[test]
fn milestone_g_shadow_calibration_and_policy_rollback() {
    let mut trace = WorkloadTrace::new("mixed-desktop-trace".into());
    for i in 0..24u64 {
        let phase = i % 6;
        let (avail, severe, thrash) = match phase {
            0..=2 => (70.0, false, false),
            3..=4 => (18.0, true, false),
            _ => (4.0, true, true),
        };
        trace.append(TraceRecord {
            step_index: i,
            relative_timestamp_ms: i * 1000,
            mem_total_bytes: 4 * 1024 * 1024 * 1024,
            mem_available_bytes: (avail * 40.0 * 1024.0 * 1024.0) as u64,
            available_pct: avail,
            psi_some_avg10: if severe { 16.0 } else { 0.5 },
            psi_full_avg10: if thrash { 8.0 } else { 0.0 },
            swap_thrashing: thrash,
            severe_psi: severe,
            high_page_fault_rate: severe,
            cgroup_usage_bytes: 256 * 1024 * 1024,
        });
    }

    let pages: Vec<Vec<u8>> = (0..12).map(structured_page).collect();
    let cal = ShadowCalibrator::calibrate(&trace, &pages).unwrap();
    assert!(cal.advisory_only);
    assert_eq!(cal.measurement_kind, "counterfactual_replay_estimate");
    assert_eq!(cal.rollback_config, PolicyConfig::safe_default());
    assert_eq!(cal.evaluation_comparisons.len(), 4);
}

#[test]
fn milestone_g_daemon_hardening_and_fault_injection_recovery() {
    let temp_dir = std::env::temp_dir().join(format!("xynoos_g_fault_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let pid_path = temp_dir.join("hypercore.pid");
    let journal_path = temp_dir.join("journal.json");

    let lock = PidLock::acquire(pid_path.clone()).unwrap();
    assert!(PidLock::acquire(pid_path.clone()).is_err());

    let mut daemon = AuraDaemon::new(DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        interval_ms: 100,
        pid_file: Some(pid_path.clone()),
        journal_file: Some(journal_path.clone()),
        ..Default::default()
    })
    .unwrap();

    // Pending journal where kernel was NOT written yet (current == original) -> clean no-op
    let id = Identity {
        device: 7,
        inode: 8,
        control_inode: 9,
        mount_id: None,
    };
    let mut backend = MockCgroupBackend {
        id,
        low: 4096,
        high: 16384,
        max: 65536,
    };
    let pending = TransactionJournal {
        schema_version: 1,
        lease_id: "test-lease".into(),
        timestamp_epoch_secs: 1700000001,
        boot_id: None,
        owner_uid: 1000,
        process_pid: std::process::id(),
        process_start_ticks: None,
        authorized_root: None,
        identity: id,
        action: Action::MemoryHigh { bytes: 32768 },
        original_value: 16384,
        requested_value: 32768,
        status: JournalStatus::Pending,
        integrity_crc: 0,
    };
    pending.persist(&journal_path).unwrap();
    assert!(!daemon.reconcile_crash_recovery(&mut backend).unwrap());
    assert!(!journal_path.exists());

    // Simulate real daemon run bounded to 3 cycles using live PressureIQ telemetry
    let stop = Arc::new(AtomicBool::new(false));
    let diag = daemon.run_with_signal(stop, Some(3)).unwrap();
    assert_eq!(diag.cycles_completed, 3);
    assert!(diag.rss_within_budget);

    drop(lock);
    let _ = fs::remove_dir_all(&temp_dir);
}
