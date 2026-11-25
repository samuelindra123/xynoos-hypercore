#![forbid(unsafe_code)]
use crate::{
    compression_policy::{AdaptiveCompressionConfig, MultiResourceState},
    interplay::{AuraKernelCoordinator, KernelTuningRecommendation},
    tiering::{
        AdaptiveTieringPolicy, AuraAdaptiveCoordinator, AuraTieringCoordinator, PressureStabilizer,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};
use xynoos_core::{CodecId, MemoryPressure, MigrationBudget, ResourcePressureLevel};
use xynoos_hypermemory::PageStore;
use xynoos_pressureiq::intelligence::Diagnosis;

/// Rekaman telemetri beban kerja sadar-privasi (*privacy-aware*).
/// HANYA mencatat metadata performa dan tekanan kernel, sama sekali tidak merekam
/// konten buffer aplikasi, kredensial, atau data privat pengguna.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceRecord {
    pub step_index: u64,
    pub relative_timestamp_ms: u64,
    pub mem_total_bytes: u64,
    pub mem_available_bytes: u64,
    pub available_pct: f64,
    pub psi_some_avg10: f64,
    pub psi_full_avg10: f64,
    pub swap_thrashing: bool,
    pub severe_psi: bool,
    pub high_page_fault_rate: bool,
    pub cgroup_usage_bytes: u64,
}

impl TraceRecord {
    pub fn to_diagnosis(&self) -> Diagnosis {
        let mut classes = Vec::new();
        if self.swap_thrashing {
            classes.push("swap_thrashing".into());
        } else if self.severe_psi {
            classes.push("sustained_pressure".into());
        } else if self.psi_some_avg10 > 10.0 {
            classes.push("elevated_pressure".into());
        } else {
            classes.push("normal".into());
        }
        Diagnosis {
            classes,
            evidence: Vec::new(),
            limitations: Vec::new(),
            available_pct: Some(self.available_pct),
            rates_per_second: std::collections::BTreeMap::new(),
            disk_bytes_per_second: std::collections::BTreeMap::new(),
            zram_ratios: std::collections::BTreeMap::new(),
            interval_seconds: Some(1.0),
            reliable: true,
        }
    }
}

/// Rekam jejak beban kerja terstruktur untuk evaluasi deterministik offline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkloadTrace {
    pub schema_version: u32,
    pub workload_identifier: String,
    pub recorded_at_epoch_secs: u64,
    pub total_steps: usize,
    pub records: Vec<TraceRecord>,
}

impl WorkloadTrace {
    pub fn new(workload_identifier: String) -> Self {
        Self {
            schema_version: 1,
            workload_identifier,
            recorded_at_epoch_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            total_steps: 0,
            records: Vec::new(),
        }
    }

    pub fn append(&mut self, record: TraceRecord) {
        self.records.push(record);
        self.total_steps = self.records.len();
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        let content = self
            .to_json()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut f = File::create(path)?;
        f.write_all(content.as_bytes())
    }

