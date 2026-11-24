//! Modul jembatan tiering adaptif AURA (Milestones C & D).
//! Menghubungkan telemetri memori PressureIQ (Low, Moderate, High, Critical)
//! dengan kebijakan penempatan adaptif AURA untuk memigrasikan halaman secara otomatis
//! antara raw (Memory) dan LZ4 compressed storage di xynoos-hypermemory.
//!
//! Milestone D mengamankan subsistem ini dengan:
//! 1. Validasi telemetri ketat (penolakan NaN, Inf, persentase negatif/overflow, sinyal kontradiktif).
//! 2. Mesin status histeresis (`PressureStabilizer`) untuk mencegah osilasi cepat (thrashing).
//! 3. Promosi selektif (`selective_promotion`) saat tekanan rendah untuk menghindari dekompresi mubazir.
//! 4. Pembatasan laju migrasi per siklus (`MigrationBudget`).

use std::sync::Mutex;
use xynoos_core::{CostEstimate, MemoryPressure, MigrationBudget, StorageTier};
use xynoos_hypermemory::{Error as HyperMemoryError, PageStore, TieringReport};
use xynoos_pressureiq::intelligence::Diagnosis;

use crate::DecisionPolicy;

/// Status validasi telemetri memori sebelum dikonsumsi oleh jembatan kebijakan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryValidation {
    /// Telemetri valid dan memenuhi seluruh batas fisik yang masuk akal.
    Valid,
    /// Flag keandalan dari pengumpul bertanda tidak andal atau kedaluwarsa.
    Unreliable,
    /// Metrik wajib (seperti `available_pct` atau `interval_seconds`) hilang/None.
    MissingMetrics,
    /// Nilai metrik berada di luar domain numerik yang valid (NaN, Inf, < 0.0, > 100.0, dt <= 0).
    InvalidRange,
    /// Terdapat sinyal diagnosis yang saling bertentangan secara fisik.
    ContradictorySignals,
}

/// Jembatan deterministik untuk menginterpretasikan telemetri PressureIQ menjadi tingkat tekanan memori.
pub struct PressureBridge;

impl PressureBridge {
    /// Memvalidasi integritas telemetri dari `Diagnosis` secara ketat.
    pub fn validate_telemetry(diagnosis: &Diagnosis) -> TelemetryValidation {
        if !diagnosis.reliable {
            return TelemetryValidation::Unreliable;
        }

        // Validasi interval pengambilan sampel
        let Some(interval) = diagnosis.interval_seconds else {
            return TelemetryValidation::MissingMetrics;
        };
        if !interval.is_finite() || interval <= 0.0 {
            return TelemetryValidation::InvalidRange;
        }

        // Validasi ketersediaan memori
        let Some(avail) = diagnosis.available_pct else {
            return TelemetryValidation::MissingMetrics;
        };
        if !avail.is_finite() || !(0.0..=100.0).contains(&avail) {
            return TelemetryValidation::InvalidRange;
        }

        // Validasi konsistensi sinyal diagnosis (deteksi kontradiksi fisik)
        let has = |class: &str| diagnosis.classes.iter().any(|c| c == class);
        let has_normal = has("normal");
        let has_thrashing = has("swap_thrashing");
        let has_direct_reclaim = has("active_direct_reclaim");
        let has_sustained = has("sustained_pressure");

        // Kontradiksi 1: Status normal bersamaan dengan indikator tekanan parah
        if has_normal && (has_thrashing || has_direct_reclaim || has_sustained) {
            return TelemetryValidation::ContradictorySignals;
        }

        // Kontradiksi 2: Swap thrashing saat ketersediaan RAM dilaporkan > 50%
        if has_thrashing && avail > 50.0 {
            return TelemetryValidation::ContradictorySignals;
        }

        // Kontradiksi 3: Status normal saat ketersediaan RAM dilaporkan < 5%
        if has_normal && avail < 5.0 {
            return TelemetryValidation::ContradictorySignals;
        }

        TelemetryValidation::Valid
    }

