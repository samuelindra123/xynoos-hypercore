//! Modul jembatan tiering adaptif AURA (Milestone C).
//! Menghubungkan PressureIQ memory pressure levels (Low, Moderate, High, Critical)
//! dengan keputusan penempatan tiering AURA untuk memigrasikan halaman secara otomatis
//! antara raw (Memory) dan LZ4 compressed storage di xynoos-hypermemory.

use xynoos_core::{CostEstimate, MemoryPressure, StorageTier};
use xynoos_hypermemory::{Error as HyperMemoryError, PageStore, TieringReport};
use xynoos_pressureiq::intelligence::Diagnosis;

use crate::DecisionPolicy;

/// Jembatan deterministik untuk menginterpretasikan telemetri PressureIQ menjadi tingkat tekanan memori.
pub struct PressureBridge;

impl PressureBridge {
    /// Mengevaluasi tingkat tekanan memori dari Diagnosis PressureIQ.
    /// Memiliki kontrak fail-safe: jika telemetri tidak andal (unreliable/stale),
    /// sistem kembali ke tingkat `Moderate` yang aman untuk mencegah keputusan ekstrem.
    pub fn evaluate_pressure(diagnosis: &Diagnosis) -> MemoryPressure {
        if !diagnosis.reliable {
            // Safe fallback: telemetri kedaluwarsa atau tidak lengkap
            return MemoryPressure::Moderate;
        }

        let has = |class: &str| diagnosis.classes.iter().any(|c| c == class);
        let avail = diagnosis.available_pct.unwrap_or(100.0);

        if has("swap_thrashing") || avail < 5.0 {
            MemoryPressure::Critical
        } else if has("active_direct_reclaim") || has("sustained_pressure") || avail < 15.0 {
            MemoryPressure::High
        } else if has("elevated_pressure") || has("swap_activity") || avail < 25.0 {
            MemoryPressure::Moderate
        } else {
            MemoryPressure::Low
        }
    }
}

/// Kebijakan penempatan adaptif AURA untuk penentuan tingkatan penyimpanan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptiveTieringPolicy {
    /// Batas latensi (ns) di mana halaman dianggap hot / latency-critical
    pub hot_latency_threshold_ns: u64,
}

impl Default for AdaptiveTieringPolicy {
    fn default() -> Self {
        Self {
            hot_latency_threshold_ns: 500,
        }
    }
}

impl AdaptiveTieringPolicy {
    pub fn new(hot_latency_threshold_ns: u64) -> Self {
        Self {
            hot_latency_threshold_ns,
        }
    }
}

impl DecisionPolicy for AdaptiveTieringPolicy {
    fn decide(&self, pressure: MemoryPressure, cost: CostEstimate) -> StorageTier {
        match pressure {
            MemoryPressure::Critical | MemoryPressure::High => {
                // Pada tekanan tinggi dan kritis, seluruh halaman dimigrasikan ke Compressed
                // untuk membebaskan ruang memori fisik secepat dan sebesar mungkin.
                StorageTier::Compressed
            }
            MemoryPressure::Moderate => {
                // Pada tekanan moderat, halaman hot/latensi rendah tetap di Memory (raw);
                // halaman cold/latensi tinggi dimigrasikan ke Compressed.
                if cost.latency_ns <= self.hot_latency_threshold_ns {
                    StorageTier::Memory
                } else {
                    StorageTier::Compressed
                }
            }
            MemoryPressure::Low => {
                // Pada tekanan rendah, prioritaskan kecepatan akses tanpa dekompresi
                // dengan membiarkan halaman berada di Memory (raw) tier.
                StorageTier::Memory
            }
        }
    }
}

/// Koordinator tingkat tinggi yang mengorkestrasi evaluasi telemetri PressureIQ
/// dan eksekusi migrasi tiering di HyperMemory.
#[derive(Default)]
pub struct AuraTieringCoordinator {
    pub policy: AdaptiveTieringPolicy,
}

impl AuraTieringCoordinator {
    pub fn new(policy: AdaptiveTieringPolicy) -> Self {
        Self { policy }
    }

    /// Melakukan satu langkah evaluasi telemetri dan migrasi tiering pada PageStore.
    pub fn step(
        &self,
        store: &mut PageStore,
        diagnosis: &Diagnosis,
    ) -> Result<TieringReport, HyperMemoryError> {
        let pressure = PressureBridge::evaluate_pressure(diagnosis);
        self.step_with_pressure(store, pressure)
    }

