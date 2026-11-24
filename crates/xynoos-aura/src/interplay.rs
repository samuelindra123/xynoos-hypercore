#![forbid(unsafe_code)]
use crate::tiering::{AuraTieringCoordinator, MemoryPressure};
use serde::Serialize;
use xynoos_hypermemory::{PageStore, TieringReport};
use xynoos_linux_control::{
    transaction::{self, Backend, TransactionReport},
    Action, MAX_HIGH_BYTES, MAX_LOW_BYTES,
};
use xynoos_pressureiq::{intelligence::Diagnosis, system::SystemSnapshot};

/// Rekomendasi aksi kernel cgroup v2 berdasarkan status tekanan memori yang distabilkan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelTuningRecommendation {
    None,
    ProtectEssential { bytes: u64 },
    ThrottleBackground { bytes: u64 },
    EmergencyLimit { bytes: u64 },
}

impl KernelTuningRecommendation {
    /// Mengonversi rekomendasi menjadi `Action` kernel terikat yang dapat dieksekusi transaksional.
    pub fn to_action(&self) -> Option<Action> {
        match *self {
            KernelTuningRecommendation::None => None,
            KernelTuningRecommendation::ProtectEssential { bytes } => {
                let bounded = bytes.min(MAX_LOW_BYTES);
                let aligned = bounded - (bounded % 4096);
                if aligned == 0 {
                    None
                } else {
                    Some(Action::MemoryLow { bytes: aligned })
                }
            }
            KernelTuningRecommendation::ThrottleBackground { bytes } => {
                let bounded = bytes.min(MAX_HIGH_BYTES);
                let aligned = bounded - (bounded % 4096);
                if aligned == 0 {
                    None
                } else {
                    Some(Action::MemoryHigh { bytes: aligned })
                }
            }
            KernelTuningRecommendation::EmergencyLimit { bytes } => {
                let bounded = bytes.min(MAX_HIGH_BYTES);
                let aligned = bounded - (bounded % 4096);
                if aligned == 0 {
                    None
                } else {
                    Some(Action::MemoryHigh { bytes: aligned })
                }
            }
        }
    }
}

/// Laporan hasil orkestrasi terpadu memori userspace dan tuning kernel.
#[derive(Debug, Serialize)]
pub struct InterplayReport {
    pub raw_pressure: MemoryPressure,
    pub stabilized_pressure: MemoryPressure,
    pub recommendation: KernelTuningRecommendation,
    pub tiering_report: TieringReport,
    pub kernel_transaction: Option<TransactionReport>,
}

/// Koordinator interaksi terpadu AURA yang menyelaraskan migrasi tingkatan memori userspace
/// (`HyperMemory`) dengan pengendalian cgroup v2 Linux kernel (`xynoos-linux-control`).
pub struct AuraKernelCoordinator {
    pub tiering_coordinator: AuraTieringCoordinator,
    pub default_protection_bytes: u64,
    pub default_throttle_bytes: u64,
}

impl AuraKernelCoordinator {
    pub fn new(tiering_coordinator: AuraTieringCoordinator) -> Self {
        Self {
            tiering_coordinator,
            default_protection_bytes: 16 * 1024 * 1024, // 16 MiB memory.low protection
            default_throttle_bytes: 64 * 1024 * 1024,   // 64 MiB memory.high throttle
        }
    }

    /// Menghasilkan rekomendasi tuning kernel berdasarkan status tekanan memori yang distabilkan.
    pub fn recommend(&self, pressure: MemoryPressure) -> KernelTuningRecommendation {
        match pressure {
            MemoryPressure::Low => KernelTuningRecommendation::None,
            MemoryPressure::Moderate => KernelTuningRecommendation::ProtectEssential {
                bytes: self.default_protection_bytes,
            },
            MemoryPressure::High => KernelTuningRecommendation::ThrottleBackground {
                bytes: self.default_throttle_bytes,
            },
            MemoryPressure::Critical => KernelTuningRecommendation::EmergencyLimit {
                bytes: (self.default_throttle_bytes / 2).max(4096),
            },
        }
    }

    /// Menjalankan satu siklus orkestrasi penuh:
    /// 1. Mengidentifikasi tekanan dan melakukan mitigasi tiering memori userspace via `AuraTieringCoordinator`.
    /// 2. Menghasilkan rekomendasi tuning kernel cgroup v2 (`KernelTuningRecommendation`).
    /// 3. Jika backend disposable cgroup dan snapshot kernel tersedia serta terotorisasi,
    ///    mengeksekusi transaksi kernel yang terikat fail-safe dengan pemulihan otomatis.
    pub fn step<B: Backend + ?Sized>(
        &self,
        store: &mut PageStore,
        diagnosis: &Diagnosis,
        system_snapshot: Option<&SystemSnapshot>,
        backend: Option<&mut B>,
        authorized: bool,
    ) -> Result<InterplayReport, xynoos_hypermemory::Error> {
        let raw_pressure = self.tiering_coordinator.evaluate_pressure(diagnosis);
        let stabilized_pressure = self.tiering_coordinator.stabilized_pressure(raw_pressure);

        // Eksekusi tiering userspace HyperMemory
        let tiering_report = self
            .tiering_coordinator
            .step_with_pressure(store, stabilized_pressure)?;

        // Rekomendasi kernel
        let recommendation = self.recommend(stabilized_pressure);

        // Eksekusi transaksi kernel jika backend disediakan
        let kernel_transaction = match (recommendation.to_action(), system_snapshot, backend) {
            (Some(action), Some(snapshot), Some(b)) => {
                Some(transaction::run(b, snapshot, action, authorized))
            }
            _ => None,
        };

        Ok(InterplayReport {
            raw_pressure,
            stabilized_pressure,
            recommendation,
            tiering_report,
            kernel_transaction,
        })
    }
}
