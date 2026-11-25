//! Non-invasive, workload-aware background controller and resource classifier.
#![forbid(unsafe_code)]

use crate::telemetry_scheduler::TelemetrySample;
use serde::{Deserialize, Serialize};

/// Behavioral classification of active system workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadClass {
    /// Responsive UI / desktop interaction with intermittent bursts.
    InteractiveForeground,
    /// Multi-process web browser with dynamic memory allocation.
    WebBrowser,
    /// Development IDE / text editor with periodic compilation/indexing.
    DevelopmentEditor,
    /// Sustained multi-core compilation / software build process.
    BackgroundCompilation,
    /// Sustained numerical computation / CPU-bound task.
    CpuIntensive,
    /// Heavy memory allocation, thrashing, or high reclaim rates.
    MemoryIntensive,
    /// Heavy disk reads/writes or high I/O wait.
    IoIntensive,
    /// Video decoding / playback requiring strictly bounded frame timing.
    VideoPlayback,
    /// Quiescent system with minimal resource demand.
    Idle,
    /// Co-occurring heterogeneous workloads.
    Mixed,
    /// Unknown or insufficient telemetry evidence to classify.
    #[default]
    Unknown,
}

/// Proposed workload-level cgroup tuning recommendation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadRecommendation {
    pub classification: WorkloadClass,
    pub recommended_cpu_weight: Option<u32>,
    pub recommended_memory_low_headroom_bytes: Option<u64>,
    pub recommended_memory_high_headroom_bytes: Option<u64>,
    pub protect_background_progress: bool,
    pub confidence: f64,
    pub explanation: String,
}

/// Workload controller evaluating resource trends and proposing safe, bounded adjustments.
pub struct WorkloadController;

impl WorkloadController {
    /// Classify current system workload from telemetry metrics.
    ///
    /// Strictly refuses invasive process inspection, command lines, environment, or private files.
    pub fn classify(sample: &TelemetrySample) -> (WorkloadClass, f64) {
        let cpu_util = sample.cpu_utilization_pct.unwrap_or(0.0);
        let mem_psi_some = sample.psi_memory_some_avg10.unwrap_or(0.0);
        let cpu_psi_some = sample.psi_cpu_some_avg10.unwrap_or(0.0);
        let io_psi_some = sample.psi_io_some_avg10.unwrap_or(0.0);
        let swap_active = sample.swap_in_delta + sample.swap_out_delta;

        // 1. Quiescent Idle
        if cpu_util < 4.0 && mem_psi_some < 0.5 && cpu_psi_some < 0.5 && io_psi_some < 0.5 {
            return (WorkloadClass::Idle, 0.95);
        }

        // 2. Video Playback
        if sample.gpu_active
            && (10.0..=45.0).contains(&cpu_util)
            && io_psi_some < 3.0
            && mem_psi_some < 1.0
        {
            return (WorkloadClass::VideoPlayback, 0.85);
        }

        // 3. Background Software Compilation
        // Characteristic: high CPU utilization across all cores + moderate I/O pressure + page faults
        if cpu_util >= 75.0 && (io_psi_some >= 2.0 || sample.pgmajfault_delta >= 50) {
            return (WorkloadClass::BackgroundCompilation, 0.90);
        }

        // 4. Pure CPU-intensive computation
        if cpu_util >= 85.0 && io_psi_some < 2.0 && mem_psi_some < 1.0 {
            return (WorkloadClass::CpuIntensive, 0.88);
        }

        // 5. Memory-intensive workload
        if mem_psi_some >= 8.0 || swap_active >= 20 || sample.cgroup_memory_events_high_delta > 0 {
            return (WorkloadClass::MemoryIntensive, 0.92);
        }

        // 6. I/O-intensive workload
        if io_psi_some >= 15.0 && cpu_util < 60.0 {
            return (WorkloadClass::IoIntensive, 0.90);
        }

        // 7. Interactive foreground / browser / editor
        if (5.0..=55.0).contains(&cpu_util) && cpu_psi_some < 5.0 && io_psi_some < 5.0 {
            if sample.mem_available_pct.is_some_and(|p| p < 30.0) {
                return (WorkloadClass::WebBrowser, 0.78);
            }
            return (WorkloadClass::InteractiveForeground, 0.80);
        }

        // 8. Mixed
        if cpu_util > 50.0 && (io_psi_some > 5.0 || mem_psi_some > 5.0) {
            return (WorkloadClass::Mixed, 0.75);
        }

        (WorkloadClass::Unknown, 0.50)
    }

    /// Compute bounded workload resource recommendations.
    pub fn evaluate(
        sample: &TelemetrySample,
        current_workload_cgroup_weight: Option<u32>,
        current_workload_cgroup_usage: Option<u64>,
    ) -> WorkloadRecommendation {
        let (class, confidence) = Self::classify(sample);

        match class {
            WorkloadClass::BackgroundCompilation => {
                // Ensure compilation progress is protected: do not starve CPU or clamp memory below working set
                let weight = current_workload_cgroup_weight.unwrap_or(100).max(60);
                let mem_high_headroom = current_workload_cgroup_usage.map(|u| {
                    // Provide 25% headroom over current working set
                    (u as u128 * 125 / 100).min(xynoos_linux_control::MAX_HIGH_BYTES as u128) as u64
                });

                WorkloadRecommendation {
                    classification: class,
                    recommended_cpu_weight: Some(weight),
                    recommended_memory_low_headroom_bytes: None,
                    recommended_memory_high_headroom_bytes: mem_high_headroom,
                    protect_background_progress: true,
                    confidence,
                    explanation: "Sustained compilation detected; protecting CPU weight and memory headroom against starvation".into(),
                }
            }

            WorkloadClass::InteractiveForeground | WorkloadClass::VideoPlayback => {
                // Prioritize UI responsiveness
                WorkloadRecommendation {
                    classification: class,
                    recommended_cpu_weight: Some(150),
                    recommended_memory_low_headroom_bytes: current_workload_cgroup_usage.map(|u| {
                        (u as u128 * 110 / 100).min(xynoos_linux_control::MAX_LOW_BYTES as u128) as u64
                    }),
                    recommended_memory_high_headroom_bytes: None,
                    protect_background_progress: true,
                    confidence,
                    explanation: "Interactive/video workload detected; reserving bounded memory low protection and elevated CPU weight".into(),
                }
            }

            WorkloadClass::MemoryIntensive => {
                WorkloadRecommendation {
                    classification: class,
                    recommended_cpu_weight: Some(80),
                    recommended_memory_low_headroom_bytes: None,
                    recommended_memory_high_headroom_bytes: None,
                    protect_background_progress: true,
                    confidence,
                    explanation: "Memory contention detected; deferring aggressive CPU changes to avoid compounding reclaim pressure".into(),
                }
            }

            WorkloadClass::Idle => {
                WorkloadRecommendation {
                    classification: class,
                    recommended_cpu_weight: Some(100),
                    recommended_memory_low_headroom_bytes: None,
                    recommended_memory_high_headroom_bytes: None,
                    protect_background_progress: true,
                    confidence,
                    explanation: "System idle; preserving balanced default resource weights".into(),
                }
            }

            _ => {
                WorkloadRecommendation {
                    classification: class,
                    recommended_cpu_weight: None,
                    recommended_memory_low_headroom_bytes: None,
                    recommended_memory_high_headroom_bytes: None,
                    protect_background_progress: true,
                    confidence,
                    explanation: "Ambiguous or balanced workload; remaining in non-intervening observation mode".into(),
                }
            }
        }
    }
}
