## 1. HALAMAN IDENTITAS

Xynoos HyperCore — TASK 008: AURA FastPath Scalability. Tanggal 15 November 2026. Milestone 0.1.0 riset userspace. Basis source main 03e4872d52968732eff7a5772c18cce975f2368a. Status teknis SELESAI; status merge diverifikasi terpisah melalui GitHub.

## 2. ABSTRAK

Eksperimen memperluas pengujian FastPath ke enam ukuran grup, membandingkan bounded clone, journal dan FastPath serta mengukur allocator System pada hot path. Empat puluh tiga unit test dan enam quality gate lulus. Pada 64 grup median batch 128 event adalah 3.818.003,5 ns clone dan 1.226.812 ns FastPath; masing-masing 16.512 dan 128 alokasi teramati. Peak live heap spesifik hot path belum terukur. Ini pengamatan userspace sintetis, bukan reclaim kernel.

## 3. PENDAHULUAN

Penelitian menilai skala dan alokasi agar keputusan adopsi FastPath tidak semata berdasarkan benchmark 16 grup. Ruang lingkup ialah simulator, bukan modifikasi Linux atau swap.

## 4. LANDASAN TEKNIS

Allocator menerima Layout yang menyatakan ukuran yang diminta, bukan overhead aktual allocator. Instrumen atomik menambah biaya pada alokasi; dealloc pada jendela dapat berasal dari objek yang dibuat sebelum pengukuran sehingga tidak cukup untuk menghitung peak live yang valid. Median/p95 dihitung pada batch 128 event, bukan percentile latency individual.

## 5. AUDIT TASK 007

Git main lokal dan remote diverifikasi pada 03e4872. File ss tidak terlacak tetap dipertahankan. Source physical.rs, selector.rs, placement.rs, benchmark dan laporan TASK007 diperiksa. Oracle mengulang akses dan clone Vec untuk setiap preview; journal hanya mengurangi clone commit; FastPath menyiapkan akses sekali. Source antara step dan prepare/complete diduplikasi untuk oracle independen, menimbulkan risiko drift.

## 6. ANALISIS BOTTLENECK

Analisis source memperkirakan per event clone=2n+1 Vec clones dan access replay=n+1; journal=2n Vec clones dan n+1 access; FastPath=1 Vec clone dan satu access. Ini hitungan alur, bukan counter instrumentasi clone tersendiri. Allocator menunjukkan jumlah dan byte aktual permintaan hot path pada eksekusi ini; overhead clone, journal dan timer termasuk.

## 7. METODOLOGI SCALABILITY

Ukuran1,4,8,16,32,64. Tiap fixture dibangun ulang lewat Simulator::new; ukuran raw4096/grup, metadata80/grup, RAM awal n×4096, budget fisik RAM+metadata, kapasitas compressed n×4096 dan backing n×8192. Seed1,42,2026 menghasilkan trace deterministik 128 event, tekanan690/800. Policy Aura dan urutan indeks identik antar mode. Satu warm-up dan tiga sample tiap kombinasi; urutan mode dirotasi menurut ukuran+seed. Dua subprocess terpisah menghasilkan 324 record. Median dan p95 pada18 batch per ukuran/mode; p95 adalah nearest-rank batch, bukan p95 event individual. Fixture, trace, Vec order dan CSV di luar window waktu/alokasi.

## 8. PROFILING ALOKASI HEAP

GlobalAlloc wrapper khusus executable scalability meneruskan alloc/dealloc/realloc ke System, tanpa rekursi, dengan atomic counters dan enabled flag. Unsafe hanya pada wrapper benchmark dan delegasi allocator; library tidak mengubah forbid unsafe. Program single-thread; atomic relaxed tidak mengisolasi thread lain dan asumsi ini penting. Self-check Box 17 byte memverifikasi data, alloc, byte, dealloc terhadap System. Realloc sukses dihitung sebagai satu allocation-request dan satu deallocation-event, byte=ukuran baru; ini bukan jumlah panggilan kernel. Instrumen memakai atomik dalam hot path, memengaruhi waktu. Gagal alloc tidak dihitung. Instrumen tidak memetakan identitas alokasi dan window dimulai setelah fixture, sehingga peak live hot-path tidak dapat direkonstruksi dengan benar; ditulis tidak diukur, bukan diklaim nol. RSS maksimum subprocess keseluruhan melalui getrusage adalah 14592 KiB, bukan isolated heap.

