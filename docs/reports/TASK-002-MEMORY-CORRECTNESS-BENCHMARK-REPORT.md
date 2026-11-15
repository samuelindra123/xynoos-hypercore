# HALAMAN IDENTITAS

**Judul:** Integritas halaman, akuntansi memori, dan evaluasi benchmark userspace  
**Proyek:** Xynoos HyperCore  
**Task:** 002 — Memory Correctness, Integrity & Benchmark Research  
**Tanggal:** 15 November 2026  
**Milestone:** TASK 002 pada workspace 0.1.0  
**Status:** SELESAI

## ABSTRAK

TASK 001 menyediakan penyimpanan LZ4 tanpa checksum dan pengukuran satu dataset. Penelitian ini mengaudit seluruh crate, menambahkan pemeriksaan CRC32 dua lapis dan batas alokasi per halaman, serta mengevaluasi lima dataset terhadap baseline raw. Sembilan unit test dan lima quality gate berhasil. Data entropy menghasilkan rasio 1,004395 sehingga kompresi tidak selalu menghemat payload. RSS proses dan waktu CPU diukur dengan batas interpretasi eksplisit. Hasil berlaku untuk simulator userspace dan tidak mengukur reclaim kernel.

## 1. PENDAHULUAN

Rumusan masalah mencakup deteksi korupsi, konsistensi state setelah error, batas ukuran alokasi, dan pemisahan statistik payload dari penggunaan memori proses. Tujuannya meningkatkan keterujian dan menyediakan bukti empiris. Lingkup hanya library userspace dengan data sintetis dan kebijakan AURA baseline; tidak ada perubahan host, swap atau hardware.

## 2. LANDASAN TEKNIS

CRC32 IEEE mendeteksi banyak pola kesalahan byte dengan biaya komputasi tambahan. CRC tidak menyediakan autentikasi kriptografis dan collision tetap mungkin. LZ4 lossless dapat menghasilkan payload lebih besar daripada input. Vec memiliki panjang dan kapasitas alokasi berbeda; HashMap juga memiliki alokasi dan ruang kosong. RSS mencakup halaman resident seluruh proses; VmHWM adalah peak RSS proses, dengan ketelitian terbatas menurut dokumentasi proc. Benchmark harus memisahkan pembuatan data, codec, store dan verifikasi.

## 3. AUDIT IMPLEMENTASI SEBELUMNYA

Audit membaca seluruh lima manifest dan source, status Git, dokumentasi, serta struktur repository sebelum perubahan. Seluruh hasil TASK 001 masih berupa perubahan lokal; dipertahankan. Tidak ditemukan Critical atau High yang dapat dibuktikan.

| Severity | Temuan dan bukti | Penanganan |
|---|---|---|
| Medium | Entry TASK 001 hanya Vec; get memeriksa panjang tanpa checksum. Payload berubah yang tetap valid LZ4 dapat lolos. | CRC payload dan CRC asli dengan error berbeda. |
| Medium | Konstruktor mengizinkan ukuran hingga u32::MAX; get mengalokasikan berdasarkan konfigurasi tersebut. Ini risiko konfigurasi lokal, bukan exploit metadata eksternal. | Batas 1 MiB dan output buffer tetap. |
| Low | Perkalian page_count × page_size serta metadata tidak memvalidasi konfigurasi ekstrem; overflow merupakan risiko aritmetika teoretis, bukan kegagalan yang diamati pada RAM host. | Validasi kapasitas terhadap batas output codec dan metadata sebelum store dibuat. |
| Low | Overhead HashMap/allocator tidak terukur, benchmark nol tunggal belum cukup. | Akuntansi eksplisit dan harness multi-dataset. |
| Low | with_capacity memakai expect atas konfigurasi default valid; unwrap di test/asserteq demonstrasi bukan jalur input decoder. | Konstruktor fallible, caller diperbarui; assertion test dipertahankan sebagai oracle. |

Penghitung ID semula sudah checked_add dan tidak berubah saat Full/InvalidLength. Get tidak mutasi, remove unknown tidak mutasi. Tidak ada unsafe di library proyek. Tidak ada format impor atau panjang per-entry dari input publik; karena itu tidak ditemukan jalur metadata tidak tepercaya yang menentukan panjang alokasi. Allocation codec masih dapat abort pada OOM.

## 4. DESAIN PERBAIKAN