    pub fn load_from_file(path: &Path) -> io::Result<Self> {
        let mut f = File::open(path)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        Self::from_json(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

/// Laporan evaluasi kontrafaktual dari Workload Shadow Replay Profiler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowEvaluationReport {
    pub workload_identifier: String,
    pub total_steps_replayed: usize,
    pub simulated_duration_ms: u64,
    pub peak_pressure: MemoryPressure,
    pub low_pressure_cycles: usize,
    pub moderate_pressure_cycles: usize,
    pub high_pressure_cycles: usize,
    pub critical_pressure_cycles: usize,
    pub total_pages_migrated: usize,
    pub simulated_space_saved_bytes: usize,
    pub simulated_compression_ratio: f64,
    pub advisory_tuning_recommendations_count: usize,
}

/// Pemutar jejak bayangan deterministik yang mereplikasi respons kebijakan AURA
/// tanpa mengubah workload fisik asli.
pub struct ShadowPlayer;

impl ShadowPlayer {
    /// Mereplikasi rekaman jejak telemetri secara deterministik terhadap PageStore dan AURA.
    pub fn replay(
        trace: &WorkloadTrace,
        coordinator: &AuraKernelCoordinator,
        store: &mut PageStore,
    ) -> Result<ShadowEvaluationReport, xynoos_hypermemory::Error> {
        let mut peak_pressure = MemoryPressure::Low;
        let mut low_count = 0;
        let mut mod_count = 0;
        let mut high_count = 0;
        let mut crit_count = 0;
        let mut total_migrated = 0;
        let mut advisory_recs = 0;

        for record in &trace.records {
            let diag = record.to_diagnosis();
            let raw_pressure = crate::tiering::PressureBridge::evaluate_pressure(&diag);
            let stabilized = coordinator
                .tiering_coordinator
                .stabilizer
                .step(raw_pressure);

            match stabilized {
                MemoryPressure::Low => low_count += 1,
                MemoryPressure::Moderate => {
                    mod_count += 1;
                    if peak_pressure < MemoryPressure::Moderate {
                        peak_pressure = MemoryPressure::Moderate;
                    }
                }
                MemoryPressure::High => {
                    high_count += 1;
                    if peak_pressure < MemoryPressure::High {
                        peak_pressure = MemoryPressure::High;
                    }
                }
                MemoryPressure::Critical => {
                    crit_count += 1;
                    peak_pressure = MemoryPressure::Critical;
                }
            }

            let tier_report = coordinator
                .tiering_coordinator
                .step_with_pressure(store, stabilized)?;
            total_migrated += tier_report.migrated_to_compressed + tier_report.migrated_to_memory;

            let rec = coordinator.recommend(stabilized);
            if rec != KernelTuningRecommendation::None {
                advisory_recs += 1;
            }
        }

        let stats = store.stats();
        let savings = stats.original_bytes.saturating_sub(stats.compressed_bytes);
        let ratio = if stats.original_bytes > 0 {
            stats.compressed_bytes as f64 / stats.original_bytes as f64
        } else {
            1.0
        };

        let duration = trace
            .records
            .last()
            .map(|r| r.relative_timestamp_ms)
            .unwrap_or(0);

        Ok(ShadowEvaluationReport {
            workload_identifier: trace.workload_identifier.clone(),
            total_steps_replayed: trace.total_steps,
            simulated_duration_ms: duration,
            peak_pressure,
            low_pressure_cycles: low_count,
            moderate_pressure_cycles: mod_count,
            high_pressure_cycles: high_count,
            critical_pressure_cycles: crit_count,
            total_pages_migrated: total_migrated,
            simulated_space_saved_bytes: savings,
            simulated_compression_ratio: ratio,
            advisory_tuning_recommendations_count: advisory_recs,
        })
    }
}

/// Versioned policy configuration schema with strict validation and safe rollback defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub schema_version: u32,
    pub hot_latency_threshold_ns: u64,
    pub promotion_latency_threshold_ns: u64,
    pub selective_promotion: bool,
    pub deescalation_hold_ticks: u64,
    pub migration_max_pages: usize,
    pub migration_max_bytes: usize,
    pub cooldown_ticks: u64,
    pub min_recompression_savings_bytes: usize,
    pub enable_lz4hc: bool,
    pub lz4hc_level: u8,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self::safe_default()
    }
}

impl PolicyConfig {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Conservative known-good default configuration for safe rollback.
    pub fn safe_default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            hot_latency_threshold_ns: 600,
            promotion_latency_threshold_ns: 1500,
            selective_promotion: true,
            deescalation_hold_ticks: 3,
            migration_max_pages: 16,
            migration_max_bytes: 256 * 1024,
            cooldown_ticks: 4,
            min_recompression_savings_bytes: 256,
            enable_lz4hc: true,
            lz4hc_level: 6,
        }
    }

    /// Strict schema and bounds validation before accepting or saving any calibrated policy.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(format!(
                "unsupported policy schema_version {} (expected {})",
                self.schema_version,
                Self::SCHEMA_VERSION
            ));
        }
        if self.hot_latency_threshold_ns == 0 || self.hot_latency_threshold_ns > 1_000_000 {
            return Err("hot_latency_threshold_ns out of bounds".into());
        }
        if self.promotion_latency_threshold_ns < self.hot_latency_threshold_ns
            || self.promotion_latency_threshold_ns > 10_000_000
        {
            return Err("promotion_latency_threshold_ns out of bounds".into());
        }
        if self.deescalation_hold_ticks > 32 {
            return Err("deescalation_hold_ticks out of bounds (max 32)".into());
        }
        if self.migration_max_pages == 0 || self.migration_max_pages > 4096 {
            return Err("migration_max_pages out of bounds".into());
        }
        if self.migration_max_bytes < 4096 || self.migration_max_bytes > 64 * 1024 * 1024 {
            return Err("migration_max_bytes out of bounds".into());
        }
        if self.cooldown_ticks > 64 {
            return Err("cooldown_ticks out of bounds".into());
        }
        if !(1..=12).contains(&self.lz4hc_level) {
            return Err("lz4hc_level must be within 1..=12".into());
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json_validated(json: &str) -> Result<Self, String> {
        let cfg: Self = serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }
}