## 9. PERANCANGAN DAN IMPLEMENTASI

examples/scalability.rs mengukur tiga mode dengan fixture valid, gate atomik dan warm-up. scripts/benchmark_task008.py menjalankan dua subprocess, memastikan 324 record dan unique key, membandingkan seluruh metrik simulasi antar mode dan antar proses, serta membuat ringkasan. Test six_sizes_budget_and_fallback membandingkan full state/Error pada enam ukuran dan kondisi budget nol, confidence rendah dan scratch nol.

## 10. ANALISIS KOMPLEKSITAS

Oracle mempreview n kandidat yang masing-masing clone metadata O(n), sehingga O(n²); journal tetap O(n²) pada preview. FastPath melakukan satu clone O(n), satu akses dan n inspeksi O(1), sehingga O(n). Memori peak proses tidak diisolasi. Instrumen menunjukkan jumlah alokasi per batch pada ukuran64: 16512 clone, 16384 journal dan128 FastPath; ini satu pengamatan allocator, bukan bukti universal.

## 11. HASIL UNIT TEST

43 test workspace lulus, termasuk 42 test terdahulu. Satu test baru menjalankan 6 ukuran ×4 konfigurasi ×128 event =3072 perbandingan dengan oracle. Kondisi pressure adversarial690/800, budget CPU/I/O nol, confidence fallback dan scratch nol diuji; PartialEq memeriksa seluruh state dan hasil. Test lama tetap mencakup invalidindex, invalidpermutation, overflow, rollback, journal-vs-clone dan scratch.

## 12. HASIL DIFFERENTIAL TEST

Test baru membandingkan hasil, state dan cursor pada setiap event. Runner membandingkan refault, migrasi, stall, CPU/I/O, occupancy, peak modeled dan evaluasi antar tiga mode pada setiap sample; seluruh mode dan dua subprocess identik untuk metrik non-timing/non-allocator. Verifikasi ini bounded, belum membuktikan equivalence pada semua kemungkinan state.

## 13. HASIL QUALITY GATES

cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace, cargo check --workspace, cargo build --workspace --release, cargo build -p xynoos-aura --examples --release seluruhnya lulus. Log lengkap task-008-quality-gates.txt. Kompilasi awal sempat gagal akibat jumlah placeholder format; akar kesalahan diperbaiki sebelum mengulang seluruh gate.

## 14. HASIL BENCHMARK

| Grup | Mode | Median batch ns | p95 batch ns | Inspeksi median ns | Alloc count | Alloc byte | Dealloc count |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | clone | 38767.0 | 56746 | 22266.0 | 384.0 | 30720.0 | 384.0 |
| 1 | journal | 35366.0 | 63739 | 22125.0 | 256.0 | 20480.0 | 256.0 |
| 1 | fast | 32986.5 | 33562 | 14592.5 | 128.0 | 10240.0 | 128.0 |
| 4 | clone | 114438.0 | 167392 | 93366.0 | 1152.0 | 368640.0 | 1152.0 |
| 4 | journal | 109764.5 | 167703 | 92972.0 | 1024.0 | 327680.0 | 1024.0 |
| 4 | fast | 86205.5 | 109724 | 63208.0 | 128.0 | 40960.0 | 128.0 |
| 8 | clone | 223622.5 | 380309 | 196517.5 | 2176.0 | 1392640.0 | 2176.0 |
| 8 | journal | 216598.5 | 264393 | 194212.5 | 2048.0 | 1310720.0 | 2048.0 |
| 8 | fast | 146873.5 | 163445 | 117887.0 | 128.0 | 81920.0 | 128.0 |
| 16 | clone | 564843.0 | 956945 | 533071.0 | 4224.0 | 5406720.0 | 4224.0 |
| 16 | journal | 559979.0 | 975108 | 533687.5 | 4096.0 | 5242880.0 | 4096.0 |
| 16 | fast | 283668.5 | 321820 | 249545.0 | 128.0 | 163840.0 | 128.0 |
| 32 | clone | 1345598.5 | 2087923 | 1309320.0 | 8320.0 | 21299200.0 | 8320.0 |
| 32 | journal | 1354374.5 | 1821807 | 1331325.5 | 8192.0 | 20971520.0 | 8192.0 |
| 32 | fast | 580462.5 | 849674 | 541038.5 | 128.0 | 327680.0 | 128.0 |
| 64 | clone | 3818003.5 | 6595948 | 3770825.5 | 16512.0 | 84541440.0 | 16512.0 |
| 64 | journal | 3552789.5 | 6395565 | 3529936.0 | 16384.0 | 83886080.0 | 16384.0 |
| 64 | fast | 1226812.0 | 1950657 | 1172966.0 | 128.0 | 655360.0 | 128.0 |

