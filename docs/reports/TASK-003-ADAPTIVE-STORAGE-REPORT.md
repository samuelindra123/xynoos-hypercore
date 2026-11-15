# HALAMAN IDENTITAS

**Judul:** Penyimpanan halaman adaptif dan kuota byte userspace  
**Proyek:** Xynoos HyperCore  
**Task:** 003 — Adaptive Page Storage, Memory Budget & Reliability Engineering  
**Tanggal:** 15 November 2026  
**Milestone:** fondasi riset 0.1.0  
**Status:** SELESAI

## ABSTRAK

Task ini menyelesaikan prototipe adaptif dari laporan sementara dengan pemilihan berdasarkan kapasitas Vec aktual, batas anggaran byte terhitung, pengujian correctness dan pengukuran tiga strategi. Empat belas unit test dan seluruh quality gate lulus. Pada dataset entropy sebanyak 1.048.576 byte, adaptive menyimpan 1.048.576 byte payload sementara LZ4 menyimpan 1.053.184 byte. Pada dataset nol keduanya menyimpan 6.912 byte. RSS terukur adalah milik seluruh proses dan hasilnya tidak membuktikan penghematan RSS. Hasil terbatas pada data sintetis dan simulator userspace.

## 1. PENDAHULUAN

LZ4 tidak selalu mengurangi ukuran halaman. Pertanyaan riset ialah apakah pemilihan representasi berdasarkan kapasitas buffer yang benar-benar dipertahankan dapat menghindari ekspansi, bagaimana batas budget ditegakkan, dan apa biaya relatifnya. Ruang lingkup ialah penyimpanan halaman memori proses sendiri; tidak ada integrasi reclaim kernel. Kriteria selesai sebelumnya yang belum terpenuhi mencakup perbandingan kapasitas raw aktual, uji tepi budget, benchmark tiga strategi, sampel mentah, dan laporan lengkap.

## 2. LANDASAN TEKNIS

LZ4 adalah codec lossless berbasis blok. Vec memiliki panjang logis dan kapasitas; kapasitas tidak menyatakan overhead allocator atau ukuran fisik. CRC32 IEEE mendeteksi banyak kesalahan namun tidak mengautentikasi dan collision mungkin. RSS serta VmHWM dari procfs mengukur proses, bukan satu struktur data; dokumentasi Linux menyatakan angka ini dapat kurang akurat. Median dan p95 batch dihitung atas rata-rata per halaman dalam batch, bukan distribusi latency individual.

## 3. AUDIT IMPLEMENTASI AWAL

Status Git sebelum pekerjaan menunjukkan README termodifikasi dan workspace, Cargo.lock, crate, dan docs belum dilacak; semua dipertahankan, termasuk laporan TASK 001/002. Implementasi TASK 002 selalu mengompresi, CRC memeriksa payload dan data asli, get membatasi output dan mempertahankan state saat error. Versi sementara TASK 003 menambahkan mode adaptive dan checked budget, tetapi membandingkan kapasitas LZ4 dengan *panjang* raw sebelum mengetahui kapasitas raw sebenarnya. Statistik awal juga memakai penjumlahan dan perkalian tanpa checked arithmetic yang eksplisit. Budget bukan RSS dan dilakukan sesudah kompresi; ini tetap batas desain. Uji sementara belum menguji batas tepat/kurang satu, benchmark tiga backend dan penyimpanan sampel belum tersedia. AURA tetap baseline; tidak dimodifikasi.

## 4. PERANCANGAN ALGORITMA ADAPTIVE STORAGE

Entry tetap satu tipe dengan Vec payload, flag raw, dan dua CRC; metadata statis kedua representasi identik. Setelah kompresi, bila kapasitas LZ4 kurang dari panjang input, kapasitas calon Vec raw pasti setidaknya panjang input dan kompresi dipilih tanpa alokasi raw. Jika tidak, raw dicadangkan dengan try_reserve_exact dan kapasitas kedua Vec aktual dibandingkan; raw menang pada kapasitas sama atau lebih kecil. Input hanya disalin ke raw bila raw dipilih. Vec capacity adalah ukuran yang dilaporkan Rust, bukan pengukuran overhead fisik allocator. Alokasi raw sementara dapat gagal walaupun kandidat LZ4 ada; error dikembalikan tanpa mengubah store.

Biaya budget setiap entry = kapasitas payload + size_of::<(PageId, Entry)> dengan checked_add. Total diperiksa sebelum reservasi map dan commit. Penghapusan mengembalikan biaya. Konstruktor memvalidasi perkalian kapasitas dan batas maksimum codec/metadata; statistik menggunakan checked arithmetic terhadap invariant ini. Failure sebelum commit tidak menggunakan ID. Biaya HashMap kosong/kelebihan kapasitas, allocator, struktur store, CRC runtime, kompresi sementara dan output get tidak termasuk budget. Budget bukan batas RAM absolut.

