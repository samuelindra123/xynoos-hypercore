# Laporan Rekayasa HyperCore: Milestone G — Adaptive Compression, Resource Cost Intelligence & Runtime Reliability

## A. Ringkasan Eksekutif

**Milestone G — Adaptive Compression, Resource Cost Intelligence & Runtime Reliability** telah diimplementasikan secara penuh, diaudit dari temuan Phase 0, dan diverifikasi melalui seluruh pipeline GitHub Actions pada cabang `feat/hypercore-milestone-g-adaptive-compression` (PR #70).

Pencapaian rekayasa utama yang telah terbukti melalui pengujian dan tolok ukur CI:
1. **Perbaikan Audit Phase 0**: Memperbaiki bug *double-tick / double-hit* pada `ClusterStore::get_page_into`, bug mutasi tick sebelum commit pada `ClusterStore::insert_cluster`, pertumbuhan tak terbatas `ClusterStore::evicted_history`, celah akuntansi `page_crcs`, loop telemetri tiruan pada `AuraDaemon::run_with_signal`, salah baca `read_low()` sebagai penggunaan memori cgroup, risiko integer overflow pada `compute_workload_aware_action`, serta hardcoded `available_pct: 50.0` pada `xynoos profile record`.
2. **Arsitektur Kompresi Adaptif (`xynoos-hypermemory/src/codec.rs`)**: Menambahkan `CodecId` eksplisit (`Raw`, `Lz4Standard`, `Lz4Hc { level }`), `CompressionConfig`, `CompressedObjectMeta`, buffer `CodecScratch` terikat, dan encoder **LZ4HC pure-safe-Rust** (`#![forbid(unsafe_code)]`) berbasis *hash-chain + lazy matching* yang kompatibel penuh dengan decoder blok LZ4 standar (`lz4_flex::block::decompress_into`).
3. **Resource Cost Intelligence & Mesin Keputusan (`cost_model.rs`, `compression_policy.rs`)**: Mengimplementasikan model biaya deterministik berbasis statistik bergulir terikat (32 sampel) dan EWMA *fixed-point* (permille) yang membedakan penghematan bersih setelah overhead metadata, scratch, biaya CPU kompresi/rekompresi, dan penalti latensi baca.
4. **Telemetri Multi-Sumber PressureIQ (`multi_resource.rs`)**: Memperluas pembacaan read-only untuk CPU PSI, Memory PSI, I/O PSI, `/proc/meminfo`, `/proc/stat`, `/proc/vmstat`, dan cgroup v2 dengan penanganan eksplisit atas PSI yang hilang, izin ditolak, nilai tak valid, reset counter, dan kedaluwarsa tanpa pernah memalsukan metrik.
5. **Kalibrasi Kebijakan Bayangan (`ShadowCalibrator` & `PolicyConfig`)**: Menambahkan skema `PolicyConfig` berversi dengan validasi ketat serta kalibrasi offline terikat dengan pemisahan *calibration* (50%), *validation* (25% anti-overfitting), dan *evaluation* (25%).
6. **Ketahanan Daemon & Pemulihan Crash (`AuraDaemon`, `PidLock`, `TransactionJournal`)**: Mengukur live RSS daemon `ObserveOnly` pada **3.212 KiB (~3,14 MiB)** (jauh di bawah target < 10 MiB) dan startup **18 µs**, serta mengeraskan `PidLock` (`O_CREAT | O_EXCL`, `boot_id`, `start_ticks`) dan `TransactionJournal` (tulis sementara + `fsync` + `rename` atomik + sync direktori + CRC32 + deteksi konflik *fail-closed*).
7. **Validasi Kernel cgroup v2 Terisolasi**: Membuktikan eksekusi nyata terhadap controller `memory` cgroup v2 via `systemd-run --user --scope -p Delegate=yes` (3/3 uji kernel terisolasi lulus: `MemoryLow`, `MemoryHigh`, `MemoryMax`, penolakan tak terotorisasi, simulasi pemulihan konflik, dan pembersihan cgroup sekali pakai).

---

## B. Status Repositori Awal

Status terverifikasi langsung pada repositori sebelum Milestone G dimulai:
- **HEAD `main` Awal**: `57309cd` (*"feat(hypercore): complete milestone F continuous daemon and system integration"*).
- **Pull Request Terbuka**: `0`.
- **Milestone Sebelumnya**: Milestone D (#54), Milestone E (#56), Milestone F (#62) seluruhnya telah terintegrasi ke `main` dan ditutup.
- **Workflow Pasca-Merge `57309cd`**: 4 workflow sukses (*Distribution archive validation*, *Debian package validation*, *HyperCore Engine CI*, *CLI and distribution validation*).
- **Issue Terbuka yang Dipertahankan**: #17–#23 (riset prasyarat), #36 (penyiapan kunci OpenPGP APT produksi), #37 (inisialisasi publikasi GitHub Wiki), #41–#44 (penerimaan fisik N5000, pembaruan dua versi riil, rotasi arsip, kesiapan stabil).

---

## C. Implementasi Arsitektur

### C.1 Subsistem Kompresi Adaptif (`xynoos-hypermemory/src/codec.rs`, `lib.rs`, `cluster.rs`)
- **Pemilihan Backend**: Setelah mengaudit dependensi crate (`Cargo.lock`, `packaging/prepare.py`, MSRV 1.82.0, `x86_64-unknown-linux-musl`, dan kebijakan `#![forbid(unsafe_code)]`), `lz4_flex` 0.11.7 tidak menyediakan encoder LZ4HC dan crate C/FFI ditolak demi menjaga keamanan memori serta kompatibilitas pengemasan Debian offline. Sebagai solusi teknis terverifikasi, diimplementasikan encoder **LZ4HC pure-safe-Rust** (*32K hash-head + 64K ring chain + lazy-1/lazy-2 lookahead*) yang memancarkan blok LZ4 standar valid dan diuji round-trip terhadap `lz4_flex::block::decompress_into`.
- **Metadata Eksplisit**: Setiap entri menyimpan `codec: CodecId`, `payload_crc`, `original_crc`, `last_compress_ns`, `last_decompress_ns`, `access_count`, `incompressible_flagged`, dan `last_transition_tick`.
- **Optimasi Scratch**: `CodecScratch` menyimpan buffer `encode_buf`, `decode_buf`, `hc_head`, dan `hc_chain` yang dipakai ulang dengan batas `max_scratch_bytes`, dikombinasikan dengan `try_reserve_exact` untuk menghindari slack alokasi dari `get_maximum_output_size`.

### C.2 Resource Cost Intelligence & Decision Engine (`xynoos-aura/src/cost_model.rs`, `compression_policy.rs`)
- **Tanpa Runtime ML Berat**: Menggunakan EWMA integer *fixed-point* (`alpha = 250 / 1000`) dan jendela statistik bergulir terikat (`ROLLING_WINDOW_CAP = 32`).
- **Klasifikasi Suhu**: `PageTemperature::{Hot, Warm, Cold}` berdasarkan `recency_ticks` dan `access_count`.
- **Keputusan Teraudit**: `CompressionDecisionEngine` menghasilkan `CompressionAction` (`PreserveRaw`, `ApplyStandardLz4`, `ApplyLz4Hc { level }`, `RetainExisting`, `DeferCompression`, `DeferMigration`, `PromoteToRaw`, `FallbackSafe`) beserta `DecisionReason` eksplisit.

### C.3 Telemetri Multi-Sumber (`xynoos-pressureiq/src/multi_resource.rs`)
- Membaca `/proc/pressure/{cpu,memory,io}`, `/proc/meminfo`, `/proc/vmstat`, `/proc/stat`, dan batas cgroup v2 (`memory.current`, `memory.low`, `memory.high`).
- Sumber yang tidak tersedia, ditolak izinnya (`Permission`), terkena *counter reset*, atau bertentangan (`MemAvailable > MemTotal`) dilaporkan sebagai `Err` atau `None` dan dicatat pada `limitations`; tidak ada angka tiruan yang disintesis.

### C.4 Kalibrasi Kebijakan Bayangan (`xynoos-aura/src/profiler.rs`)
- `PolicyConfig` berversi (`schema_version = 1`) dengan validasi batas ketat (`validate()`) dan jalur rollback (`PolicyConfig::safe_default()`).
- `ShadowCalibrator::calibrate` membagi trace secara deterministik menjadi 50% kalibrasi, 25% validasi (*overfitting guard*), dan 25% evaluasi terhadap 4 pembanding (`static_standard_lz4`, `milestone_f_baseline`, `milestone_g_default`, `milestone_g_calibrated`), dengan label eksplisit `measurement_kind = "counterfactual_replay_estimate"` dan `advisory_only = true`.

### C.5 Optimasi Daemon & Ketahanan Crash (`xynoos-aura/src/daemon.rs`, `systemd/xynoos-hypercore.service`)
- `AuraDaemon::run_with_signal` kini menggunakan kolektor `xynoos_pressureiq::intelligence::collect()` nyata dan `diagnose()` antar-siklus, histori terikat 32 entri, *adaptive polling*, pelaporan RSS langsung (`/proc/self/statm`), serta penekanan logging error berulang (*rate-limited*).
- `systemd/xynoos-hypercore.service` menambahkan `RuntimeDirectory=xynoos` agar direktori `/run/xynoos` tersedia aman di bawah `ProtectSystem=strict`.

---

## D. Algoritma & Model Keputusan

### D.1 Fungsi Biaya Matematis (`ResourceCostModel::evaluate_transition`)
Untuk transisi kandidat dari codec saat ini ke codec target pada objek berukuran $B_{\text{orig}}$ byte:

$$\text{Score} = \Delta B_{\text{net}} \cdot W_{\text{mem}}(P_{\text{mem}}) - \frac{(C_{\text{cpu}} + C_{\text{recomp}} + C_{\text{lat}}) \cdot M_{\text{cpu}}(P_{\text{cpu}})}{1000} - P_{\text{scratch}}$$

Dengan komponen terukur:
- $\Delta B_{\text{net}} = B_{\text{curr\_stored}} - \lfloor B_{\text{orig}} \cdot r_{\text{target}} / 1000 \rfloor$: selisih byte tersimpan bersih (dengan pemisahan `metadata_overhead_bytes` dan `temp_scratch_overhead_bytes`).
- $W_{\text{mem}}(P_{\text{mem}}) \in \{4, 64, 512, 1024\}$ ns/byte untuk tekanan memori $\{Low, Moderate, High, Critical\}$.
- $C_{\text{cpu}}$: biaya CPU kompresi/dekompresi berbasis EWMA `ns_per_kib`.
- $C_{\text{recomp}}$: penalti dekompresi tambahan saat rekompresi antar-codec terkompresi.
- $C_{\text{lat}}$: penalti latensi dekompresi masa depan dikalikan bobot ekspektasi baca ($Hot=8, Warm=2, Cold=0$).
- $M_{\text{cpu}}(P_{\text{cpu}}) \in \{1000, 2500, 8000, 32000\}$ permille untuk tekanan CPU $\{Low, Moderate, High, Critical\}$.
- $P_{\text{scratch}}$: penalti 50.000 jika RAM tersedia $< 8 \times \text{temp\_scratch\_overhead\_bytes}$.

### D.2 Aturan & Batas Keamanan
1. Telemetri tidak valid/tidak andal $\rightarrow$ `FallbackSafe` (tidak melakukan kompresi/promosi agresif).
2. `CpuPressure >= High` $\rightarrow$ memblokir `Lz4Hc`; `CpuPressure == Critical` (selama memori bukan `Critical`) $\rightarrow$ `DeferCompression`.
3. `incompressible_flagged == true` $\rightarrow$ `PreserveRaw` permanen untuk mencegah pengulangan kompresi sia-sia.
4. *Cooldown* (`cooldown_ticks = 4`) mencegah osilasi kebijakan antar-siklus (dikecualikan pada eskalasi `Critical`).
5. Rekompresi `Lz4Standard -> Lz4Hc` ditolak bila tambahan penghematan $< 256$ byte (`min_recompression_savings_bytes`).

**Keterbatasan Model**: Bobot linear hanyalah aproksimasi terhadap biaya *page fault* / *reclaim* kernel non-linear dan tidak mengamati tekanan *scheduler* aplikasi secara langsung; model ini merupakan heuristik deterministik berbasis EWMA, bukan algoritma *machine learning*.

---

## E. Hasil Tolok Ukur Kompresi (`xynoos-bench adaptive-campaign`, GitHub Actions Run `45527852564`)

Seluruh pengukuran dieksekusi di runner GitHub Actions (`ubuntu-24.04`, 4 vCPU, mode `--release --locked`) pada **64 halaman × 4.096 byte = 262.144 byte (256 KiB)** per dataset:

| Dataset | Rezim | Kebijakan | Stored (B) | Rasio | Net Savings (B) | Compress (MiB/s) | Decompress (MiB/s) | P50 (ns) | P99 (ns) |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `zero_pages` | `cold_heavy` | `baseline_a_milestone_f` | 1.728 | 0,0066 | +249.443 | 1.623,27 | 14.235,89 | 241 | 261 |
| `zero_pages` | `cold_heavy` | `baseline_b_static_lz4` | 1.728 | 0,0066 | +249.443 | 6.653,18 | 14.175,25 | 250 | 251 |
| `zero_pages` | `cold_heavy` | `candidate_c_adaptive_g` | 1.728 | 0,0066 | +249.443 | 6.183,53 | 14.219,59 | 241 | 260 |
| `zero_pages` | `cold_heavy` | `candidate_d_lz4hc6` | 1.664 | 0,0063 | +102.051 | 223,04 | 14.131,08 | 250 | 261 |
| `structured_logs` | `cold_heavy` | `baseline_b_static_lz4` | 41.576 | 0,1586 | +209.595 | 2.220,09 | 5.314,57 | 681 | 781 |
| `structured_logs` | `cold_heavy` | `candidate_c_adaptive_g` | 41.576 | 0,1586 | +209.595 | 7.136,33 | 5.431,93 | 691 | 711 |
| `structured_logs` | `cold_heavy` | `candidate_d_lz4hc6` | 40.000 | 0,1526 | +63.715 | 139,65 | 5.156,74 | 681 | 1.182 |
| `structured_logs` | `hot_random` | `baseline_b_static_lz4` | 41.576 | 0,1586 | +209.595 | 2.222,26 | 5.307,22 | 691 | 721 |
| `structured_logs` | `hot_random` | `candidate_c_adaptive_g` | 82.931 | 0,3164 | +168.240 | 9.150,47 | **9.059,16** | **331** | 931 |
| `source_code` | `cold_heavy` | `baseline_b_static_lz4` | 8.874 | 0,0339 | +242.297 | 5.375,19 | 7.390,57 | 461 | 721 |
| `source_code` | `cold_heavy` | `candidate_d_lz4hc6` | 8.597 | 0,0328 | +95.118 | 218,94 | 7.929,33 | 461 | 480 |
| `source_code` | `hot_random` | `candidate_c_adaptive_g` | 56.362 | 0,2150 | +194.809 | 9.117,10 | **10.263,09** | **330** | 481 |
| `binary_like` | `cold_heavy` | `baseline_b_static_lz4` | 167.743 | 0,6399 | +83.428 | 548,35 | 1.831,96 | 2.074 | 2.233 |
| `binary_like` | `cold_heavy` | `candidate_d_lz4hc6` | 161.387 | 0,6156 | -57.672 | 47,43 | 1.861,62 | 2.043 | 2.173 |
| `binary_like` | `hot_random` | `candidate_c_adaptive_g` | 185.443 | 0,7074 | +65.728 | 9.180,71 | **5.846,71** | **331** | 2.203 |
| `mixed_compressibility` | `cold_heavy` | `candidate_c_adaptive_g` | 114.919 | 0,4384 | +136.252 | 9.473,29 | 4.732,34 | 941 | 1.682 |
| `mixed_compressibility` | `hot_random` | `candidate_c_adaptive_g` | 142.519 | 0,5437 | +108.652 | 9.476,88 | **8.935,51** | **331** | 1.683 |
| `high_entropy_random` | `cpu_constrained` | `candidate_c_adaptive_g` | 262.144 | 1,0000 | -10.973 | 9.816,24 | 10.576,19 | 321 | 370 |
| `high_entropy_random` | `cpu_constrained` | `candidate_d_lz4hc6` | 262.144 | 1,0000 | -158.429 | 114,65 | 11.039,84 | 321 | 371 |
| `synthetic_app_memory` | `hot_random` | `candidate_c_adaptive_g` | 183.621 | 0,7005 | +67.550 | 9.516,92 | **10.061,48** | **330** | 541 |

**Temuan Kunci**:
- Pada beban kerja `hot_random`, `candidate_c_adaptive_g` memangkas latensi baca median (P50) hingga **2,1× – 6,2×** (`691 ns → 331 ns` pada `structured_logs`, `2.043 ns → 331 ns` pada `binary_like`) karena mempertahankan halaman panas dalam format `Raw` sambil tetap mengompres halaman dingin.
- `Lz4Hc` secara konsisten menurunkan panjang payload (`41.576 → 40.000 B` pada `structured_logs`, `167.743 → 161.387 B` pada `binary_like`) dengan kecepatan dekompresi setara LZ4 standar, namun membutuhkan biaya CPU kompresi ~10–16× lebih lambat (`139–223 MiB/s`) serta buffer scratch hash-chain sebesar `151.981 B` (~148 KiB).

---

## F. Akuntansi Memori Bersih & Evaluasi Kluster (`xynoos-bench clustering-g`)

### F.1 Akuntansi Overhead Penuh (`MemoryAccounting`)
Pada uji skala 64 halaman (262.144 B logis):
- **Metadata tetap (`PageStore`)**: `6.448 byte` (termasuk `Entry`, `HashMap`, dan struktur store).
- **Scratch standar LZ4**: `4.525 byte` (terpakai ulang antar operasi).
- **Scratch LZ4HC (`hc_head` + `hc_chain`)**: `151.981 byte` (~148 KiB tetap).
- **Implikasi Kejujuran Rekayasa**: Pada store kecil (256 KiB), overhead scratch 148 KiB membuat `candidate_d_lz4hc6` menghasilkan penghematan bersih **negatif (`-57.672 B` pada `binary_like`, `-9.974 B` pada `mixed_compressibility`, `-57.817 B` pada `synthetic_app_memory`)** meskipun payload-nya lebih kecil! LZ4HC hanya menguntungkan secara memori bersih bila diamortisasi pada kapasitas store yang jauh lebih besar (misal ≥ 4 MiB) atau data dingin sangat kompresibel.

### F.2 Perbandingan Ukuran Kluster 1, 2, 4, dan 8 Halaman (`262.144 B` logis)

| Beban Kerja | Halaman/Kluster | Payload (B) | Metadata (B) | Scratch (B) | Net Savings (B) | Rasio | Seq (MiB/s) | Random P50 (ns) | Random P99 (ns) | Read Amp |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `structured_logs` | 1 (4 KiB) | 41.576 | 5.688 | 4.525 | +210.355 | 0,1586 | 5.453,99 | 861 | 901 | 1,0× |
| `structured_logs` | 2 (8 KiB) | 38.928 | 3.128 | 9.031 | **+211.057** | 0,1485 | 5.213,55 | 1.632 | 1.733 | 2,0× |
| `structured_logs` | 4 (16 KiB) | 38.720 | 1.848 | 18.042 | +203.534 | 0,1477 | 5.023,71 | 3.245 | 3.456 | 4,0× |
| `structured_logs` | 8 (32 KiB) | 38.504 | 1.208 | 36.064 | +186.368 | 0,1469 | 4.818,07 | 6.480 | 19.429 | 8,0× |
| `source_code` | 1 (4 KiB) | 8.874 | 5.688 | 4.525 | +243.057 | 0,0339 | 9.031,14 | 611 | 641 | 1,0× |
| `source_code` | 2 (8 KiB) | 5.110 | 3.128 | 9.031 | **+244.875** | 0,0195 | 14.039,42 | 731 | 761 | 2,0× |
| `source_code` | 4 (16 KiB) | 3.381 | 1.848 | 18.042 | +238.873 | 0,0129 | 9.705,72 | 972 | 1.002 | 4,0× |
| `source_code` | 8 (32 KiB) | 2.245 | 1.208 | 36.064 | +222.627 | 0,0086 | 17.616,80 | 1.873 | 1.923 | 8,0× |
| `high_entropy_random` | 1–8 | 262.144 | 5.688→1.208 | 4.525→36.064 | -10.213 → -37.272 | 1,0000 | 13.501–18.727 | 281→771 | 291→782 | 1,0× |

**Kesimpulan Kluster**: Kluster **2 halaman (8 KiB)** memberi penghematan bersih tertinggi pada beban kerja 256 KiB (`+211.057 B` pada `structured_logs`, `+244.875 B` pada `source_code`), sedangkan kluster 8 halaman menurunkan rasio hingga `0,0086` namun menaikkan *read amplification* akses acak menjadi `8,0×` dan `P99` dari `901 ns` ke `19.429 ns`.

---

## G. Performa Daemon (`xynoos-bench daemon-sustained` & Uji Proses Live CLI)

Pengukuran pada GitHub Actions Run `45527852564`:
- **Live Proses `xynoos daemon --mode observe-only --interval-ms 200` (`/proc/$PID/status`)**:
  - Live RSS (`VmRSS`): **3.212 KiB (~3,14 MiB)** (memenuhi target < 10 MiB).
  - Uji `SIGKILL` + pemulihan lock PID langsung + 2 siklus lanjutan selesai total dalam **1.115 ms** (termasuk 1 detik observasi).
- **Uji Berkelanjutan 2.000 Siklus (`daemon-sustained 2000`)**:
  - Waktu startup konstruksi `AuraDaemon::new`: **18 µs** (target < 1 detik terpenuhi).
  - RSS awal: **2.764 KiB**; RSS akhir: **2.768 KiB** (+4 KiB selama 2.000 siklus + 40 injeksi kegagalan); Peak RSS (`VmHWM`): **2.768 KiB**; `rss_within_budget = true`.
  - Total wall-clock: **13 ms**; CPU time: **13 ms** (`cycles_completed = 2000`, `cycles_failed = 40` ter-recover).
- **Catatan Soak Jangka Panjang**: Workflow manual `.github/workflows/daemon-soak.yml` (15–60 menit via `workflow_dispatch`) telah disediakan. Soak 24 jam **belum dijalankan** dan tidak diklaim.

---

## H. Validasi Interaksi Kernel (Pemisahan Mock vs Cgroup Riil vs Fisik)

1. **Uji Mock (`crates/xynoos-linux-control/tests/transactions.rs`, `crates/xynoos-aura/tests/milestone_g_integration.rs`)**: Seluruh pengujian mock lulus untuk skenario konflik, pembatalan tanpa otorisasi, dan kegagalan rollback.
2. **Uji Cgroup v2 Kernel Riil Terisolasi (`Isolated Cgroup v2 Validation` di Run `45527852564`)**:
   - Lingkungan: Runner `ubuntu-24.04` menggunakan `systemd-run --user --scope -p Delegate=yes -p MemoryAccounting=yes` dengan pemindahan proses shell ke sub-cgroup leaf agar memenuhi aturan *no-internal-processes* cgroup v2 tanpa menyentuh root cgroup runner dan tanpa `CAP_DAC_OVERRIDE`.
   - Status probe: `STATUS: DELEGATED_MEMORY_CONTROLLER_AVAILABLE_VIA_SYSTEMD_USER`.
   - Hasil (`cargo test -p xynoos-linux-control --test kernel_transaction --locked -- --ignored --nocapture`):
     - `kernel_apply_restore_and_cleanup`: **ok**
     - `kernel_multi_action_high_max_unauthorized_and_cleanup`: **ok** (`MemoryHigh`, `MemoryMax`, dan penolakan tanpa otorisasi)
     - `kernel_real_cgroup_conflict_and_restoration`: **ok**
     - Total: **3 passed, 0 failed**.
3. **Validasi Perangkat Keras Fisik N5000**: **NOT RUN / BLOCKED ON OPERATOR HARDWARE** (Issue #42 tetap terbuka; laptop pengembang lokal tidak disentuh).

---

## I. Keandalan & Pemulihan Crash

Skenario *fault-injection* yang diverifikasi pada `daemon.rs` dan `milestone_g_integration.rs`:
- **Jurnal `Pending` sebelum penulisan kernel**: `reconcile_crash_recovery` mendeteksi `current_val == original_value`, membersihkan jurnal secara aman tanpa melakukan rollback tak perlu.
- **Jurnal `Applied` setelah crash**: Memverifikasi `boot_id`, `Identity` (`device`, `inode`, `control_inode`), dan kecocokan `current_val == requested_value` sebelum memulihkan `original_value` dan memverifikasi pembacaan ulang.
- **Konflik Perubahan Eksternal**: Bila administrator/eksternal mengubah nilai cgroup ke nilai lain (misal `48 MiB`), `reconcile_crash_recovery` menolak rollback (*fail-closed*) tanpa menimpa perubahan eksternal.
- **Jurnal Korup / Parsial / CRC Mismatch**: JSON terpotong atau CRC tidak cocok ditolak secara eksplisit (*fail-closed*).
- **PID Reuse & `SIGKILL`**: `PidLock` memverifikasi `boot_id` dan `start_ticks` `/proc/<pid>/stat`, terbukti pulih bersih setelah `SIGKILL` di CI.
- **Batasan Jujur**: Pemulihan terjadi pada startup daemon berikutnya (*next-startup recovery*), bukan pemulihan instan saat `SIGKILL` atau mati listrik mendadak di tengah penulisan cgroup tanpa fsync kernel pada cgroupfs.

---

## J. Evaluasi Beban Kerja Tingkat Sistem (`xynoos-bench workload-eval`)

Hasil terpisah per lapisan di CI (`45527852564`):
1. **HyperMemory Internal Storage**: `net_savings_bytes = 82.996 B` (inklusif overhead), `decision_latency_ns = 121.563 ns`.
2. **AURA Advisory Counterfactual Replay**: Simulasi 24 langkah menghasilkan `net_savings = 30.426 B` (dilabeli eksplisit `counterfactual_replay_estimate`).
3. **Linux Telemetry**: Terbaca `logical_cpus = 4`, `cgroup_v2_observable = true`.
4. **Whole-System Responsiveness**: `transparent_app_ram_compression = false` (*not implemented; userspace-only*). **HyperCore saat ini belum mendemonstrasikan percepatan responsivitas sistem desktop secara menyeluruh** karena tidak ada modul kernel atau intersepsi memori aplikasi sewenang-wenang; hal ini dilaporkan apa adanya sesuai aturan kejujuran rekayasa.

---

## K. Penilaian Keamanan

- **`#![forbid(unsafe_code)]`**: Dipertahankan 100% pada `xynoos-core`, `xynoos-hypermemory` (termasuk encoder LZ4HC baru `codec.rs`), `xynoos-pressureiq`, `xynoos-aura`, `xynoos-decision`, dan `xynoos-bench`.
- **Dependensi**: Nol crate eksternal baru ditambahkan ke `Cargo.lock` (`packaging/prepare.py` tetap lolos tanpa masalah lisensi/vendoring).
- **Default Aman & Hak Akses**: `ObserveOnly` tetap default; `Advisory` nol penulisan kernel; `Transactional` memerlukan otorisasi eksplisit; `DisposableCgroup::create` menolak eksekusi sebagai UID 0 (`root`) dan mewajibkan direktori induk terdelegasi eksklusif milik user efektif.
- **Jurnal & Lock**: Penulisan atomik (`temp + fsync + rename + dir sync`) dan verifikasi `boot_id` + `start_ticks` menghilangkan celah TOCTOU dan *PID reuse*.

---

## L. Konsumsi Sumber Daya

- **Mesin Pengembang Lokal (Pentium N5000 / 4 GiB RAM / 57 GiB eMMC)**:
  - Penggunaan disk sebelum/sesudah tetap pada **17 GiB terpakai / 37–38 GiB tersedia** (jauh di atas syarat minimal ≥ 8 GiB kosong; nol artefak CI diunduh lokal; hanya `cargo fmt` dan `git`/`gh` ringan dijalankan lokal).
- **Eksekusi Berat di GitHub Actions**:
  - Seluruh build release, `cargo clippy --workspace --all-targets --locked -- -D warnings`, Rust `1.82.0` & `1.100.0`, `x86_64-unknown-linux-musl`, `sbuild` Debian Trixie & Bookworm lifecycle, uji cgroup v2, dan tolok ukur dijalankan sepenuhnya di runner CI.

---

## M. Status Pengiriman GitHub

- **Cabang Pengembangan**: `feat/hypercore-milestone-g-adaptive-compression`
- **Pull Request**: [#70](https://github.com/samuelindra123/xynoos-hypercore/pull/70)
- **Milestone GitHub**: #4 — *Milestone G — Adaptive Compression, Resource Cost Intelligence & Runtime Reliability*
- **Issue Milestone G**: #63, #64, #65, #66, #67, #68, #69
- **Hash Commit Cabang**: `2163274` (implementasi awal), `4180409` (kalibrasi & probe cgroup), `10de4e8` (perbaikan prior cost model & uji konflik cgroup)
- **Hasil CI Pra-Merge (`10de4e8` — 19/19 Check Hijau)**:
  - *HyperCore Engine CI* (PR): `https://github.com/samuelindra123/xynoos-hypercore/actions/runs/45527852564` (Success — termasuk Clippy, Unit/Integration/Property tests, Benchmarks, dan Isolated Cgroup v2 Validation 3/3)
  - *HyperCore Engine CI* (Push): `https://github.com/samuelindra123/xynoos-hypercore/actions/runs/45527845240` (Success)
  - *Debian package validation*: `https://github.com/samuelindra123/xynoos-hypercore/actions/runs/45527852547` (Success — Rust 1.82.0, Rust 1.100.0, Clean Trixie sbuild, Bookworm/Trixie lifecycle, Signed APT test lifecycle)
  - *CLI and distribution validation*: `https://github.com/samuelindra123/xynoos-hypercore/actions/runs/45527852539` (Success — static musl binary)
  - *Phase 5 workspace validation*: `https://github.com/samuelindra123/xynoos-hypercore/actions/runs/45527852692` (Success)
  - *Distribution archive validation*: `https://github.com/samuelindra123/xynoos-hypercore/actions/runs/45527852603` (Success)
- **Status Squash & Merge / Main**: Diintegrasikan melalui Squash and Merge pada PR #70 dan diverifikasi pasca-merge (catatan sinkronisasi hash akhir disampaikan pada ringkasan pengiriman).
- **Issue Terbuka yang Dipertahankan**: #17–#23, #36 (OpenPGP APT signing), #37 (Wiki publish), #41–#44 (kualifikasi fisik N5000 & rilis historis).

---

## N. Keterbatasan yang Diketahui

1. **Tidak Ada Kompresi RAM Aplikasi Transparan**: `HyperMemory` bekerja pada `PageStore` / `ClusterStore` userspace kooperatif; belum ada mekanisme aman untuk mencegat/mengompresi memori proses sewenang-wenang di seluruh sistem.
2. **Validasi Perangkat Keras Fisik N5000 (#42)**: Belum dijalankan (menunggu konfirmasi operator pada perangkat keras fisik).
3. **Kunci Penandatanganan OpenPGP APT Produksi (#36)**: Masih diblokir pada konfigurasi secret/identitas OpenPGP oleh pemilik repositori; publikasi APT produksi tidak dilakukan.
4. **Uji Soak Berkelanjutan 15–60 Menit / 24 Jam**: Workflow `daemon-soak.yml` tersedia via `workflow_dispatch`, namun belum dijalankan pada durasi 24 jam.
5. **Overhead Scratch LZ4HC**: Tabel hash-chain `151.981 B` membuat `Lz4Hc` tidak ekonomis untuk store kecil (< ~1 MiB) meskipun meningkatkan rasio kompresi payload.

---

## O. Milestone Rekayasa Berikutnya

Rekomendasi objektif untuk **Milestone H** berdasarkan bukti pengukuran Milestone G:
1. **Dynamic / Scaled Scratch Table untuk LZ4HC**: Mengukur dan mengecilkan jejak tabel hash-chain LZ4HC untuk ukuran blok kecil (4–16 KiB) agar `Lz4Hc` tidak memakan 148 KiB overhead tetap pada store kecil.
2. **Integrasi `AuraAdaptiveCoordinator` ke `AuraDaemon` Advisory Mode + IPC Diagnostik Ringan**: Menghubungkan `MultiResourceDiagnosis` langsung ke keputusan adaptif `AuraDaemon` dan mengeksekusi soak test `daemon-soak.yml` 15–60 menit di CI.
3. **Kualifikasi Terpandu Perangkat Keras Fisik (#42) & Penyiapan Signing (#36)**: Menjalankan uji HIL terikat pada MX Linux / Pentium Silver N5000 bersama operator.

---

## P. Keputusan Akhir

**COMPLETE AND MERGED WITH DOCUMENTED LIMITATIONS**
*(Seluruh implementasi Milestone G, audit Phase 0, tolok ukur komparatif, akuntansi overhead penuh, uji daemon live/berkelanjutan, uji cgroup v2 terisolasi nyata, serta validasi pengemasan Debian Trixie/Bookworm telah lulus di GitHub Actions dan siap/diselesaikan melalui Squash and Merge ke `main`, dengan keterbatasan fisik #42, signing #36, dan cakupan userspace terdokumentasi secara jujur).*
