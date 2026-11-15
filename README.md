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