    /// Menerapkan migrasi tiering secara langsung berdasarkan tingkat tekanan memori eksplisit.
    pub fn step_with_pressure(
        &self,
        store: &mut PageStore,
        pressure: MemoryPressure,
    ) -> Result<TieringReport, HyperMemoryError> {
        store.apply_tiering(|_id, _current_tier, cost| self.policy.decide(pressure, cost))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn mock_diagnosis(classes: Vec<&str>, avail: Option<f64>, reliable: bool) -> Diagnosis {
        Diagnosis {
            classes: classes.into_iter().map(String::from).collect(),
            evidence: vec![],
            limitations: vec![],
            available_pct: avail,
            rates_per_second: BTreeMap::new(),
            disk_bytes_per_second: BTreeMap::new(),
            zram_ratios: BTreeMap::new(),
            interval_seconds: Some(1.0),
            reliable,
        }
    }

    #[test]
    fn test_pressure_bridge_evaluations() {
        // 1. Unreliable / stale telemetri -> fallback ke Moderate
        let unreli = mock_diagnosis(vec!["normal"], Some(90.0), false);
        assert_eq!(
            PressureBridge::evaluate_pressure(&unreli),
            MemoryPressure::Moderate
        );

        // 2. Normal dan RAM melimpah -> Low
        let normal = mock_diagnosis(vec!["normal"], Some(75.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&normal),
            MemoryPressure::Low
        );

        // 3. Elevated pressure atau swap activity atau RAM < 25% -> Moderate
        let elevated = mock_diagnosis(vec!["elevated_pressure"], Some(30.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&elevated),
            MemoryPressure::Moderate
        );
        let low_ram = mock_diagnosis(vec![], Some(20.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&low_ram),
            MemoryPressure::Moderate
        );

        // 4. Sustained pressure, direct reclaim, atau RAM < 15% -> High
        let sustained = mock_diagnosis(vec!["sustained_pressure"], Some(18.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&sustained),
            MemoryPressure::High
        );
        let direct = mock_diagnosis(vec!["active_direct_reclaim"], Some(20.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&direct),
            MemoryPressure::High
        );
        let vlow_ram = mock_diagnosis(vec![], Some(10.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&vlow_ram),
            MemoryPressure::High
        );

        // 5. Swap thrashing atau RAM < 5% -> Critical
        let thrashing = mock_diagnosis(vec!["swap_thrashing"], Some(12.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&thrashing),
            MemoryPressure::Critical
        );
        let crit_ram = mock_diagnosis(vec![], Some(3.0), true);
        assert_eq!(
            PressureBridge::evaluate_pressure(&crit_ram),
            MemoryPressure::Critical
        );
    }

    #[test]
    fn test_adaptive_policy_decisions() {
        let policy = AdaptiveTieringPolicy::new(500);

        let hot_cost = CostEstimate {
            latency_ns: 200,
            bytes: 4096,
        };
        let cold_cost = CostEstimate {
            latency_ns: 1200,
            bytes: 4096,
        };

        // Low pressure -> both hot and cold stay in Memory
        assert_eq!(
            policy.decide(MemoryPressure::Low, hot_cost),
            StorageTier::Memory
        );
        assert_eq!(
            policy.decide(MemoryPressure::Low, cold_cost),
            StorageTier::Memory
        );

        // Moderate pressure -> hot stays in Memory, cold moves to Compressed
        assert_eq!(
            policy.decide(MemoryPressure::Moderate, hot_cost),
            StorageTier::Memory
        );
        assert_eq!(
            policy.decide(MemoryPressure::Moderate, cold_cost),
            StorageTier::Compressed
        );

        // High pressure -> both move to Compressed (unless very small)
        assert_eq!(
            policy.decide(MemoryPressure::High, hot_cost),
            StorageTier::Compressed
        );
        assert_eq!(
            policy.decide(MemoryPressure::High, cold_cost),
            StorageTier::Compressed
        );

        // Critical pressure -> strictly Compressed
        assert_eq!(
            policy.decide(MemoryPressure::Critical, hot_cost),
            StorageTier::Compressed
        );
    }

    #[test]
    fn test_tiering_coordinator_end_to_end() {
        let mut store = PageStore::adaptive(4096, 16, 262144).unwrap();
        let p1 = store.insert(&[1u8; 4096]).unwrap();
        let p2 = store.insert(&[2u8; 4096]).unwrap();
        let p3 = store.insert(&[3u8; 4096]).unwrap();

        let coordinator = AuraTieringCoordinator::default();

        // Step under Low pressure: all pages migrate to Memory tier (raw)
        let diag_low = mock_diagnosis(vec!["normal"], Some(80.0), true);
        let rep_low = coordinator.step(&mut store, &diag_low).unwrap();
        assert_eq!(rep_low.inspected_pages, 3);
        assert_eq!(store.page_tier(p1).unwrap(), StorageTier::Memory);
        assert_eq!(store.page_tier(p2).unwrap(), StorageTier::Memory);
        assert_eq!(store.page_tier(p3).unwrap(), StorageTier::Memory);

        // Verify data integrity in Memory tier
        assert_eq!(store.get(p1).unwrap(), vec![1u8; 4096]);
        assert_eq!(store.get(p2).unwrap(), vec![2u8; 4096]);
        assert_eq!(store.get(p3).unwrap(), vec![3u8; 4096]);

        let memory_accounted = store.stats().accounted_bytes;

        // Step under High pressure: all pages migrate to Compressed tier
        let diag_high = mock_diagnosis(vec!["sustained_pressure"], Some(10.0), true);
        let rep_high = coordinator.step(&mut store, &diag_high).unwrap();
        assert_eq!(rep_high.inspected_pages, 3);
        assert_eq!(rep_high.migrated_to_compressed, 3);
        assert_eq!(store.page_tier(p1).unwrap(), StorageTier::Compressed);
        assert_eq!(store.page_tier(p2).unwrap(), StorageTier::Compressed);
        assert_eq!(store.page_tier(p3).unwrap(), StorageTier::Compressed);

        // Verify accounted bytes decreased after compression
        let compressed_accounted = store.stats().accounted_bytes;
        assert!(compressed_accounted < memory_accounted);

        // Verify data integrity in Compressed tier
        let mut out = vec![0u8; 4096];
        store.get_into(p1, &mut out).unwrap();
        assert_eq!(out, vec![1u8; 4096]);

        // Step with unreliable telemetry: fallback to Moderate without panic or state corruption
        let diag_stale = mock_diagnosis(vec!["sustained_pressure"], Some(10.0), false);
        let rep_stale = coordinator.step(&mut store, &diag_stale).unwrap();
        assert_eq!(rep_stale.inspected_pages, 3);
        // Pages are in Compressed (latency estimate for Compressed is 1200 > 500), so they stay Compressed
        assert_eq!(rep_stale.no_change, 3);
    }
}
