# Laporan Rekayasa HyperCore: Milestone C — AURA Adaptive Memory Tiering

## 1. Identifikasi Milestone
- **Milestone**: MILESTONE C — AURA Adaptive Memory Tiering.
- **Repository**: `samuelindra123/xynoos-hypercore`
- **Cabang Pengembangan**: `feat/hypercore-aura-tiering`
- **Base Branch**: `main` (commit `8d7cd48`)
- **Tujuan Rekayasa**: Menghubungkan engine telemetri Linux `PressureIQ`, mesin penempatan adaptif `AURA`, dan engine cache memori `HyperMemory` (`PageStore`) untuk mengorkestrasi transisi tingkatan penyimpanan memori (*memory tier migration*) userspace secara otonom, deterministik, dan terverifikasi di bawah variasi tekanan memori sistem Linux.

---

## 2. Status Awal Repositori
- **HEAD Commit Main**: `8d7cd48` (Merge Pull Request #52: "feat: implement HyperCore memory engine correctness, deterministic eviction & observability").
- **Kondisi Sebelum Milestone C**:
  - `HyperMemory` (`PageStore`) telah memiliki mekanisme penggusuran deterministik (LRU & FIFO), pembacaan *zero-allocation* (`get_into`), pembaruan in-place atomik (`update`), dan telemetri runtime operasional.
  - Namun, penyimpanan halaman bersifat statis saat disisipkan (ditentukan saat `insert` berdasarkan apakah kompresi LZ4 menghemat ruang), tanpa kemampuan migrasi dinamis antar tingkat (*raw* vs *compressed*) selama runtime.
  - `xynoos-core::MemoryPressure` hanya mendefinisikan 3 tingkat (`Low`, `Moderate`, `High`), belum mencakup tingkat kritis (`Critical`) untuk situasi darurat seperti *swap thrashing* atau kepunahan memori bebas.
  - Belum ada jembatan telemetri (`PressureBridge`) yang memetakan sinyal diagnosa `PressureIQ::Diagnosis` ke keputusan migrasi memori.
  - Belum ada koordinator tiering adaptif userspace (`AuraTieringCoordinator`) yang mengorkestrasi evaluasi tekanan dan eksekusi migrasi tanpa overhead alokasi atau kepunahan data.

---

## 3. Desain Arsitektur & Prinsip Rekayasa

```
+-------------------------------------------------------------------+
|               Linux Kernel Telemetry (/proc, /sys)                |
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                    xynoos-pressureiq                              |
|           Observation -> Diagnosis (PSI, Cgroup, ZRAM)            |
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                        xynoos-aura                                |
|  PressureBridge: Diagnosis -> MemoryPressure                      |
|  AdaptiveTieringPolicy: DecisionPolicy (Latency, Recency, Budget) |
|  AuraTieringCoordinator: Orchestrator                             |
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                   xynoos-hypermemory                              |
|  PageStore:                                                       |
|  - StorageTier::Memory <===========> StorageTier::Compressed      |
|  - migrate_page() (Fail-closed transaction safety)                |
|  - apply_tiering() (Deterministic iteration order)                |
|  - Checksum validation (CRC32 payload & original)                 |
+-------------------------------------------------------------------+
```

### 3.1 Kontrak Kegagalan Fail-Safe Telemetri
Jika telemetri kernel tidak lengkap, file `/proc/pressure` tidak tersedia, atau pembacaan dianggap kedaluwarsa (`diagnosis.reliable == false`), `PressureBridge` secara konservatif menetapkan tekanan memori pada tingkat `MemoryPressure::Moderate`. Sistem tidak pernah melakukan kepanikan (*panic*), pembongkaran dekompresi agresif, atau penggusuran buta saat integritas telemetri diragukan.

### 3.2 Kontrak Keamanan Transaksional Migrasi (*Fail-Closed Migration Safety*)
Operasi `PageStore::migrate_page(id, target)` dirancang dengan batas komit eksplisit:
1. **Migrasi Memory -> Compressed**: Mengompresi payload via LZ4 in-place, memverifikasi CRC32 payload, dan mengurangi kapasitas akuntansi (`accounted_bytes`). Kompresi membebaskan ruang memori fisik.
2. **Migrasi Compressed -> Memory**: Mendekompresi payload ke buffer sementara, memverifikasi CRC32 terhadap data asli, dan menghitung penambahan biaya akuntansi. Jika `accounted + diff > budget`, migrasi langsung dibatalkan (*atomic rollback*) dengan mengembalikan `Err(Error::BudgetExceeded)` tanpa memodifikasi atau merusak entri terkompresi.

---

## 4. Rincian Implementasi Teknis

### 4.1 Ekstensi Fondasi Core (`xynoos-core`)
- Menambahkan varian `Critical` pada enum `MemoryPressure` dengan turunan `PartialOrd`, `Ord`, `Hash`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub enum MemoryPressure {
      Low,
      Moderate,
      High,
      Critical,
  }
  ```

### 4.2 Mesin Migrasi Halaman In-Place (`xynoos-hypermemory`)
- Menambahkan metode inspeksi tier:
  `PageStore::page_tier(&self, id: PageId) -> Result<StorageTier, Error>`
- Menambahkan estimasi biaya berbasis resensi akses:
  `PageStore::page_cost_estimate(&self, id: PageId) -> Result<CostEstimate, Error>`
  Memperhitungkan penalti jarak waktu akses terakhir (`recency_ticks = current_tick - last_accessed_tick`) untuk membedakan halaman *hot* (akses baru) dan halaman *cold* (akses lampau).
- Menambahkan metode migrasi transaksional in-place:
  `PageStore::migrate_page(&mut self, id: PageId, target: StorageTier) -> Result<bool, Error>`
- Menambahkan metode eksekusi kebijakan massal deterministik:
  `PageStore::apply_tiering<F>(&mut self, mut decide: F) -> Result<TieringReport, Error>`
  Menjamin iterasi berurutan berdasarkan `PageId.0` untuk reproduktibilitas mutlak.
- Menambahkan struktur pelaporan `TieringReport`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub struct TieringReport {
      pub inspected_pages: usize,
      pub migrated_to_compressed: usize,
      pub migrated_to_memory: usize,
      pub no_change: usize,
      pub net_accounted_delta: isize,
  }
  ```

### 4.3 Mesin Kebijakan & Jembatan Telemetri (`xynoos-aura`)
- Modul baru `crates/xynoos-aura/src/tiering.rs`:
  - `PressureBridge`: Memetakan `Diagnosis` ke `MemoryPressure` dengan deteksi *swap thrashing*, *active direct reclaim*, *sustained pressure*, serta ambang batas RAM bebas (5%, 15%, 25%).
  - `AdaptiveTieringPolicy`: Mengimplementasikan `DecisionPolicy` dengan parameter `hot_latency_threshold_ns`:
    - `Critical` & `High`: Seluruh halaman dimigrasikan ke `StorageTier::Compressed`.
    - `Moderate`: Halaman *hot* (`latency_ns <= threshold`) dipertahankan di `StorageTier::Memory`; halaman *cold* dimigrasikan ke `StorageTier::Compressed`.
    - `Low`: Seluruh halaman diizinkan berada di `StorageTier::Memory` untuk memaksimalkan performa *zero-decompression*.
  - `AuraTieringCoordinator`: Koordinator non-intrusif yang mengevaluasi telemetri dan memicu migrasi berurutan pada `PageStore`.

---

## 5. Verifikasi & Pengujian

### 5.1 Pengujian Unit Baru
- `crates/xynoos-hypermemory`:
  - `test_tier_query_and_roundtrip_migration`: Memvalidasi pembacaan status tier, migrasi bolak-balik Memory <-> Compressed, dan integritas bit data asli.
  - `test_migration_accounting_delta`: Memvalidasi penyusutan dan penambahan `accounted_bytes` yang presisi saat siklus kompresi/dekompresi.
  - `test_migration_budget_protection`: Memvalidasi penolakan dekompresi saat budget terbatas tanpa merusak data terkompresi.
  - `test_apply_tiering_deterministic`: Memvalidasi eksekusi deterministik massal dan akurasi `TieringReport`.
- `crates/xynoos-aura`:
  - `test_pressure_bridge_evaluations`: Memvalidasi seluruh cabang pemetaan diagnosa (Normal, Elevated, Sustained, Thrashing, dan Unreliable fallback).
  - `test_adaptive_policy_decisions`: Memvalidasi keputusan penempatan tiering di bawah kombinasi tekanan dan biaya latensi.
  - `test_tiering_coordinator_end_to_end`: Memvalidasi siklus orkestrasi lengkap coordinator terhadap store.

### 5.2 Pengujian Integrasi Siklus Hidup Multi-Fase (`adaptive_tiering_integration.rs`)
Pengujian integrasi end-to-end memvalidasi 5 fase siklus hidup sistem di bawah variasi tekanan dinamis:
1. **Fase 1 (Low Pressure)**: Seluruh 10 halaman berpindah ke `Memory` tier (raw), siap untuk pembacaan instan tanpa dekompresi.
2. **Fase 2 (Moderate Pressure)**: Tiga halaman diakses secara intensif (*hot*). Hasil tiering: 3 halaman *hot* tetap di `Memory`, 7 halaman *cold* otomatis dikompresi ke `Compressed` tier.
3. **Fase 3 (High Pressure)**: Tekanan meningkat tajam. Tiga halaman tersisa otomatis dikompresi ke `Compressed` tier. Akuntansi memori menyusut signifikan.
4. **Fase 4 (Critical Pressure)**: Swap thrashing terdeteksi. Sistem mempertahankan 100% halaman terkompresi secara stabil (`no_change: 10`).
5. **Fase 5 (Return to Low Pressure)**: Tekanan mereda. Seluruh 10 halaman didekompresi kembali ke `Memory` tier secara aman tanpa korupsi bit atau pelanggaran CRC32.
6. **Uji Perlindungan Budget**: Memvalidasi bahwa dekompresi berhenti secara aman saat budget habis tanpa menyebabkan *out-of-memory* atau membatalkan halaman yang sudah didekompresi.
7. **Uji Telemetri Basi/Tidak Lengkap**: Memvalidasi fallback deterministik ke `Moderate` saat telemetri kehilangan reliabilitas.

---

## 6. Ringkasan Eksekusi CI & Kualitas Kode
- **Format Check**: `cargo fmt --all -- --check` — PASSED (0 diffs).
- **Strict Clippy**: `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASSED (0 warnings).
- **Full Workspace Test Suite**: `cargo test --workspace --locked` — PASSED:
  - `xynoos-core`: 0 tests (pure types/traits).
  - `xynoos-hypermemory`: 30 tests passed, 0 failed.
  - `xynoos-aura`: 48 tests passed (45 unit + 3 integration), 0 failed.
  - `xynoos-pressureiq`: 21 tests passed, 0 failed.
  - `xynoos-decision`: 5 tests passed, 0 failed.
  - `xynoos-cli`: 11 tests passed, 0 failed.
  - `xynoos-linux-control`: 13 tests passed, 0 failed.
  - **Total**: 128 tests passed secara menyeluruh.
- **Keselamatan Memori**: `#![forbid(unsafe_code)]` dipatuhi secara absolut pada seluruh crates (`xynoos-core`, `xynoos-hypermemory`, `xynoos-aura`, `xynoos-pressureiq`, `xynoos-decision`, `xynoos-linux-control`, `xynoos-bench`).

---

## 7. Status & Langkah Berikutnya
1. Commit dan push branch `feat/hypercore-aura-tiering` ke remote origin.
2. Membuka Pull Request untuk Milestone C.
3. Memverifikasi kelulusan seluruh matriks GitHub Actions CI pada Pull Request.