    /// Mengevaluasi tingkat tekanan memori beserta status validitas telemetrinya.
    /// Jika telemetri tidak valid, sistem kembali secara deterministik ke `MemoryPressure::Moderate`.
    pub fn evaluate_pressure_with_status(
        diagnosis: &Diagnosis,
    ) -> (MemoryPressure, TelemetryValidation) {
        let validation = Self::validate_telemetry(diagnosis);
        if validation != TelemetryValidation::Valid {
            return (MemoryPressure::Moderate, validation);
        }

        let has = |class: &str| diagnosis.classes.iter().any(|c| c == class);
        let avail = diagnosis.available_pct.unwrap_or(100.0);

        let pressure = if has("swap_thrashing") || avail < 5.0 {
            MemoryPressure::Critical
        } else if has("active_direct_reclaim") || has("sustained_pressure") || avail < 15.0 {
            MemoryPressure::High
        } else if has("elevated_pressure") || has("swap_activity") || avail < 25.0 {
            MemoryPressure::Moderate
        } else {
            MemoryPressure::Low
        };

        (pressure, validation)
    }

    /// Mengevaluasi tingkat tekanan memori dari Diagnosis PressureIQ.
    /// Memiliki kontrak fail-safe: jika telemetri tidak valid/andal,
    /// sistem kembali ke tingkat `Moderate` yang aman untuk mencegah keputusan ekstrem.
    pub fn evaluate_pressure(diagnosis: &Diagnosis) -> MemoryPressure {
        Self::evaluate_pressure_with_status(diagnosis).0
    }
}

/// Status internal untuk mesin status histeresis tekanan memori.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StabilizerState {
    current: Option<MemoryPressure>,
    candidate: Option<MemoryPressure>,
    candidate_ticks: u64,
}

/// Mesin status histeresis untuk menstabilkan fluktuasi tekanan memori.
///
/// Mencegah osilasi cepat (misal Low <-> Moderate) dengan menahan transisi de-eskalasi
/// selama `deescalation_hold_ticks` berturut-turut.
/// Eskalasi darurat ke `Critical` selalu dieksekusi seketika (0 tick) untuk mencegah OOM.
#[derive(Debug)]
pub struct PressureStabilizer {
    state: Mutex<StabilizerState>,
    deescalation_hold_ticks: u64,
    escalation_hold_ticks: u64,
}

impl Clone for PressureStabilizer {
    fn clone(&self) -> Self {
        let state = *self.state.lock().expect("stabilizer mutex poisoned");
        Self {
            state: Mutex::new(state),
            deescalation_hold_ticks: self.deescalation_hold_ticks,
            escalation_hold_ticks: self.escalation_hold_ticks,
        }
    }
}

impl Default for PressureStabilizer {
    fn default() -> Self {
        Self::direct()
    }
}

impl PressureStabilizer {
    /// Membuat stabilizer dengan jumlah hold ticks kustom.
    pub fn new(deescalation_hold_ticks: u64, escalation_hold_ticks: u64) -> Self {
        Self {
            state: Mutex::new(StabilizerState {
                current: None,
                candidate: None,
                candidate_ticks: 0,
            }),
            deescalation_hold_ticks,
            escalation_hold_ticks,
        }
    }

    /// Mode direct tanpa jeda konfirmasi (0 hold ticks), cocok untuk kompatibilitas ke belakang.
    pub fn direct() -> Self {
        Self::new(0, 0)
    }

    /// Mode reliable standar (3 ticks konfirmasi untuk de-eskalasi, 0 tick untuk eskalasi).
    pub fn default_reliable() -> Self {
        Self::new(3, 0)
    }