/// Evaluation metrics for one policy configuration on a replay split.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyBaselineComparison {
    pub policy_name: String,
    pub steps_evaluated: usize,
    pub net_savings_bytes: isize,
    pub stored_payload_bytes: usize,
    pub original_logical_bytes: usize,
    pub total_transitions: usize,
    pub rejected_expensive_ops: usize,
    pub utility_score: i64,
}

/// Bounded offline calibration report separating calibration, validation, and held-out evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub schema_version: u32,
    pub workload_identifier: String,
    pub measurement_kind: &'static str,
    pub advisory_only: bool,
    pub calibration_steps: usize,
    pub validation_steps: usize,
    pub evaluation_steps: usize,
    pub candidates_evaluated: usize,
    pub overfitting_guard_triggered: bool,
    pub recommended_config: PolicyConfig,
    pub rollback_config: PolicyConfig,
    pub evaluation_comparisons: Vec<PolicyBaselineComparison>,
}

/// Bounded offline policy calibrator using train/validation/evaluation trace splits.
pub struct ShadowCalibrator;

impl ShadowCalibrator {
    fn candidate_grid() -> Vec<PolicyConfig> {
        let mut grid = Vec::new();
        for hold in [2u64, 3, 5] {
            for cooldown in [2u64, 4, 8] {
                for hc in [false, true] {
                    grid.push(PolicyConfig {
                        deescalation_hold_ticks: hold,
                        cooldown_ticks: cooldown,
                        enable_lz4hc: hc,
                        lz4hc_level: if hc { 6 } else { 4 },
                        ..PolicyConfig::safe_default()
                    });
                }
            }
        }
        // Strictly bounded candidate count (max 18)
        grid.truncate(12);
        grid
    }

    fn trace_to_multi_resource(rec: &TraceRecord) -> MultiResourceState {
        let diag = rec.to_diagnosis();
        let mem_pressure = crate::tiering::PressureBridge::evaluate_pressure(&diag);
        // Derive conservative CPU/IO contention proxy from page-fault / PSI flags in legacy trace
        let cpu_pressure = if rec.high_page_fault_rate && rec.severe_psi {
            ResourcePressureLevel::Moderate
        } else {
            ResourcePressureLevel::Low
        };
        let io_pressure = if rec.swap_thrashing {
            ResourcePressureLevel::High
        } else {
            ResourcePressureLevel::Low
        };
        MultiResourceState {
            memory_pressure: mem_pressure,
            cpu_pressure,
            io_pressure,
            available_ram_bytes: Some(rec.mem_available_bytes),
            telemetry_valid: true,
        }
    }