Kolom event dan inspeksi dalam ns per batch128. Raw CSV memuat seed, process, sample, evaluasi, hitungan replay/clone estimasi, refault, migrasi, stall, CPU/I/O, okupansi dan peak model. Perbandingan berlaku pada mode yang state-nya ekuivalen.

## 15. HASIL PENGUKURAN ALOKASI

Pada64 grup median requested bytes per batch: clone84541440, journal83886080, fast655360; allocation count masing-masing16512,16384,128. Ini allocation traffic kumulatif bukan memori resident atau peak; deallocation-event dapat mencakup objek pra-window. Peak live heap belum tersedia; 14592KiB adalah peak RSS agregat subprocess, termasuk inisialisasi dan semua mode.

## 16. EVALUASI DEFAULT PATH

Default oracle dipertahankan. Equivalence terbatas dan benchmark mendukung eksperimen lebih lanjut, namun oracle dan FastPath masih menduplikasi fase, belum ada injection OOM/panic-unwind, sampel hanya satu jenis trace, dan tidak ada validasi allocator pada lingkungan multi-thread. Tidak cukup alasan untuk mengubah kontrak/default produksi.

## 17. ANALISIS REGRESI

Semua test lama dipertahankan. Tidak ada perubahan kode placement atau kebijakan utama; perubahan selector.rs hanya menambah test. Benchmark memakai allocator wrapper sehingga timing tidak langsung dibandingkan TASK007 yang tidak terinstrumentasi. P95 batch menunjukkan variabilitas; tidak ada interval kepercayaan dan dua subprocess tidak mewakili populasi perangkat.

## 18. ANALISIS KEAMANAN

Unsafe allocator hanya di executable benchmark; safety invariant adalah meneruskan pointer/layout identik ke System dan tidak mengalokasikan dari counter. Operasi simulator userspace tidak menyentuh kernel, device atau swap. Alokasi OOM dapat abort; wrapper bukan allocator produksi.

## 19. KETERBATASAN DAN ANCAMAN VALIDITAS

Tidak tersedia peak live heap terisolasi tanpa atribusi identitas allocation yang benar. Dua subprocess, satu workload sintetis dan instrumentasi atomik membatasi generalisasi. Baseline RSS termasuk fixture dan runtime. Tidak diukur hardware CPU-time atau biaya reclaim kernel. Access replay dan Vec clone adalah estimasi source, bukan telemetry yang diinstrumentasi secara terpisah.

## 20. KESIMPULAN

Enam ukuran, profiling allocation traffic, differential testing dan quality gates selesai. Skala data konsisten dengan hipotesis O(n²) dan O(n), namun bukan bukti performa produksi. Default tetap oracle.

## 21. REKOMENDASI TASK 009

Tambahkan allocator profiling yang melacak identitas objek dan peak live terisolasi, throughput pada workload beragam, uji OOM dan mekanisme type-state untuk mengurangi drift oracle. Evaluasi default hanya setelah hasil tersebut dan review safety. Model AI yang sesuai adalah model coding Rust yang mampu meninjau unsafe dan differential tests.

## 22. DAFTAR PUSTAKA

Dokumentasi Rust GlobalAlloc: https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html ; Rust System: https://doc.rust-lang.org/std/alloc/struct.System.html ; laporan TASK007 dan source simulator dalam repository.

## 23. LAMPIRAN DAN BUKTI

Artefak: task-008-raw.csv, summary.csv, quality-gates.txt, reproducibility.txt dan memory.txt pada docs/benchmarks. Reproduksi: cargo build -p xynoos-aura --examples --release; python3 scripts/benchmark_task008.py. Perubahan: scalability.rs, selector.rs (test), benchmark_task008.py, README, ARCHITECTURE, laporan dan artefak. Storage awal181 MiB, akhir190 MiB; ruang tersedia17814593536 byte saat diukur. File ss tidak masuk perubahan. Tidak ada dependency baru.