Entry menyimpan payload, payload_crc, original_crc. CRC payload diperiksa sebelum decoder; CRC asli sesudah panjang output diverifikasi. Batas konfigurasi privat membatasi output setiap get. CRC memakai crc32fast 1.5.5 dan cfg-if 1.0.5 transitif; dependency ini menyediakan CRC32 IEEE teroptimasi, bukan CRC32C. Proyek memakai API safe, tetapi pelarangan unsafe proyek bukan bukti bahwa seluruh dependency bebas unsafe.

## 5. IMPLEMENTASI

HyperMemory menambah Error::InvalidCapacity, PayloadChecksumMismatch, ChecksumMismatch, Allocation; Decompression tetap dipakai untuk decoder dan panjang salah. Insert memvalidasi ukuran, kapasitas, ID sebelum commit; reservasi map fallible sebelum entry dimasukkan. Get memakai try_reserve_exact dan decompress_into pada buffer tetap. OOM pada alokasi internal codec tidak seluruhnya menjadi Result.

API with_capacity kini Result<Self, Error>; CLI, test dan harness dimigrasikan. Format Entry berubah tanpa kebutuhan migrasi persistent karena belum ada serialisasi. Stats menambahkan hash_map_capacity dan store_struct_bytes; metadata pada target x86_64 menjadi 40 byte per entry termasuk padding, versus 32 byte pada TASK 001. Ini batas bawah entri aktif, bukan total alokasi map.

## 6. METODOLOGI PENGUJIAN

Lingkungan aktual Ubuntu 24.04.5 LTS, Linux 6.8.0-1070-azure x86_64, 4 CPU logis AMD EPYC 7763, rustc 1.100.0. Tidak ada instalasi toolchain besar. Harness release menjalankan 10 subprocess berurutan (lima dataset × dua backend), masing-masing 256 halaman × 4096 byte, empat warm-up dan 32 batch terukur. Dataset dibuat sebelum pengukuran: nol, pola periodik 13 byte, structured (48 dari setiap 64 byte berpola dan sisanya pseudoacak), entropy xorshift32 seed 0x12345678, serta mixed dengan pembagian sama empat jenis. Ini pseudoacak, bukan bukti entropi kriptografis.

Codec diukur terpisah dengan alokasi output, tanpa checksum/store; verifikasi seluruh buffer dilakukan setelah timer berhenti. Insert mencakup clone/raw atau kompresi+CRC+map/HyperMemory, pertumbuhan HashMap, dan pencatatan ID. Get mencakup lookup, alokasi/copy atau dekompresi+CRC dan pencatatan output; verifikasi eksternal di luar interval. black_box dan pemeriksaan byte mencegah eliminasi pekerjaan. Pembuatan dataset tidak masuk timer; tidak dilakukan estimasi waktunya.

Median/p95 memakai nearest rank atas 32 rata-rata batch per operasi; ini bukan tail latency operasi individual. P99 tidak dilaporkan karena sampel batch kecil. Throughput adalah median MiB input asli per detik pada codec; baris raw juga menjalankan codec sebagai kontrol terpisah, bukan throughput raw store. Timer Instant menyertakan penjadwalan host.

RSS awal setelah dataset dan RSS terisi pada batch terakhir berasal dari /proc/self/status, VmHWM mencakup keseluruhan subprocess. Pengukuran terisi dilakukan sebelum get tetapi sesudah codec, sehingga allocator/cache codec dapat memengaruhi RSS. CPU berasal dari selisih runtime scheduler /proc/self/schedstat untuk thread tunggal; meliputi warm-up, codec, store, verifikasi dan pembacaan proc, bukan biaya CPU store terisolasi. None berarti pengukuran tidak tersedia. Semua pengukuran kecil dan terbatas; tidak ada stress tanpa batas.

Pengujian mencakup delapan test HyperMemory dan satu AURA. Rangkaian model deterministik melakukan 512 insert pada masing-masing empat ukuran (2048 total), dengan maksimum 16 entri hidup dan verifikasi model berulang. Sebanyak 256 blok malformed kecil dilewatkan ke decoder dengan CRC payload diperbarui, sehingga jalur decoder juga diperiksa. Ini pengujian generatif terbatas tanpa dependency property-testing atau klaim fuzzing menyeluruh.

## 7. HASIL BENCHMARK

Data berikut berasal dari eksekusi release setelah perubahan kode terakhir. Sumber mesin: `docs/benchmarks/task-002.csv`. Semua dataset mewakili 1.048.576 byte asli.