## 5. IMPLEMENTASI

`crates/xynoos-hypermemory/src/lib.rs` menyediakan `PageStore::adaptive(page_size, capacity, budget)`; `new` dan `with_capacity` mempertahankan always-LZ4. Kedua representasi diverifikasi CRC payload, panjang hasil dan CRC asli. Stats mencakup raw_pages, compressed_pages, raw_bytes, compressed_bytes, stored_bytes, buffer_capacity_bytes, accounted_bytes dan metadata_bytes_lower_bound. Compression ratio menggunakan seluruh payload; ini perubahan semantik dari TASK 002 jika menggunakan adaptive. Entry internal berubah tanpa migrasi karena tidak ada persistensi. `xynoos-bench` diperluas; skrip Python standar merangkum sampel. README dan arsitektur diperbarui.

## 6. METODOLOGI BENCHMARK

Lingkungan Ubuntu 24.04.5, Linux 6.8 Azure x86_64, 4 vCPU AMD EPYC 7763, rustc 1.100.0; release profile. Lima dataset deterministik: zero, periodik 13 byte, structured (48 byte berpola per 64 byte), pseudoacak xorshift32 seed 0x12345678, dan mixed 25% masing-masing. Dataset 256 halaman × 4096 byte dibuat sebelum timer dan sebelum RSS awal. Setiap kombinasi memakai subprocess terpisah, 4 warm-up dan 32 batch terukur; urutan backend dirotasi antar dataset. Ini bukan randomisasi pada setiap pengulangan, sehingga bias urutan tetap mungkin.

Raw menggunakan HashMap<usize, Vec<u8>> dengan clone dan tidak memakai CRC; LZ4 dan adaptive menggunakan PageStore, CRC dan map ID. Perbandingan karena itu memperlihatkan biaya keseluruhan implementasi, bukan codec murni. Insert dan get diukur sebagai durasi total batch dibagi 256; get mencakup alokasi dan output sementara. Hasil diverifikasi byte-per-byte setelah timer. `black_box` menghambat penghilangan kerja. Codec-only mengukur kompresi dan dekompresi LZ4 terpisah termasuk alokasi, tanpa CRC atau store; ini dijalankan pula pada subprocess raw dan tidak berarti raw menjalankan codec pada hot path. CPU /proc/self/schedstat adalah runtime scheduler kumulatif seluruh benchmark subprocess, termasuk warm-up, codec, pengecekan dan pembacaan proc, bukan CPU khusus insert/get.

Sampel mentah terdiri dari 480 baris (15 kombinasi × 32); kolom RSS terisi diambil setelah insert pada batch terkait, peak dan CPU kumulatif diambil setelah batch. Ringkasan menggunakan median dan nearest-rank p95 dari 32 rata-rata batch. `rss_before_kib` setelah dataset, `rss_loaded_kib` dan peak meliputi seluruh proses serta efek allocator dan pengukuran codec sebelumnya. Nilai 0 berarti pembacaan proc tidak tersedia, bukan RSS nol. Pengukuran dibatasi sekitar 1 MiB dataset setiap subprocess dan dijalankan berurutan.

## 7. HASIL QUALITY GATES

| Perintah | Hasil |
|---|---|
| cargo fmt --all -- --check | lulus |
| cargo clippy --workspace --all-targets -- -D warnings | lulus |
| cargo test --workspace | 14 unit test lulus, 0 gagal; doc-test 0 |
| cargo check --workspace | lulus |
| cargo build --workspace --release | lulus |

Test mencakup exact budget, budget kurang satu dan lebih satu byte, full, state gagal, remove dan reuse, mixed, overflow konfigurasi, raw dan LZ4 round-trip, CRC payload dan original, truncation, input invalid, model operasi deterministik dan 256 blok malformed dari TASK 002. Ini pengujian generatif terbatas, bukan fuzzing menyeluruh atau property-testing berbasis framework.

## 8. HASIL BENCHMARK TIGA STRATEGI

