# Xynoos HyperCore

Fondasi riset userspace untuk optimasi sumber daya Linux pada perangkat terbatas. Milestone TASK 002 menyediakan simulasi penyimpanan halaman terkompresi dan kebijakan AURA baseline; ini bukan pengganti memory manager, swap, atau zram dan belum siap produksi.

## Menjalankan

Rust stable dan Cargo diperlukan. `cargo build --workspace`, `cargo test --workspace`, `cargo run -p xynoos-cli`, dan `cargo run -p xynoos-bench`. Quality gate: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`.

## Arsitektur

- `xynoos-core`: ID, tekanan, tier, dan estimasi biaya bersama.
- `xynoos-hypermemory`: HashMap halaman LZ4, kapasitas dalam jumlah halaman, pengambilan dan penghapusan. Bergantung pada core dan `lz4_flex` dengan implementasi safe.
- `xynoos-aura`: kontrak dan baseline deterministik, bergantung pada core.
- `xynoos-cli` dan `xynoos-bench`: demonstrasi dan pengukuran, bergantung pada hypermemory.

Halaman harus tepat sebesar ukuran yang dikonfigurasi (default 4096 byte). Rasio adalah byte payload tersimpan dibagi byte asli; semakin kecil semakin baik. Statistik melaporkan kapasitas buffer terkompresi dan batas bawah metadata per entri; overhead HashMap, allocator, struktur store, dan memori sementara tidak dihitung. Kompresi tidak menjamin ukuran mengecil. CRC32 IEEE pada payload dan data asli membantu mendeteksi korupsi; checksum bukan autentikasi dan dapat mengalami collision. Ukuran halaman dibatasi 1 MiB. `PageStore::with_capacity` kini mengembalikan `Result`; gunakan `?` untuk menangani konfigurasi tidak valid.

Tidak ada akses memori proses lain, swap, device, atau kernel. Alokasi gagal dapat menyebabkan abort; kapasitas halaman bukan batas memori byte. AURA belum mengukur tekanan atau biaya nyata. HyperTier, RefaultSense, PageFusion dan integrasi kernel masih konsep. Langkah berikutnya: fuzzing terarah, isolasi pengukuran allocator, dan kebijakan berbasis pengukuran. Lihat [arsitektur](docs/ARCHITECTURE.md) dan [laporan](docs/reports/TASK-001-FOUNDATION-REPORT.md). Lisensi: GPL-3.0, lihat LICENSE.


Benchmark TASK 002: `cargo build --workspace --release`, kemudian `target/release/xynoos-bench`. Lima dataset × raw/HyperMemory dijalankan dalam subprocess terpisah, masing-masing 4 warm-up dan 32 pengulangan 256 halaman 4 KiB. Median/p95 adalah distribusi rata-rata waktu batch per operasi, bukan percentile latency individual. Throughput codec diukur terpisah dari store dan checksum. RSS/VmHWM serta CPU scheduler adalah pengukuran proses keseluruhan. [Data CSV](docs/benchmarks/task-002.csv) dan [laporan TASK 002](docs/reports/TASK-002-MEMORY-CORRECTNESS-BENCHMARK-REPORT.md) memuat hasil dan batas interpretasinya.

Percobaan TASK 003 menambahkan `PageStore::adaptive(page_size, capacity, budget)`. Budget menghitung kapasitas Vec dan ukuran statis pasangan ID/Entry aktif, bukan RSS atau puncak alokasi; map, allocator dan buffer sementara tidak termasuk. Mode lama tetap LZ4. Statistik `stored_bytes` mencakup raw dan LZ4; `compression_ratio` kini menggunakan total tersebut. Pemilihan menggunakan kapasitas aktual kedua Vec ketika diperlukan; metadata entry sama untuk kedua representasi. Lihat [laporan TASK 003](docs/reports/TASK-003-ADAPTIVE-STORAGE-REPORT.md).

Benchmark TASK 003: `cargo build --workspace --release`, `target/release/xynoos-bench > docs/benchmarks/task-003-raw.csv`, `python3 scripts/summarize_task003.py`. CSV mentah memuat rata-rata per halaman pada tiap batch; throughput codec diukur terpisah, bukan throughput end-to-end.

TASK 004 menambahkan modul independen `xynoos_aura::placement`: model biaya integer, tiga tier virtual, LRU sederhana, compression-first, serta AURA berbasis biaya dengan hysteresis, cooldown dan budget migrasi. Jalankan `cargo run -p xynoos-aura --example research --release > docs/benchmarks/task-004-raw.csv`, kemudian `python3 scripts/summarize_task004.py`. Model memakai biaya sintetis dan tidak memindahkan halaman kernel atau mengakses backing device. [Laporan TASK 004](docs/reports/TASK-004-AURA-ALGORITHM-RESEARCH-REPORT.md) memisahkan hasil simulasi dari waktu eksekusi algoritma. AURA tidak selalu unggul; entropy dan prediksi tidak andal menunjukkan risiko stall.

TASK 005 menambahkan modul `xynoos_aura::physical`: RAM raw dan compressed berbagi budget simulasi dengan metadata terpisah. `event_candidate` menerima kandidat yang sama antar kebijakan. Hasil TASK 004 dipertahankan. Jalankan `cargo run -p xynoos-aura --example validation --release > docs/benchmarks/task-005-raw.csv`, lalu `python3 scripts/summarize_task005.py`. Lihat [laporan TASK 005](docs/reports/TASK-005-AURA-MODEL-VALIDATION-REPORT.md).

Eksperimen awal TASK006 dengan mask eksogen diarsipkan pada [laporan awal](docs/reports/TASK-006-PRELIMINARY-REPORT.md); hasil tersebut memiliki confound eligibility dan terpisah dari eksperimen completion.

TASK 006 completion berstatus **SELESAI**. Preview selector memakai event commit yang sama; journal dua grup lulus differential test tetapi clone tetap default. Eksperimen final: `cargo run -p xynoos-aura --example completion --release > docs/benchmarks/task-006-final-raw.csv`, lalu `python3 scripts/summarize_task006.py`. Hasil awal tetap tersimpan; lihat laporan TASK006 yang telah direvisi.

TASK007 menambahkan FastPath eksperimental O(n) yang mempertahankan oracle/default TASK006. Jalankan `cargo build -p xynoos-aura --examples --release` dan `python3 scripts/benchmark_task007.py`. [Laporan TASK007](docs/reports/TASK-007-AURA-FASTPATH-RESEARCH-REPORT.md) mencakup differential testing, data timing lokal dan batas validitas.

TASK008 menambahkan pengujian skala 1–64 grup dan profiling alokasi melalui executable benchmark terpisah. Jalankan `cargo build -p xynoos-aura --examples --release` lalu `python3 scripts/benchmark_task008.py`. Default tetap oracle. Lihat [laporan TASK008](docs/reports/TASK-008-AURA-SCALABILITY-RESEARCH-REPORT.md).

TASK009 menambahkan register heap scope pada executable `reliability`, fault hook private, dan validasi rollback error/unwind sebelum publikasi state. Jalankan `cargo build -p xynoos-aura --examples --release` lalu `python3 scripts/benchmark_task009.py`. Tidak mengklaim OOM recovery; oracle tetap default. Lihat [laporan TASK009](docs/reports/TASK-009-AURA-MEMORY-RELIABILITY-REPORT.md).

TASK010 mengganti prepared Vec FastPath dengan reservasi fallible dan error `AllocationReservation`. Oracle tetap default. Jalankan `cargo build -p xynoos-aura --examples --release`, `target/release/examples/tracker_validation` dan `python3 scripts/benchmark_task010.py`. Lihat [laporan TASK010](docs/reports/TASK-010-FALLIBLE-MEMORY-SAFETY-REPORT.md) untuk batas OOM/unwind dan atribusi heap global.

TASK011 menambahkan type-state internal Prepared → CommitReady untuk FastPath; oracle tetap default. [Laporan TASK011](docs/reports/TASK-011-TRANSACTION-SAFETY-RESEARCH-REPORT.md) memuat audit unsafe, batas journal dan hasil benchmark. Reproduksi: `cargo build -p xynoos-aura --examples --release` lalu `python3 scripts/benchmark_task011.py`.

TASK012 menambahkan guard rollback journal pada `Err` dan unwind yang diuji; default tetap clone. Eksperimen regresi menggunakan baseline TASK010 dalam subprocess ABBA: `python3 scripts/benchmark_task012.py` setelah build examples release. [Laporan TASK012](docs/reports/TASK-012-FASTPATH-RELIABILITY-REPORT.md) menjelaskan keterbatasan dan self-audit allocator.