| Dataset | Backend | Insert median/p95 (ns) | Get median/p95 (ns) | Codec kompresi/dekompresi (MiB/s) | Rasio |
|---|---|---:|---:|---:|---:|
| zero | raw | 1105.89/1548.00 | 2126.20/2932.07 | 6090.65/3387.88 | 1.000000 |
| zero | hyper | 869.24/971.50 | 2133.20/2468.23 | 6144.24/2096.39 | 0.006592 |
| pattern | raw | 1101.66/1590.59 | 2150.96/2850.00 | 5880.38/1063.21 | 1.000000 |
| pattern | hyper | 898.59/1314.60 | 4751.67/7604.69 | 5844.57/890.36 | 0.009521 |
| structured | raw | 1206.51/1727.75 | 2141.26/3193.77 | 1221.22/1591.54 | 1.000000 |
| structured | hyper | 3495.34/4210.35 | 3003.84/3655.09 | 1223.22/1519.49 | 0.337083 |
| entropy | raw | 196.07/274.57 | 1875.14/2709.19 | 3131.89/1973.22 | 1.000000 |
| entropy | hyper | 1673.32/2691.43 | 2440.06/3334.77 | 3066.09/1937.75 | 1.004395 |
| mixed | raw | 1119.31/1552.78 | 2164.62/3106.53 | 2498.90/1414.27 | 1.000000 |
| mixed | hyper | 1779.57/3121.80 | 3152.09/5013.45 | 2692.86/1403.12 | 0.339416 |

| Dataset | Backend | Payload/capacity (byte) | Metadata minimum (byte) | RSS awal/terisi (KiB) | Peak RSS (KiB) | CPU (ms) |
|---|---|---:|---:|---:|---:|---:|
| zero | raw | 1048576/1048576 | 8192 | 3196/4440 | 5348 | 61.636222 |
| zero | hyper | 6912/6912 | 10240 | 3276/3588 | 4236 | 61.936169 |
| pattern | raw | 1048576/1048576 | 8192 | 3104/4348 | 5340 | 87.956764 |
| pattern | hyper | 9984/9984 | 10240 | 3280/3576 | 4352 | 115.980785 |
| structured | raw | 1048576/1048576 | 8192 | 3252/4428 | 5304 | 98.940032 |
| structured | hyper | 353457/353457 | 10240 | 3300/3916 | 4792 | 128.929396 |
| entropy | raw | 1048576/1048576 | 8192 | 3260/4564 | 5352 | 62.974826 |
| entropy | hyper | 1053184/1053184 | 10240 | 3024/4404 | 5220 | 86.722723 |
| mixed | raw | 1048576/1048576 | 8192 | 3112/4296 | 5176 | 90.95835 |
| mixed | hyper | 355903/355903 | 10240 | 3188/3808 | 4644 | 100.97664 |

Semua map memiliki capacity 448 entri. Payload dan Vec capacity kebetulan sama pada eksekusi ini, bukan jaminan API. Batas bawah metadata HyperMemory 10.240 byte dan raw 8.192 byte; ukuran statis store dilaporkan API secara terpisah. Overhead allocator dan byte alokasi HashMap tidak diukur. RSS tidak dapat ditafsirkan sebagai ukuran payload fisik tersendiri.

## 8. ANALISIS DAN PEMBAHASAN

Data nol menghasilkan payload 6.912 byte; metadata minimum lebih besar daripada payload. Structured dan mixed menyusut menjadi sekitar sepertiga data asli. Entropy membesar 4.608 byte sebelum metadata; fallback raw layak dievaluasi pada task berikutnya. Pada structured, insert HyperMemory lebih mahal daripada raw; pola berulang juga memperlihatkan biaya get lebih tinggi. Ini observasi sampel, belum merupakan generalisasi ke workload nyata.

Subprocess mengurangi kontaminasi lintas backend, tetapi urutan selalu raw kemudian hyper, HashMap memakai seed acak internal, dan CPU host berbagi dengan Codespaces. Perbedaan throughput codec antarbackend mengindikasikan variasi kondisi pengukuran, bukan perubahan algoritma codec. RSS awal berbeda dan allocator mempertahankan arena; pengurangan RSS terisi dengan RSS awal hanya observasi proses, bukan footprint store yang terisolasi. Throughput codec batch tidak boleh disamakan dengan latency store. CPU total tidak membuktikan penyebab bottleneck spesifik tanpa profiling.

## 9. ANALISIS KEAMANAN

