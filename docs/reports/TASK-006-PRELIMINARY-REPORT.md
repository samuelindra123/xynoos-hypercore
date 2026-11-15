## 1. HALAMAN IDENTITAS

Xynoos HyperCore TASK 006 — AURA Simulator Reliability. 15 November 2026. Status SELESAI SEBAGIAN.

## 2. ABSTRAK

Audit dan selector bounded terlaksana; 34 test dan lima quality gate lulus. Eksperimen 243 kombinasi tersimpan. Definisi no-feasible, pemisahan stall/accounting, validasi scratch, kalibrasi lima dataset, serta analisis transaksi belum lengkap.

## 3. PENDAHULUAN

Tujuannya mengurangi stagnasi selector dan membandingkan kebijakan pada kandidat identik dalam simulator userspace.

## 4. LANDASAN TEKNIS

Fairness kandidat berbeda dari fairness pemilihan tier. Eligibility eksogen pada eksperimen tidak membuktikan kelayakan kapasitas, cooldown atau budget state simulator.

## 5. AUDIT TASK 005

Perubahan TASK004/005 belum terlacak atau termodifikasi saat awal dan dipertahankan. Selector recency eksogen dapat berulang memilih grup backing; physical memakai shared RAM, stall threshold75%, serta clone seluruh metadata setiap event. Fallback recency tetap dibatasi cooldown dan budget. Benchmark TASK003 menunjukkan lima dataset dengan rasio dan throughput codec yang berbeda.

## 6. ANALISIS CANDIDATE STARVATION

Selector historis memilih recency terkecil tanpa memeriksa tier, dan dapat berulang memilih grup yang tidak dapat dipindahkan. Progress inspeksi berbeda dengan progress migrasi.

## 7. PERANCANGAN SELECTOR

selector.rs memakai cursor round-robin dan mask eligibility eksternal; maksimal64 evaluasi per pemanggilan dan tie ditentukan urutan circular. Counter evaluations, rejected dan no_candidate memakai checked arithmetic; overflow tidak mengubah selector. Dengan mask tetap tiap kandidat true diperiksa dalam paling banyak satu putaran. Mask bukan hasil feasibility simulator.

## 8. MODEL ACCOUNTING DAN STALL

Simulator physical TASK005 tetap digunakan tanpa perubahan. Ia menghitung raw+compressed+metadata dan budget scratch secara terpisah. Peak total bersamaan permanent+scratch belum dihitung; pemisahan eksperimen accounting dan stall belum dilaksanakan. Angka stall tetap asumsi TASK005 dan bukan waktu nyata.

## 9. DESAIN TRANSAKSI

Clone O(n) dipertahankan. Journal atau commit alternatif belum diimplementasikan; differential test antara clone dan journal belum tersedia. Ini menjaga perilaku historis tetapi tidak membuktikan equivalence mekanisme alternatif.

## 10. IMPLEMENTASI

Ditambahkan crates/xynoos-aura/src/selector.rs dan examples/selector_research.rs; lib.rs mengekspor selector. Contoh menggunakan fixture physical dan kandidat eksternal identik untuk setiap policy. Tidak mengubah eksperimen TASK005.

## 11. METODOLOGI PENGUJIAN

Uji selector dengan mask tetap/berubah, semua false, batas64, input kosong dan overflow atomic. Eksperimen 9 trace ×3 seed ×3 policy ×3 selector =243 kombinasi; masing-masing 512 event. Bounded memakai mask eksogen yang menolak halaman yang sedang diakses; historical recency dan round-robin tidak memakai mask tersebut. Perbedaan ini merupakan confound, sehingga perbandingan selector tidak mengisolasi algoritma semata.

## 12. HASIL UNIT TEST

34 unit test workspace lulus; tiga test baru selector. Pengujian khusus no-feasible migration, candidate state eligibility, differential journal dan peak total scratch masih belum tersedia.

## 13. HASIL QUALITY GATES

cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo check --workspace; cargo build --workspace --release semuanya lulus. Build contoh release juga lulus. Kompilasi awal contoh gagal karena selector::Error belum mengimplementasikan std::error::Error; diperbaiki dan seluruh gate diulang. Log hasil final tersedia.

## 14. HASIL BENCHMARK SELECTOR

Pada trace interactive/Aura, historical menghasilkan 2 refault dan 3 migrasi, bounded 64 refault dan 74 migrasi, round-robin 33 refault dan 75 migrasi; ketiganya hasil simulasi rata-rata tiga seed. Mask berbeda berarti angka ini tidak membuktikan penyebab performa. event_ns hanya mengukur Simulator.event_candidate, tanpa biaya pemilihan selector. CSV memuat restore/stall, CPU/I/O, physical dan backing.

## 15. HASIL DIFFERENTIAL TEST TRANSAKSI

Tidak dilakukan: alternatif journal belum diimplementasikan. Jangan menafsirkan pengulangan deterministik sebagai differential test transaksi.

## 16. ANALISIS KOMPLEKSITAS

Selector bounded O(n) dengan n≤64, state O(1); simulator masih meng-clone metadata O(n) per event. Tidak ada benchmark clone vs journal.

## 17. ANALISIS KEAMANAN

Operasi userspace dan bounded; tidak menyentuh kernel, swap atau perangkat. Budget permanen tidak membatasi RSS dan scratch bersamaan. CRC dan persistensi bukan fokus selector.

## 18. KETERBATASAN DAN ANCAMAN VALIDITAS

Counter rejected hanya mask eksogen, bukan penolakan placement; no_candidate berarti tidak ada eligibility dalam mask, bukan no feasible migration. Counter dan biaya selector historis/round-robin belum diukur sebanding. Kalibrasi lima dataset belum diimplementasikan. Simulator belum mengintegrasikan pencarian kandidat berikutnya saat keputusan policy Keep/Defer. Perbedaan mask menghalangi kesimpulan kausal.

## 19. KESIMPULAN

Selector eksogen dan eksperimen awal berfungsi. Kriteria fairness menyeluruh, akuntansi scratch dan stall, serta evaluasi transaksi yang dapat diaudit belum terpenuhi; status SELESAI SEBAGIAN.

## 20. REKOMENDASI TASK 007

Selesaikan TASK006 terlebih dahulu: selector feasibility state-aware dengan protokol kandidat sama, eksperimen accounting/stall terpisah, total scratch, kalibrasi lima dataset, dan evaluasi journal berbasis differential testing.

## 21. DAFTAR PUSTAKA

Source primer repository: physical.rs, selector.rs, laporan TASK005 dan CSV task-003-summary.csv. Tidak ada klaim sumber eksternal.

## 22. LAMPIRAN

Reproduksi: cargo build -p xynoos-aura --example selector_research --release; target/release/examples/selector_research > docs/benchmarks/task-006-raw.csv. Ringkasan, log quality gates dan reproducibility pada docs/benchmarks. Dua eksekusi identik pada kolom simulasi, event_ns tidak dibandingkan. Storage awal153 MiB, akhir162 MiB; ruang tersisa17 GiB menurut df -h. Tidak ada commit/push.
