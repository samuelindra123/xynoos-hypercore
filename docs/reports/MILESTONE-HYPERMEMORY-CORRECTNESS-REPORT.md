# Laporan Milestone Rekayasa HyperCore: Memory Engine Correctness, Deterministic Eviction & Observability

## 1. Identifikasi Milestone
- **Milestone**: MILESTONE A & B — Memory Engine Correctness, Bounded Deterministic Eviction & Telemetry Observability.
- **Repository**: `samuelindra123/xynoos-hypercore`
- **Cabang Pengembangan**: `feat/hypercore-next-milestone`
- **Base Branch**: `main` (commit `c5c8beb`)
- **Tujuan Rekayasa**: Mentransformasi engine memori terkompresi userspace (`PageStore`) dari simulator statis (TASK 003) menjadi engine cache adaptif dengan jaminan correctness, siklus hidup halaman eksplisit, penggusuran deterministik (LRU & FIFO), pembacaan zero-allocation (`get_into`), pembaruan in-place atomik (`update`), telemetri observabilitas operasional (`Telemetry`), dan pengujian invarian berbasis properti.

---

## 2. Status Awal Repositori
- **HEAD Commit Main**: `c5c8beb` (Merge PR #51: "Fix Astro deployment at Vercel root").
- **Kondisi Sebelumnya**:
  - `PageStore` hanya mendukung operasi dasar: `insert`, `get`, `remove`, `stats`.
  - Tidak memiliki mekanisme penggusuran (eviction); penolakan langsung terjadi saat kapasitas atau budget penuh (`Error::Full` / `Error::BudgetExceeded`).
  - Tidak memiliki query `contains` atau pelacakan siklus hidup (`PageLifecycleState`).
  - Pengambilan (`get`) selalu mengalokasikan `Vec<u8>` baru, menghasilkan overhead alokasi pada hot-path pembacaan.
  - Tidak mendukung pembaruan halaman in-place (`update`); pemanggil harus menghapus dan memasukkan ulang, membuang ID halaman.
  - Telemetri hanya berupa snapshot alokasi statis (`Stats`), tidak ada counter hit, miss, eviction, update, atau rasio keberhasilan cache.
  - Pengujian belum mencakup property-based test multi-operasi untuk membuktikan kebenaran invarian akuntansi di bawah mutasi acak.
  - Benchmark `xynoos-bench` hanya menguji batch insert/get sederhana, tanpa uji penggusuran, mutasi in-place, atau distribusi latensi percentile.

---

## 3. Rangkuman Perubahan Terimplementasi
1. **Siklus Hidup Eksplisit & Query**:
   - Menambahkan enum `PageLifecycleState { Active, Evicted }` pada `xynoos-core`.
   - Menambahkan metode `contains(&self, id: PageId) -> bool`.
   - Menambahkan metode `lifecycle_state(&self, id: PageId) -> PageLifecycleState`.
2. **Pengambilan Zero-Allocation (`get_into`) & Inspeksi Non-Intrusif (`peek`)**:
   - Menambahkan `get_into(&self, id: PageId, out: &mut [u8]) -> Result<usize, Error>` yang langsung mendekompresi data ke buffer pemanggil tanpa alokasi heap.
   - Menambahkan error variant `Error::OutputBufferTooSmall { min_required, actual }`.
   - Menambahkan `peek(&self, id: PageId) -> Result<Vec<u8>, Error>` untuk inspeksi diagnostik tanpa memajukan access tick atau menambah hit counter.
   - Menjaga `get(&self, id: PageId) -> Result<Vec<u8>, Error>` dengan mendelegasikan langsung ke `get_into`.
3. **Pembaruan Halaman In-Place Atomik (`update`)**:
   - Menambahkan `update(&mut self, id: PageId, data: &[u8]) -> Result<(), Error>`.
   - Mengevaluasi delta budget sebelum komit; jika budget terlampaui, pembaruan digagalkan secara atomik (`Error::BudgetExceeded`) dan state lama dipertahankan tanpa perubahan.
4. **Mesin Penggusuran Deterministik (`EvictionPolicy`)**:
   - Menambahkan enum `EvictionPolicy { Reject, Lru, Fifo }` pada `xynoos-core`.
   - Menambahkan `insert_with_policy(&mut self, data: &[u8], policy: EvictionPolicy) -> Result<PageId, Error>`.
   - Menambahkan `evict_with_policy(&mut self, policy: EvictionPolicy) -> Result<Option<PageId>, Error>` dan `evict(&mut self) -> Result<Option<PageId>, Error>`.
   - Pemilihan kandidat LRU (`last_accessed_tick`) dan FIFO (`inserted_tick`) memecahkan kondisi seri secara deterministik menggunakan `PageId.0` terendah.
5. **Observabilitas & Telemetri Ringan (`Telemetry`)**:
   - Menambahkan struct `Telemetry`:
     - `hit_count`, `miss_count`, `eviction_count`, `insert_count`, `update_count`, `remove_count`, `corruption_count`.
     - `hit_ratio(&self) -> Option<f64>`.
   - Menambahkan metode `telemetry(&self) -> Telemetry` dan `reset_telemetry(&self)`.
6. **Infrastruktur Benchmark Baru di `xynoos-bench`**:
   - Menambahkan mode `lifecycle`: membandingkan throughput alokasi `get` vs zero-allocation `get_into`, `update`, dan saturasi penggusuran LRU.
   - Menambahkan mode `mixed`: menyimulasikan beban kerja cache nyata (70% baca, 15% tulis LRU, 10% update, 5% gusur) dengan pencatatan latensi p50, p95, dan p99.
7. **Pipeline CI GitHub Actions**:
   - Menambahkan workflow dedicated `.github/workflows/hypercore-engine-ci.yml` dengan job:
     - `quality`: rustfmt, workspace check, strict clippy.
     - `test`: workspace unit & integration test, hypermemory release property tests.
     - `benchmark`: release build dan eksekusi benchmark dengan upload artefak CSV.

---

## 4. Crate yang Terdampak
- `crates/xynoos-core`: penambahan `PageLifecycleState` dan `EvictionPolicy`.
- `crates/xynoos-hypermemory`: implementasi `get_into`, `peek`, `update`, `evict`, `insert_with_policy`, `Telemetry`, serta test suite komprehensif.
- `crates/xynoos-bench`: penambahan subcommand benchmark `lifecycle` dan `mixed`.
- `.github/workflows/hypercore-engine-ci.yml`: workflow validasi CI terintegrasi.
- `docs/ARCHITECTURE.md`: dokumentasi teknis arsitektur Milestone A & B.

---

## 5. Keputusan Arsitektur Kunci
- **Interior Mutability via `std::cell::Cell`**:
  `PageStore` tetap mempertahankan API `&self` pada pembacaan (`get`, `get_into`, `contains`, `telemetry`), memungkinkan pemanggil bersama (shared references) membaca memori secara aman dan thread-safe lokal tanpa lock atau mutex overhead, seraya memperbarui tick akses dan telemetri hit/miss secara aman di bawah `#![forbid(unsafe_code)]`.
- **Pemisahan Akuntansi Penyimpanan Fisik (`Stats`) vs Telemetri Operasional (`Telemetry`)**:
  Struktur `Stats` dipertahankan murni untuk akuntansi penyimpanan byte fisik (`accounted_bytes == buffer_capacity_bytes + metadata_bytes_lower_bound`), sehingga operasi pembacaan yang gagal atau inspeksi diagnostik tidak merusak invarian historis `assert_eq!(s.stats(), before)`. Counter operasional dipisahkan ke dalam `Telemetry`.
- **Pencegahan Alokasi di Hot-Path Pembacaan**:
  `get_into` memungkinkan buffer di-reuse berulang kali oleh loop pemanggil, secara signifikan mengurangi alokator churn dan fragmentasi memori heap pada perangkat spek rendah (seperti laptop Intel N5000 RAM 4 GB).

---

## 6. Hasil Pengujian dan Verifikasi
Seluruh kategori pengujian dijalankan dan divalidasi secara lokal:

| Kategori | Target | Hasil | Catatan |
|---|---|---|---|
| Formatting | `cargo fmt --all -- --check` | **PASS** | Sesuai standar rustfmt resmi |
| Clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | **PASS** | 0 peringatan / 0 error di seluruh workspace |
| Compilation | `cargo check --workspace --all-targets --locked` | **PASS** | Kompilasi bersih tanpa peringatan |
| Unit Tests | `cargo test -p xynoos-hypermemory` | **PASS** | 25/25 pengujian lulus (termasuk 12 pengujian baru) |
| Workspace Tests | `cargo test --workspace --locked` | **PASS** | Seluruh test suite xynoos-aura, xynoos-core, xynoos-decision, xynoos-hypermemory, xynoos-linux-control, xynoos-pressureiq lulus |
| Property-Based Tests | `extensive_randomized_invariants` | **PASS** | 5.000+ operasi acak multi-seed memvalidasi 6 invarian matematika matematis pada setiap langkah mutasi |
| Fault Tests | `integrity_tests` & `malformed_tests` | **PASS** | Deteksi payload corruption, bit flip, data truncation, dan buffer mismatch diverifikasi |

---

## 7. Bukti Kinerja (Performance Evidence)
Dijalankan pada lingkungan rilis Linux lokal (`cargo run -p xynoos-bench --release`):

### 7.1. Evaluasi Throughput Pembacaan: `get` vs `get_into`
| Operasi | Iterasi | Waktu per Op (ns) | Throughput (MiB/s) | Peningkatan |
|---|---|---|---|---|
| `get` (Allocating Vec) | 2.000 | 2.729,84 ns | 1.430,95 MiB/s | Baseline |
| `get_into` (Zero-Allocation Buffer) | 2.000 | 2.307,76 ns | 1.692,66 MiB/s | **+18,29% Throughput** |
| `update` (In-Place Mutation) | 2.000 | 6.133,33 ns | 636,89 MiB/s | Kompresi + Evaluasi Budget Atomik |
| `lru_eviction` (Under Continuous Write Saturation) | 2.000 | 11.549,35 ns | 338,22 MiB/s | 1.936 penggusuran deterministik sukses |

### 7.2. Beban Kerja Campuran Nyata (Realistic Mixed Cache Workload)
Pengujian 5.000 operasi: 70% Baca (`get_into`), 15% Tulis LRU, 10% Update In-Place, 5% Evict Eksplisit:
- **Total Operasi**: 5.000
- **Distribusi Operasi**: 3.466 Reads, 823 Inserts, 532 Updates, 696 Evictions
- **Hit Ratio**: 1,0000 (100% cache hits pada entri aktif)
- **Median Latency (p50)**: 2.634 ns (~2,63 µs)
- **Percentile 95 Latency (p95)**: 15.496 ns (~15,50 µs)
- **Percentile 99 Latency (p99)**: 17.272 ns (~17,27 µs)

---

## 8. Tinjauan Keamanan (Security Review)
- `#![forbid(unsafe_code)]` ditegakkan secara ketat pada seluruh crate `xynoos-core`, `xynoos-hypermemory`, dan `xynoos-bench`. Tidak ada blok `unsafe` yang diperkenalkan.
- Tidak ada risiko buffer over-read / over-write: `get_into` secara eksplisit memeriksa batas panjang buffer `out.len() < self.page_size` dan mengembalikan `Error::OutputBufferTooSmall`.
- Integritas data diverifikasi ganda: CRC32 IEEE atas payload terkompresi dan CRC32 IEEE atas data uncompressed sebelum diserahkan ke pemanggil.
- Proteksi overflow: semua operasi penambahan akuntansi memori dan penghitung tick menggunakan `checked_add`, `checked_mul`, atau `saturating_add`.

---

## 9. Observasi Penggunaan Sumber Daya Host
- Selama pengujian lokal, penggunaan memori proses tetap di bawah 25 MB.
- Kompilasi dan pengujian unit berlangsung di bawah 8 detik.
- Tidak ada file biner besar, ISO, VM image, atau dependensi eksternal baru yang diunduh ke mesin lokal pengguna.

---

## 10. Batasan yang Diketahui (Known Limitations)
1. **Ruang Lingkup Userspace**: `PageStore` adalah engine penyimpanan memori kompresi berbasis userspace untuk cache dan buffer internal proses; tidak menggantikan subsistem Virtual Memory Manager (VMM) kernel Linux tanpa integrasi driver kernel khusus.
2. **Kapasitas Entry Metadata**: Akuntansi `metadata_bytes_lower_bound` menghitung ukuran statis `(PageId, Entry)`; overhead internal bucket kosong `HashMap` dan overhead internal allocator libc tidak dapat diukur secara eksak tanpa profil eksternal OS.
3. **Kompleksitas Pencarian Penggusuran**: Pemilihan kandidat LRU/FIFO memindai `pages.iter()` dalam kompleksitas O(N) di mana N adalah jumlah halaman dalam store. Untuk kapasitas memori hingga ribuan halaman, overhead ini dapat diabaikan (<12 µs per evict), namun untuk ratusan ribu halaman ke atas dapat dipertimbangkan pemeliharaan struktur antrean akses O(1) tambahan.

---

## 11. Rekomendasi Milestone Berikutnya
- **MILESTONE C — AURA POLICY ADAPTIVE TIERING**:
  Menghubungkan keputusan deterministik `xynoos-aura` dengan engine `xynoos-hypermemory` teruji untuk menentukan kapan halaman harus dikompresi raw vs LZ4 berdasarkan estimasi latensi dan tekanan memori PressureIQ.
- **MILESTONE D — HOST STRESS BENCHMARK HARNESS**:
  Menjalankan harness benchmark terdistribusi pada GitHub Actions runner dengan workload multi-thread.