    /// Melakukan satu langkah stabilisasi pada input tekanan mentah `raw`.
    /// Mengembalikan tingkat tekanan memori yang telah distabilkan.
    pub fn step(&self, raw: MemoryPressure) -> MemoryPressure {
        let mut state = self.state.lock().expect("stabilizer mutex poisoned");
        let current = match state.current {
            None => {
                // Inisialisasi awal langsung diterima
                state.current = Some(raw);
                state.candidate = None;
                state.candidate_ticks = 0;
                return raw;
            }
            Some(c) => c,
        };

        // Jika tekanan stabil pada nilai yang sama, batalkan kandidat pending
        if raw == current {
            state.candidate = None;
            state.candidate_ticks = 0;
            return current;
        }

        // Eskalasi darurat ke Critical selalu seketika (0 tick)
        if raw == MemoryPressure::Critical {
            state.current = Some(MemoryPressure::Critical);
            state.candidate = None;
            state.candidate_ticks = 0;
            return MemoryPressure::Critical;
        }

        let is_escalation = raw > current;
        let required_ticks = if is_escalation {
            self.escalation_hold_ticks
        } else {
            self.deescalation_hold_ticks
        };

        if required_ticks == 0 {
            state.current = Some(raw);
            state.candidate = None;
            state.candidate_ticks = 0;
            return raw;
        }

        if state.candidate == Some(raw) {
            state.candidate_ticks = state.candidate_ticks.saturating_add(1);
        } else {
            state.candidate = Some(raw);
            state.candidate_ticks = 1;
        }

        if state.candidate_ticks >= required_ticks {
            state.current = Some(raw);
            state.candidate = None;
            state.candidate_ticks = 0;
            raw
        } else {
            current
        }
    }

    /// Mengembalikan tingkat tekanan memori yang aktif saat ini.
    pub fn current(&self) -> Option<MemoryPressure> {
        self.state
            .lock()
            .expect("stabilizer mutex poisoned")
            .current
    }

    /// Mengembalikan kandidat transisi dan jumlah tick konfirmasi yang telah tercapai.
    pub fn candidate(&self) -> (Option<MemoryPressure>, u64) {
        let state = self.state.lock().expect("stabilizer mutex poisoned");
        (state.candidate, state.candidate_ticks)
    }

    /// Mengembalikan jumlah tick hold de-eskalasi yang dikonfigurasi.
    pub fn deescalation_hold_ticks(&self) -> u64 {
        self.deescalation_hold_ticks
    }

    /// Mengembalikan jumlah tick hold eskalasi yang dikonfigurasi.
    pub fn escalation_hold_ticks(&self) -> u64 {
        self.escalation_hold_ticks
    }

    /// Mereset status internal stabilizer ke kondisi awal (belum diinisialisasi).
    pub fn reset(&self) {
        let mut state = self.state.lock().expect("stabilizer mutex poisoned");
        state.current = None;
        state.candidate = None;
        state.candidate_ticks = 0;
    }
}

/// Kebijakan penempatan adaptif AURA untuk penentuan tingkatan penyimpanan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptiveTieringPolicy {
    /// Batas latensi (ns) di mana halaman dianggap hot / latency-critical
    pub hot_latency_threshold_ns: u64,
    /// Batas latensi (ns) untuk promosi selektif dari tier Compressed ke Memory saat tekanan Low.
    pub promotion_latency_threshold_ns: u64,
    /// Apakah promosi selektif aktif saat MemoryPressure::Low.
    /// Jika false (klasik Milestone C), seluruh halaman didekompresi ke Memory saat Low.
    /// Jika true (Milestone D reliable), hanya halaman hot (latency <= promotion_threshold) yang didekompresi,
    /// sedangkan halaman cold tetap berada di Compressed untuk menghemat RAM.
    pub selective_promotion: bool,
}

impl Default for AdaptiveTieringPolicy {
    fn default() -> Self {
        Self {
            hot_latency_threshold_ns: 500,
            promotion_latency_threshold_ns: 1500,
            selective_promotion: false,
        }
    }
}

impl AdaptiveTieringPolicy {
    /// Membuat kebijakan dengan batas latensi tertentu (promosi selektif nonaktif secara default).
    pub const fn new(hot_latency_threshold_ns: u64) -> Self {
        Self {
            hot_latency_threshold_ns,
            promotion_latency_threshold_ns: hot_latency_threshold_ns.saturating_add(1000),
            selective_promotion: false,
        }
    }