    fn eval_adaptive_split(
        name: &str,
        records: &[TraceRecord],
        pages: &[Vec<u8>],
        config: &PolicyConfig,
    ) -> Result<PolicyBaselineComparison, String> {
        let page_size = pages.first().map(|p| p.len()).unwrap_or(4096);
        let mut store = PageStore::adaptive(page_size, pages.len().max(1), usize::MAX)
            .map_err(|e| e.to_string())?;
        for p in pages {
            store
                .insert_with_codec(p, CodecId::Raw, xynoos_core::EvictionPolicy::Reject)
                .map_err(|e| e.to_string())?;
        }
        let mut coord = AuraAdaptiveCoordinator::new(
            AdaptiveCompressionConfig {
                enable_lz4hc: config.enable_lz4hc,
                lz4hc_level: config.lz4hc_level,
                min_savings_bytes: 128,
                min_recompression_savings_bytes: config.min_recompression_savings_bytes,
                min_scratch_ram_bytes: 8 * 1024 * 1024,
                selective_promotion: config.selective_promotion,
            },
            PressureStabilizer::new(config.deescalation_hold_ticks, 0),
            MigrationBudget::bounded(config.migration_max_pages, config.migration_max_bytes),
        );
        coord.cost_model.cooldown_ticks = config.cooldown_ticks;

        let mut transitions = 0usize;
        let mut rejected = 0usize;
        for (idx, rec) in records.iter().enumerate() {
            // Simulate realistic hot-page reads on first 20% of pages
            if !pages.is_empty() {
                let hot_count = (pages.len() / 5).max(1);
                let ids = store.page_ids_sorted();
                let target_id = ids[idx % hot_count];
                let _ = store.get(target_id);
            }
            let res = Self::trace_to_multi_resource(rec);
            let rep = coord
                .step_adaptive(&mut store, &res)
                .map_err(|e| e.to_string())?;
            transitions += rep.policy_transitions;
            rejected += rep.rejected_expensive_ops;
        }

        let acct = store.overhead_accounting();
        // Utility balances net RAM saved against migration churn
        let utility_score = (acct.net_savings_bytes as i64)
            .saturating_sub((transitions as i64).saturating_mul(256));

        Ok(PolicyBaselineComparison {
            policy_name: name.into(),
            steps_evaluated: records.len(),
            net_savings_bytes: acct.net_savings_bytes,
            stored_payload_bytes: acct.stored_payload_bytes,
            original_logical_bytes: acct.original_logical_bytes,
            total_transitions: transitions,
            rejected_expensive_ops: rejected,
            utility_score,
        })
    }

    fn eval_milestone_f_split(
        records: &[TraceRecord],
        pages: &[Vec<u8>],
    ) -> Result<PolicyBaselineComparison, String> {
        let page_size = pages.first().map(|p| p.len()).unwrap_or(4096);
        let mut store = PageStore::adaptive(page_size, pages.len().max(1), usize::MAX)
            .map_err(|e| e.to_string())?;
        for p in pages {
            store
                .insert_with_codec(p, CodecId::Raw, xynoos_core::EvictionPolicy::Reject)
                .map_err(|e| e.to_string())?;
        }
        let coord = AuraTieringCoordinator::new(AdaptiveTieringPolicy::default());
        let mut transitions = 0usize;
        for (idx, rec) in records.iter().enumerate() {
            if !pages.is_empty() {
                let hot_count = (pages.len() / 5).max(1);
                let ids = store.page_ids_sorted();
                let _ = store.get(ids[idx % hot_count]);
            }
            let rep = coord
                .step(&mut store, &rec.to_diagnosis())
                .map_err(|e| e.to_string())?;
            transitions += rep.migrated_to_compressed + rep.migrated_to_memory;
        }
        let acct = store.overhead_accounting();
        let utility_score = (acct.net_savings_bytes as i64)
            .saturating_sub((transitions as i64).saturating_mul(256));
        Ok(PolicyBaselineComparison {
            policy_name: "milestone_f_baseline".into(),
            steps_evaluated: records.len(),
            net_savings_bytes: acct.net_savings_bytes,
            stored_payload_bytes: acct.stored_payload_bytes,
            original_logical_bytes: acct.original_logical_bytes,
            total_transitions: transitions,
            rejected_expensive_ops: 0,
            utility_score,
        })
    }

    fn eval_static_lz4_split(
        records: &[TraceRecord],
        pages: &[Vec<u8>],
    ) -> Result<PolicyBaselineComparison, String> {
        let page_size = pages.first().map(|p| p.len()).unwrap_or(4096);
        let mut store = PageStore::adaptive(page_size, pages.len().max(1), usize::MAX)
            .map_err(|e| e.to_string())?;
        for p in pages {
            store
                .insert_with_codec(p, CodecId::Lz4Standard, xynoos_core::EvictionPolicy::Reject)
                .map_err(|e| e.to_string())?;
        }
        let acct = store.overhead_accounting();
        Ok(PolicyBaselineComparison {
            policy_name: "static_standard_lz4".into(),
            steps_evaluated: records.len(),
            net_savings_bytes: acct.net_savings_bytes,
            stored_payload_bytes: acct.stored_payload_bytes,
            original_logical_bytes: acct.original_logical_bytes,
            total_transitions: 0,
            rejected_expensive_ops: 0,
            utility_score: acct.net_savings_bytes as i64,
        })
    }