| Dataset | Strategi | Insert median / p95 batch ns/halaman | Get median / p95 batch ns/halaman | Rasio | Payload / kapasitas byte | Raw / LZ4 | RSS terisi / peak KiB |
|---|---|---:|---:|---:|---:|---:|---:|
| zero | raw | 1497.53 / 2008.62 | 2137.54 / 3076.2 | 1.0 | 1048576 / 1048576 | 256 / 0 | 4340 / 5272 |
| zero | lz4 | 893.52 / 1785.48 | 2255.09 / 3112.25 | 0.006592 | 6912 / 6912 | 0 / 256 | 3512 / 4316 |
| zero | adaptive | 1014.06 / 1872.79 | 2293.32 / 3232.51 | 0.006592 | 6912 / 6912 | 0 / 256 | 3576 / 4384 |
| pattern | lz4 | 921.71 / 1481.28 | 4803.06 / 6307.27 | 0.009521 | 9984 / 9984 | 0 / 256 | 3436 / 4096 |
| pattern | adaptive | 907.61 / 1771.11 | 4757.62 / 6659.64 | 0.009521 | 9984 / 9984 | 0 / 256 | 3532 / 4324 |
| pattern | raw | 1476.3 / 1838.43 | 2115.82 / 2555.9 | 1.0 | 1048576 / 1048576 | 256 / 0 | 4344 / 5272 |
| structured | adaptive | 3464.37 / 5816.55 | 2986.64 / 4596.19 | 0.337083 | 353457 / 353457 | 0 / 256 | 3876 / 4608 |
| structured | raw | 1256.22 / 1701.88 | 2159.79 / 3212.52 | 1.0 | 1048576 / 1048576 | 256 / 0 | 4428 / 5304 |
| structured | lz4 | 3492.64 / 6194.56 | 3028.19 / 4762.2 | 0.337083 | 353457 / 353457 | 0 / 256 | 3908 / 4664 |
| entropy | raw | 208.47 / 284.91 | 1813.78 / 2549.87 | 1.0 | 1048576 / 1048576 | 256 / 0 | 4632 / 5388 |
| entropy | lz4 | 1693.79 / 2644.54 | 2430.64 / 3565.98 | 1.004395 | 1053184 / 1053184 | 0 / 256 | 4584 / 5488 |
| entropy | adaptive | 1797.9 / 3118.78 | 2333.38 / 3360.88 | 1.0 | 1048576 / 1048576 | 256 / 0 | 4608 / 5480 |
| mixed | lz4 | 1837.38 / 3193.41 | 3215.41 / 4972.16 | 0.339416 | 355903 / 355903 | 0 / 256 | 3916 / 4668 |
| mixed | adaptive | 1860.13 / 2537.82 | 3207.86 / 4258.29 | 0.338317 | 354751 / 354751 | 64 / 192 | 3768 / 4564 |
| mixed | raw | 1109.68 / 1593.52 | 2190.92 / 3041.61 | 1.0 | 1048576 / 1048576 | 256 / 0 | 4412 / 5308 |


Median codec kompresi/dekompresi, CPU dan RSS awal tersedia di CSV ringkasan. Contoh entropy: LZ4 2883,93/1976,22 MiB/s, adaptive 3027,68/1976,45 MiB/s; codec diukur terpisah dan perbedaan ini mencerminkan variasi proses. CPU kumulatif entropy raw/LZ4/adaptive adalah 65,00/85,52/88,96 ms; angka ini termasuk codec dan semua kerja harness. Metadata minimum raw 8.192 byte, LZ4 maupun adaptive 12.288 byte untuk 256 halaman; ini bukan overhead allocator atau HashMap total.

## 9. ANALISIS MEMORY ACCOUNTING

Bukti terukur menunjukkan adaptive memakai kapasitas payload 4.608 byte lebih sedikit daripada LZ4 untuk entropy dan 1.152 byte lebih sedikit untuk mixed. Zero, pattern dan structured memiliki kapasitas sama dengan LZ4. Raw baseline memiliki metadata struktur entry yang berbeda dan tanpa CRC, sehingga tidak setara secara fitur. Tiap halaman PageStore menyumbang 48 byte ukuran statis pasangan pada target ini; hash_map_capacity, store_struct_bytes dan Vec capacity tidak mengukur ukuran bucket fisik atau allocator. Perhitungan ukuran raw hypothetical versus metadata yang berbeda harus mempertimbangkan perbedaan itu.

RSS entropy terisi adalah 4.584 KiB LZ4 dan 4.608 KiB adaptive; peak 5.488 vs 5.480 KiB. Dataset, alokasi sementara, arena allocator dan pemetaan proses bercampur; tidak dapat menyimpulkan selisih RSS yang signifikan dari satu subprocess per kombinasi. Stat proc tersebut bukan pengukuran tepat per-objek. CSV mempertahankan nilai aktual tanpa mengonversi perbedaan ini menjadi klaim penghematan fisik.

## 10. ANALISIS CORRECTNESS DAN KEAMANAN