    /// Membuat kebijakan reliable dengan promosi selektif aktif.
    pub const fn reliable(hot_latency_threshold_ns: u64) -> Self {
        Self {
            hot_latency_threshold_ns,
            promotion_latency_threshold_ns: hot_latency_threshold_ns.saturating_add(1000),
            selective_promotion: true,
        }
    }

    /// Menentukan batas latensi kustom untuk promosi selektif ke Memory.
    pub const fn with_promotion_threshold(mut self, threshold_ns: u64) -> Self {
        self.promotion_latency_threshold_ns = threshold_ns;
        self
    }

    /// Mengaktifkan atau menonaktifkan promosi selektif.
    pub const fn with_selective_promotion(mut self, enabled: bool) -> Self {
        self.selective_promotion = enabled;
        self
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
                if self.selective_promotion {
                    // Pada mode promosi selektif, hanya dekompresi halaman yang benar-benar hot.
                    // Halaman cold tetap disimpan di Compressed untuk mencegah bloating memori.
                    if cost.latency_ns <= self.promotion_latency_threshold_ns {
                        StorageTier::Memory
                    } else {
                        StorageTier::Compressed
                    }
                } else {
                    // Pada mode klasik, prioritaskan kecepatan akses tanpa dekompresi
                    // dengan membiarkan seluruh halaman berada di Memory (raw) tier.
                    StorageTier::Memory
                }
            }
        }
    }
}

/// Koordinator tingkat tinggi yang mengorkestrasi evaluasi telemetri PressureIQ,
/// stabilisasi histeresis, pembatasan anggaran migrasi, dan eksekusi di HyperMemory.
#[derive(Debug, Clone, Default)]
pub struct AuraTieringCoordinator {
    pub policy: AdaptiveTieringPolicy,
    pub stabilizer: PressureStabilizer,
    pub migration_budget: MigrationBudget,
}

impl AuraTieringCoordinator {
    /// Membuat koordinator klasik (tanpa jeda histeresis dan anggaran migrasi tak terbatas).
    pub fn new(policy: AdaptiveTieringPolicy) -> Self {
        Self {
            policy,
            stabilizer: PressureStabilizer::direct(),
            migration_budget: MigrationBudget::UNLIMITED,
        }
    }

    /// Membuat koordinator dengan konfigurasi reliable (histeresis de-eskalasi dan pembatasan laju migrasi).
    pub fn reliable(
        policy: AdaptiveTieringPolicy,
        deescalation_hold_ticks: u64,
        budget: MigrationBudget,
    ) -> Self {
        Self {
            policy,
            stabilizer: PressureStabilizer::new(deescalation_hold_ticks, 0),
            migration_budget: budget,
        }
    }

    /// Membuat koordinator dengan opsi lengkap.
    pub fn with_options(
        policy: AdaptiveTieringPolicy,
        stabilizer: PressureStabilizer,
        migration_budget: MigrationBudget,
    ) -> Self {
        Self {
            policy,
            stabilizer,
            migration_budget,
        }
    }

    /// Memperbarui anggaran migrasi per siklus.
    pub fn with_budget(mut self, budget: MigrationBudget) -> Self {
        self.migration_budget = budget;
        self
    }

    /// Memperbarui stabilizer histeresis.
    pub fn with_stabilizer(mut self, stabilizer: PressureStabilizer) -> Self {
        self.stabilizer = stabilizer;
        self
    }

    /// Melakukan satu langkah evaluasi telemetri dan migrasi tiering pada PageStore.
    ///
    /// Alur eksekusi:
    /// 1. Menginterpretasikan telemetri PressureIQ melalui `PressureBridge` (fail-safe ke Moderate jika tidak valid).
    /// 2. Melewatkan tekanan mentah ke `PressureStabilizer` untuk menyaring lonjakan transien.
    /// 3. Menerapkan keputusan tiering ke `PageStore` dengan mematuhi `migration_budget`.
    pub fn step(
        &self,
        store: &mut PageStore,
        diagnosis: &Diagnosis,
    ) -> Result<TieringReport, HyperMemoryError> {
        let raw_pressure = PressureBridge::evaluate_pressure(diagnosis);
        let stable_pressure = self.stabilizer.step(raw_pressure);
        self.step_with_pressure(store, stable_pressure)
    }