    /// Perform bounded calibration separating calibration (50%), validation (25%),
    /// and evaluation (25%) trace slices to prevent overfitting.
    pub fn calibrate(
        trace: &WorkloadTrace,
        sample_pages: &[Vec<u8>],
    ) -> Result<CalibrationReport, String> {
        if trace.records.len() < 8 {
            return Err(
                "calibration requires at least 8 trace records for train/val/eval splits".into(),
            );
        }
        if sample_pages.is_empty() {
            return Err("sample_pages must not be empty".into());
        }

        let n = trace.records.len();
        let cal_end = (n / 2).max(2);
        let val_end = (cal_end + ((n - cal_end) / 2).max(1)).min(n - 1);

        let cal_slice = &trace.records[..cal_end];
        let val_slice = &trace.records[cal_end..val_end];
        let eval_slice = &trace.records[val_end..];

        let candidates = Self::candidate_grid();
        let mut best_cfg = PolicyConfig::safe_default();
        let mut best_cal_score = i64::MIN;

        for cfg in &candidates {
            cfg.validate()?;
            let res = Self::eval_adaptive_split("candidate", cal_slice, sample_pages, cfg)?;
            if res.utility_score > best_cal_score {
                best_cal_score = res.utility_score;
                best_cfg = cfg.clone();
            }
        }

        // Validation gate against overfitting
        let default_cfg = PolicyConfig::safe_default();
        let default_val =
            Self::eval_adaptive_split("default_val", val_slice, sample_pages, &default_cfg)?;
        let best_val = Self::eval_adaptive_split("best_val", val_slice, sample_pages, &best_cfg)?;
        let overfitting_guard_triggered = best_val.utility_score < default_val.utility_score;
        let final_cfg = if overfitting_guard_triggered {
            default_cfg.clone()
        } else {
            best_cfg
        };

        // Final held-out evaluation against baselines
        let eval_static = Self::eval_static_lz4_split(eval_slice, sample_pages)?;
        let eval_mf = Self::eval_milestone_f_split(eval_slice, sample_pages)?;
        let eval_mg_def = Self::eval_adaptive_split(
            "milestone_g_default",
            eval_slice,
            sample_pages,
            &default_cfg,
        )?;
        let eval_mg_cal = Self::eval_adaptive_split(
            "milestone_g_calibrated",
            eval_slice,
            sample_pages,
            &final_cfg,
        )?;

        Ok(CalibrationReport {
            schema_version: PolicyConfig::SCHEMA_VERSION,
            workload_identifier: trace.workload_identifier.clone(),
            measurement_kind: "counterfactual_replay_estimate",
            advisory_only: true,
            calibration_steps: cal_slice.len(),
            validation_steps: val_slice.len(),
            evaluation_steps: eval_slice.len(),
            candidates_evaluated: candidates.len(),
            overfitting_guard_triggered,
            recommended_config: final_cfg,
            rollback_config: PolicyConfig::safe_default(),
            evaluation_comparisons: vec![eval_static, eval_mf, eval_mg_def, eval_mg_cal],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiering::AuraTieringCoordinator;

    #[test]
    fn test_trace_serialization_round_trip() {
        let mut trace = WorkloadTrace::new("web-server-mock".into());
        trace.append(TraceRecord {
            step_index: 0,
            relative_timestamp_ms: 1000,
            mem_total_bytes: 4 * 1024 * 1024 * 1024,
            mem_available_bytes: 2 * 1024 * 1024 * 1024,
            available_pct: 50.0,
            psi_some_avg10: 2.5,
            psi_full_avg10: 0.0,
            swap_thrashing: false,
            severe_psi: false,
            high_page_fault_rate: false,
            cgroup_usage_bytes: 128 * 1024 * 1024,
        });

        let json = trace.to_json().unwrap();
        let loaded = WorkloadTrace::from_json(&json).unwrap();
        assert_eq!(trace, loaded);
        assert_eq!(loaded.total_steps, 1);
    }

    #[test]
    fn test_shadow_replay_deterministic_evaluation() {
        let mut trace = WorkloadTrace::new("batch-job".into());
        // Langkah 1: Tekanan Rendah
        trace.append(TraceRecord {
            step_index: 0,
            relative_timestamp_ms: 0,
            mem_total_bytes: 4 * 1024 * 1024 * 1024,
            mem_available_bytes: 3 * 1024 * 1024 * 1024,
            available_pct: 75.0,
            psi_some_avg10: 0.0,
            psi_full_avg10: 0.0,
            swap_thrashing: false,
            severe_psi: false,
            high_page_fault_rate: false,
            cgroup_usage_bytes: 64 * 1024 * 1024,
        });
        // Langkah 2: Tekanan Tinggi
        trace.append(TraceRecord {
            step_index: 1,
            relative_timestamp_ms: 5000,
            mem_total_bytes: 4 * 1024 * 1024 * 1024,
            mem_available_bytes: 400 * 1024 * 1024,
            available_pct: 10.0,
            psi_some_avg10: 25.0,
            psi_full_avg10: 15.0,
            swap_thrashing: true,
            severe_psi: true,
            high_page_fault_rate: true,
            cgroup_usage_bytes: 256 * 1024 * 1024,
        });

        let tiering_coord =
            AuraTieringCoordinator::new(crate::tiering::AdaptiveTieringPolicy::default());
        let kernel_coord = AuraKernelCoordinator::new(tiering_coord);
        let mut store = PageStore::with_capacity(32).unwrap();

        // Masukkan beberapa halaman untuk melihat efek tiering
        let sample_page = [42u8; 4096];
        store.insert(&sample_page).unwrap();
        store.insert(&sample_page).unwrap();

        let report = ShadowPlayer::replay(&trace, &kernel_coord, &mut store).unwrap();
        assert_eq!(report.total_steps_replayed, 2);
        assert_eq!(report.simulated_duration_ms, 5000);
        assert_eq!(report.peak_pressure, MemoryPressure::Critical);
        assert!(report.advisory_tuning_recommendations_count >= 1);
    }

    #[test]
    fn test_policy_config_validation_and_rollback() {
        let mut cfg = PolicyConfig::safe_default();
        assert!(cfg.validate().is_ok());
        let json = cfg.to_json().unwrap();
        let parsed = PolicyConfig::from_json_validated(&json).unwrap();
        assert_eq!(parsed, cfg);

        cfg.lz4hc_level = 99;
        assert!(cfg.validate().is_err());
        cfg.schema_version = 999;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_shadow_calibrator_train_val_eval_splits() {
        let mut trace = WorkloadTrace::new("calibration-test".into());
        for i in 0..16u64 {
            let high = i % 4 >= 2;
            trace.append(TraceRecord {
                step_index: i,
                relative_timestamp_ms: i * 1000,
                mem_total_bytes: 4 * 1024 * 1024 * 1024,
                mem_available_bytes: if high {
                    300 * 1024 * 1024
                } else {
                    2_500_000_000
                },
                available_pct: if high { 8.0 } else { 65.0 },
                psi_some_avg10: if high { 18.0 } else { 0.5 },
                psi_full_avg10: if high { 5.0 } else { 0.0 },
                swap_thrashing: false,
                severe_psi: high,
                high_page_fault_rate: high,
                cgroup_usage_bytes: 256 * 1024 * 1024,
            });
        }
        let pages: Vec<Vec<u8>> = (0..10)
            .map(|p| {
                let mut v = Vec::with_capacity(4096);
                for row in 0..128 {
                    let s = format!("{{\"p\":{},\"row\":{},\"k\":\"calib\"}}\n", p, row);
                    v.extend_from_slice(s.as_bytes());
                }
                v.resize(4096, b'0');
                v
            })
            .collect();

        let cal = ShadowCalibrator::calibrate(&trace, &pages).unwrap();
        assert_eq!(cal.measurement_kind, "counterfactual_replay_estimate");
        assert!(cal.advisory_only);
        assert_eq!(
            cal.calibration_steps + cal.validation_steps + cal.evaluation_steps,
            16
        );
        assert_eq!(cal.evaluation_comparisons.len(), 4);
        assert!(cal.recommended_config.validate().is_ok());
    }
}