Insert memvalidasi ukuran, kapasitas halaman, ID dan byte sebelum mengubah map atau counter; test memeriksa stats dan ID setelah gagal. CRC payload diverifikasi sebelum copy/dekompresi dan CRC asli setelah validasi panjang. CRC bukan MAC, dan pihak yang dapat mengganti data serta CRC dapat membentuk data lain. Decoder dibatasi page_size maksimal 1 MiB. Kode library melarang unsafe; dependency mungkin memiliki unsafe internal. Tidak ada jaminan seluruh alokasi fallible: codec dan runtime dapat abort pada OOM. Kapasitas yang diminta dapat menyebabkan memori sementara melebihi budget.

## 11. TRADE-OFF PERFORMA

Pada entropy median insert raw 208,47 ns/halaman, LZ4 1.693,79 dan adaptive 1.797,90: fallback masih membayar percobaan kompresi, serta alokasi/copy raw. Selisih 104,11 ns antara LZ4 dan adaptive adalah observasi, bukan estimasi efek kausal yang presisi. Pada zero insert adaptive 1.014,06 vs LZ4 893,52 ns; pada pattern adaptive 907,61 vs LZ4 921,71 ns. Overlap p95 batch dan variasi urutan membuat klaim pemenang umum tidak didukung. Untuk structured adaptive dan LZ4 menggunakan representasi sama; selisih timing tidak membuktikan perbedaan berarti. Adaptive mengurangi ekspansi payload entropy, tetapi metadata tetap lebih tinggi daripada baseline raw.

## 12. KETERBATASAN

Hanya satu eksekusi subprocess tiap kombinasi, tanpa interval kepercayaan, tanpa workload produksi, pengukuran latency individual, atau isolasi heap. Rotasi dilakukan antar dataset dan bukan pengacakan penuh. RSS dan peak proses dipengaruhi allocator, codec dan dataset. Throughput codec tidak termasuk CRC, sementara latency store mencakupnya. Raw baseline tidak melakukan verifikasi CRC. Tidak ada fuzzing coverage-guided, injeksi OOM, persistensi, maupun perbandingan setara dengan zram/zswap/kernel. User harus memahami budget logis; tidak ada jaminan RSS maksimum.

## 13. KESIMPULAN

Pemilihan berdasarkan kapasitas Vec aktual dan penegakan budget yang dapat dihitung terimplementasi; seluruh gate lulus. Benchmark membuktikan perbedaan payload pada dataset yang diuji, bukan peningkatan kinerja atau RAM keseluruhan secara umum. Sasaran TASK 003 tercapai dalam batas penggunaannya sebagai riset userspace.

## 14. REKOMENDASI TASK 004

Evaluasi pengukuran heap terisolasi, beberapa subprocess berulang dan urutan berimbang acak, latency individual, serta fuzzing coverage-guided berbatas. Pertimbangkan backend raw dengan API dan CRC yang sama untuk baseline yang lebih sebanding, juga kebijakan yang dapat menghindari percobaan kompresi ketika bukti workload mendukung. Ini rekomendasi; TASK 004 belum dimulai.

## 15. DAFTAR PUSTAKA

1. Rust Vec, kapasitas dan reservasi: https://doc.rust-lang.org/std/vec/struct.Vec.html
2. crc32fast, CRC32 IEEE: https://docs.rs/crc32fast/1.5.5/crc32fast/
3. Linux proc_pid_status(5), VmRSS/VmHWM dan akurasi: https://man7.org/linux/man-pages/man5/proc_pid_status.5.html
4. lz4_flex 0.11.7: https://docs.rs/lz4_flex/0.11.7/lz4_flex/

## 16. LAMPIRAN DAN BUKTI

- Sampel mentah: `docs/benchmarks/task-003-raw.csv`; ringkasan: `docs/benchmarks/task-003-summary.csv`; log gate: `docs/benchmarks/task-003-quality-gates.txt`.
- Reproduksi: `cargo build --workspace --release`; `target/release/xynoos-bench > docs/benchmarks/task-003-raw.csv`; `python3 scripts/summarize_task003.py`. Jalankan lima perintah gate pada tabel di atas.
- Berubah: HyperMemory, benchmark, README, arsitektur dan laporan ini; skrip ringkasan dan CSV/log ditambahkan. Laporan TASK 001/002 tetap ada. Tidak ada commit atau push.
- Storage awal 122 MiB; pada pengukuran akhir sekitar 122 MiB. Ruang tersedia 17,892,978,688 byte; filesystem bersama dapat berubah karena aktivitas lain. Target build diabaikan Git.
