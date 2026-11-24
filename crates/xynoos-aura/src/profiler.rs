#![forbid(unsafe_code)]
use crate::interplay::{AuraKernelCoordinator, KernelTuningRecommendation};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};
use xynoos_core::MemoryPressure;
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
}