Kegagalan ukuran, kapasitas, ID habis, korupsi dan lookup tidak mengubah halaman lain. Get tidak memutasi state; test membandingkan snapshot statistik sebelum/sesudah kegagalan. Payload terpotong ditolak CRC; bila CRC disesuaikan decoder tetap dapat menolak input. Hasil decoder pendek dan checksum asli salah memiliki jalur error. Ukuran output berasal dari konfigurasi terbatas, bukan header payload. CRC bukan pertahanan terhadap pihak yang dapat mengganti data beserta checksum. Struktur privat menghindari API publik untuk injeksi korupsi; test internal menggunakan akses modul.

## 10. KETERBATASAN

Tidak ada jaminan bebas bug berdasarkan sembilan test. Fuzzing berkelanjutan, sanitizer/Miri, injeksi OOM dan profiling allocator belum dijalankan. Kapasitas jumlah halaman bukan kuota byte/RSS. Alokasi codec dapat abort; total store tetap dapat terlalu besar jika caller memberi kapasitas tinggi. RSS/VmHWM proc adalah pengukuran proses yang dapat kurang akurat; peak memuat dataset dan buffer sementara. Metadata corruption pada memori proses sendiri di luar model safe Rust tidak disimulasikan menyeluruh. Tidak ada persistensi, autentikasi, kernel reclaim, swap, atau perbandingan langsung zram/zswap. AURA tetap baseline dan biaya belum memengaruhi keputusan.

## 11. KESIMPULAN

Audit, penanganan integritas, pengujian kegagalan, akuntansi yang diberi batas interpretasi, dan benchmark lima dataset selesai. Sembilan unit test dan lima quality gate lulus. Hasil membuktikan round-trip serta penolakan kasus korupsi yang diuji, dan menunjukkan trade-off nyata berdasarkan compressibility; tidak membuktikan kesiapan produksi.

## 12. REKOMENDASI

TASK 003 sebaiknya mengevaluasi fallback raw dengan kuota byte, fuzzing decoder dan operasi store, serta pengukuran alokasi yang terisolasi. Tambahkan pengambilan sampel latency per operasi, urutan benchmark berimbang, sampel mentah per pengulangan, interval ketidakpastian dan workload rekaman userspace sebelum mengubah kebijakan AURA. Model AI untuk implementasi dapat dipilih berdasarkan kemampuan audit Rust, penulisan test dan penggunaan alat; hasil model tetap wajib diverifikasi quality gates.

## 13. DAFTAR PUSTAKA

1. crc32fast, dokumentasi resmi crate: https://docs.rs/crc32fast/ — CRC32 IEEE dan API hash, diakses 15 November 2026.
2. Linux man-pages, proc_pid_status(5): https://man7.org/linux/man-pages/man5/proc_pid_status.5.html — VmRSS, VmHWM dan batas akurasi.
3. Source dependency lz4_flex 0.11.7, src/block/compress.rs dan API decompress_into, diunduh Cargo dan diperiksa lokal; https://docs.rs/lz4_flex/0.11.7/lz4_flex/ .

## LAMPIRAN

Perintah aktual dan hasil akhir (exit 0):

- cargo fmt --all -- --check — lulus.
- cargo clippy --workspace --all-targets -- -D warnings — lulus tanpa warning.
- cargo test --workspace — 9 test lulus, 0 gagal; doctest tidak tersedia.
- cargo check --workspace — lulus.
- cargo build --workspace --release — lulus.
- target/release/xynoos-bench — 10 kombinasi dataset/backend selesai.

Tidak ada kegagalan quality gate yang tersisa. Build/test awal dilanjutkan setelah proses asynchronous selesai; hasil final dicatat setelah perubahan terakhir. Benchmark awal eksploratif dijalankan sebelum final, sedangkan CSV dan tabel memuat pengukuran final.

File berubah: HyperMemory lib dan manifest; AURA test; CLI caller; benchmark lib executable dan manifest; Cargo.lock; README; ARCHITECTURE; laporan ini; CSV benchmark dan log `docs/benchmarks/task-002-quality-gates.txt`. Cargo.toml workspace, LICENSE, .gitignore dan laporan TASK 001 dipertahankan. Repository belum dikomit/push sesuai ruang lingkup. git check-ignore memastikan target/release/xynoos-bench diabaikan.

Storage awal 59 MiB, akhir sekitar 119 MiB termasuk artefak build, kenaikan sekitar 60 MiB. Ruang filesystem tersedia akhir 17,897,205,760 byte (sekitar 16.67 GiB; df -h menampilkan 17G). Ini pengukuran filesystem bersama, bukan seluruh perubahan akibat source proyek saja. Tidak ada instalasi besar atau penghapusan source lama.
