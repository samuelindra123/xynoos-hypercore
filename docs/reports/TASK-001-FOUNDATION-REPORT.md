# HALAMAN IDENTITAS

**Judul:** Fondasi simulasi penyimpanan halaman dan kebijakan reclaim  
**Proyek:** Xynoos HyperCore  
**Task:** 001 — Foundation  
**Tanggal:** 15 November 2026  
**Milestone:** 0.1.0  
**Status:** SELESAI

## ABSTRAK

Dibangun workspace Rust userspace dengan penyimpanan halaman LZ4, kebijakan AURA deterministik, demonstrasi dan harness pengukuran. Pengujian round-trip dan validasi lulus; pengukuran awal hanya berlaku untuk halaman nol dan tidak membuktikan peningkatan kinerja sistem.

## 1. PENDAHULUAN

Perangkat terbatas memerlukan evaluasi efisiensi sebelum optimasi. Masalah yang dikaji ialah representasi halaman terkompresi dan keputusan tier yang dapat diuji. Tujuannya menghasilkan fondasi modular, bukan reclaim kernel. Ruang lingkup terbatas pada memori proses sendiri; tidak ada swap, I/O device, maupun integrasi page fault.

## 2. LANDASAN TEKNIS

Rust menyediakan ownership dan pemeriksaan tipe untuk mengurangi kesalahan memori. Kompresi lossless LZ4 mengubah representasi byte dan dekompresi mengembalikannya. Page storage dalam eksperimen ini adalah peta ID ke blok terkompresi; tiering adalah pemilihan representasi penyimpanan. Kernel mengelola page fault, reclaim dan sinkronisasi sistem secara nyata; simulator ini tidak melakukannya.

## 3. ANALISIS DAN PERANCANGAN

Core menyediakan PageId, MemoryPressure, StorageTier dan CostEstimate. HyperMemory memakai HashMap dan Vec serta bergantung pada core dan `lz4_flex` 0.11.7; LZ4 dipilih sebagai codec lossless dengan implementasi Rust safe. AURA bergantung pada core dan mendefinisikan trait DecisionPolicy. CLI dan benchmark bergantung pada HyperMemory. Kapasitas dibatasi jumlah halaman, ID monoton, dan error eksplisit mencakup ukuran, kapasitas, ID dan dekompresi. Metadata adalah batas bawah ukuran pasangan key dan Vec, bukan total overhead allocator atau HashMap.

## 4. IMPLEMENTASI

`crates/xynoos-hypermemory/src/lib.rs` menyediakan PageStore, insert, get, remove dan stats. Tidak ada salinan asli permanen. `xynoos-aura/src/lib.rs` menyediakan baseline deterministik; biaya belum dipakai. Core menyediakan tipe bersama. CLI melakukan demonstrasi dan bench mengukur insert serta get. Kode proyek melarang unsafe pada library.

## 5. METODOLOGI PENGUJIAN

Lingkungan Codespaces Linux, rustc 1.100.0; pengujian menggunakan halaman nol, pola periodik dan data pseudoacak deterministik 4 KiB, ukuran tidak valid, kapasitas, ID hilang, penghapusan, statistik dan determinisme. Perintah tercantum pada lampiran. Harness memakai Instant, 1000 halaman nol dan build debug; waktu insert mencakup kompresi dan operasi penyimpanan, waktu get mencakup dekompresi dan verifikasi.

## 6. HASIL DAN PEMBAHASAN

Keempat quality gate lulus: fmt, Clippy tanpa warning, test (3 test lulus), dan check. Satu pengukuran debug menghasilkan insert 35,307213 ms dan get dengan verifikasi 1,014932 ms; data asli 4.096.000 byte, data terkompresi dan kapasitas buffer 27.000 byte, metadata batas bawah 32.000 byte, rasio 0,00659. Ini observasi tunggal, bukan perbandingan zram atau ukuran RSS. Penyimpanan workspace sekitar 59 MiB dan ruang tersedia 17 GiB pada pengukuran; target build dapat berada di luar workspace.

## 7. ANALISIS KEAMANAN DAN KETERBATASAN

Library proyek melarang unsafe; dependency menggunakan fitur safe. Tidak ada checksum atau autentikasi data, sehingga korupsi tidak selalu terdeteksi. Alokasi dapat gagal secara fatal; kapasitas halaman tidak membatasi byte aktual. HashMap dan allocator tidak termasuk dalam batas bawah metadata. Benchmark satu dataset, satu pengulangan dan debug tidak representatif; waktu mencakup operasi tambahan. Kernel, concurrency, persistensi dan tekanan aktual belum diintegrasikan.

## 8. KESIMPULAN

Fondasi workspace, penyimpanan simulasi, baseline dan quality gate selesai. Hasil tidak membuktikan manfaat kinerja produksi.

## 9. REKOMENDASI PENGEMBANGAN

Tambahkan pengujian properti dan korupsi, pengukuran alokasi/RSS, dataset beragam dan pengulangan statistik, serta kebijakan berbasis biaya terukur sebelum mengevaluasi integrasi lebih lanjut.

## 10. DAFTAR PUSTAKA

- Dokumentasi Rust: https://doc.rust-lang.org/book/ ; konsep ownership.
- `lz4_flex`: https://docs.rs/lz4_flex/0.11.7/lz4_flex/ ; API codec.

## LAMPIRAN

Struktur: Cargo.toml dan Cargo.lock; lima crate pada `crates/`; README dan dokumen arsitektur. Perubahan mempertahankan LICENSE dan .gitignore. Perintah: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo check --workspace`, `cargo run -p xynoos-bench`. Semua berhasil; tiga unit test lulus.