    /// Menerapkan migrasi tiering secara langsung berdasarkan tingkat tekanan memori eksplisit
    /// dengan menghormati `migration_budget`.
    pub fn step_with_pressure(
        &self,
        store: &mut PageStore,
        pressure: MemoryPressure,
    ) -> Result<TieringReport, HyperMemoryError> {
        store.apply_tiering_with_budget(self.migration_budget, |_id, _current_tier, cost| {
            self.policy.decide(pressure, cost)
        })
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
        let thrashing = mock_diagnosis(vec!["swap_thrashing"], Some(4.0), true);
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
    fn test_pressure_bridge_telemetry_validation() {
        // NaN available_pct
        let mut d_nan = mock_diagnosis(vec!["normal"], Some(f64::NAN), true);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_nan),
            TelemetryValidation::InvalidRange
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_nan),
            MemoryPressure::Moderate
        );

        // Infinite available_pct
        d_nan.available_pct = Some(f64::INFINITY);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_nan),
            TelemetryValidation::InvalidRange
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_nan),
            MemoryPressure::Moderate
        );

        // Negative available_pct
        let d_neg = mock_diagnosis(vec!["normal"], Some(-1.0), true);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_neg),
            TelemetryValidation::InvalidRange
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_neg),
            MemoryPressure::Moderate
        );

        // Over 100% available_pct
        let d_overflow = mock_diagnosis(vec!["normal"], Some(105.0), true);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_overflow),
            TelemetryValidation::InvalidRange
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_overflow),
            MemoryPressure::Moderate
        );

        // Missing available_pct
        let d_missing_avail = mock_diagnosis(vec!["normal"], None, true);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_missing_avail),
            TelemetryValidation::MissingMetrics
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_missing_avail),
            MemoryPressure::Moderate
        );

        // Missing interval_seconds
        let mut d_missing_interval = mock_diagnosis(vec!["normal"], Some(50.0), true);
        d_missing_interval.interval_seconds = None;
        assert_eq!(
            PressureBridge::validate_telemetry(&d_missing_interval),
            TelemetryValidation::MissingMetrics
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_missing_interval),
            MemoryPressure::Moderate
        );

        // Non-positive interval_seconds
        d_missing_interval.interval_seconds = Some(0.0);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_missing_interval),
            TelemetryValidation::InvalidRange
        );

        // Contradictory classes: normal + swap_thrashing
        let d_contra_classes = mock_diagnosis(vec!["normal", "swap_thrashing"], Some(4.0), true);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_contra_classes),
            TelemetryValidation::ContradictorySignals
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_contra_classes),
            MemoryPressure::Moderate
        );

        // Contradictory values: swap_thrashing with 80% available RAM
        let d_contra_thrash = mock_diagnosis(vec!["swap_thrashing"], Some(80.0), true);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_contra_thrash),
            TelemetryValidation::ContradictorySignals
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_contra_thrash),
            MemoryPressure::Moderate
        );

        // Contradictory values: normal with 2% available RAM
        let d_contra_normal = mock_diagnosis(vec!["normal"], Some(2.0), true);
        assert_eq!(
            PressureBridge::validate_telemetry(&d_contra_normal),
            TelemetryValidation::ContradictorySignals
        );
        assert_eq!(
            PressureBridge::evaluate_pressure(&d_contra_normal),
            MemoryPressure::Moderate
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

        // Low pressure without selective promotion -> both hot and cold stay in Memory
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

        // High pressure -> both move to Compressed
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
    fn test_selective_promotion_policy() {
        let policy = AdaptiveTieringPolicy::reliable(600);

        let hot_cost = CostEstimate {
            latency_ns: 1200,
            bytes: 4096,
        };
        let cold_cost = CostEstimate {
            latency_ns: 2000,
            bytes: 4096,
        };

        // Under Low pressure with selective promotion:
        // Hot compressed pages (latency <= 1600) are promoted to Memory
        assert_eq!(
            policy.decide(MemoryPressure::Low, hot_cost),
            StorageTier::Memory
        );
        // Cold compressed pages (latency > 1600) remain Compressed to preserve physical RAM
        assert_eq!(
            policy.decide(MemoryPressure::Low, cold_cost),
            StorageTier::Compressed
        );
    }

    #[test]
    fn test_pressure_stabilizer_hysteresis_and_emergency() {
        let stabilizer = PressureStabilizer::new(3, 0); // 3 ticks de-escalation, 0 escalation

        // Tick 0: Initial state is Moderate
        assert_eq!(
            stabilizer.step(MemoryPressure::Moderate),
            MemoryPressure::Moderate
        );
        assert_eq!(stabilizer.current(), Some(MemoryPressure::Moderate));

        // Attempt de-escalation to Low (Tick 1): held at Moderate
        assert_eq!(
            stabilizer.step(MemoryPressure::Low),
            MemoryPressure::Moderate
        );
        assert_eq!(stabilizer.candidate(), (Some(MemoryPressure::Low), 1));

        // Attempt de-escalation to Low (Tick 2): held at Moderate
        assert_eq!(
            stabilizer.step(MemoryPressure::Low),
            MemoryPressure::Moderate
        );
        assert_eq!(stabilizer.candidate(), (Some(MemoryPressure::Low), 2));

        // Transient bounce back to Moderate: cancels candidate
        assert_eq!(
            stabilizer.step(MemoryPressure::Moderate),
            MemoryPressure::Moderate
        );
        assert_eq!(stabilizer.candidate(), (None, 0));

        // Consecutive de-escalation ticks:
        assert_eq!(
            stabilizer.step(MemoryPressure::Low),
            MemoryPressure::Moderate
        ); // tick 1
        assert_eq!(
            stabilizer.step(MemoryPressure::Low),
            MemoryPressure::Moderate
        ); // tick 2
        assert_eq!(stabilizer.step(MemoryPressure::Low), MemoryPressure::Low); // tick 3: committed!
        assert_eq!(stabilizer.current(), Some(MemoryPressure::Low));

        // Immediate escalation to High (escalation_hold_ticks = 0)
        assert_eq!(stabilizer.step(MemoryPressure::High), MemoryPressure::High);
        assert_eq!(stabilizer.current(), Some(MemoryPressure::High));

        // Emergency escalation to Critical: always immediate regardless of hold settings
        let strict_stabilizer = PressureStabilizer::new(5, 5);
        assert_eq!(
            strict_stabilizer.step(MemoryPressure::Low),
            MemoryPressure::Low
        );
        assert_eq!(
            strict_stabilizer.step(MemoryPressure::Critical),
            MemoryPressure::Critical
        );
        assert_eq!(strict_stabilizer.current(), Some(MemoryPressure::Critical));
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

    #[test]
    fn test_coordinator_reliable_with_budget_and_hysteresis() {
        let mut store = PageStore::adaptive(4096, 8, 131072).unwrap();
        let mut ids = Vec::new();
        for i in 0..4u8 {
            ids.push(store.insert(&vec![i; 4096]).unwrap());
        }

        // Reliable coordinator: hold_ticks = 3, budget max_pages = 2
        let budget = MigrationBudget::bounded(2, usize::MAX);
        let coordinator =
            AuraTieringCoordinator::reliable(AdaptiveTieringPolicy::new(500), 3, budget);

        // Initial step under Low pressure: 2 pages migrated, 2 deferred by budget
        let diag_low = mock_diagnosis(vec!["normal"], Some(85.0), true);
        let rep1 = coordinator.step(&mut store, &diag_low).unwrap();
        assert_eq!(rep1.inspected_pages, 4);
        assert_eq!(rep1.migrated_to_memory, 2);
        assert_eq!(rep1.deferred_by_migration_budget, 2);

        // Second step finishes remaining 2 pages
        let rep2 = coordinator.step(&mut store, &diag_low).unwrap();
        assert_eq!(rep2.migrated_to_memory, 2);
        assert_eq!(rep2.deferred_by_migration_budget, 0);

        for id in &ids {
            assert_eq!(store.page_tier(*id).unwrap(), StorageTier::Memory);
        }
    }
}
