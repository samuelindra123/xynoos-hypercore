## 1. HALAMAN IDENTITAS

Xynoos HyperCore — TASK 005: AURA v2 Physical Memory Accounting, Fair Policy Evaluation & Model Validation. Tanggal 15 November 2026. Milestone riset userspace 0.1.0. Status: SELESAI.

## 2. ABSTRAK

Model baru menggabungkan RAM raw, compressed dan metadata dalam satu budget simulasi. Kandidat eksternal identik memungkinkan perbandingan keputusan yang lebih adil. Tujuh test baru, 31 unit test workspace dan seluruh gate lulus. Eksperimen 270 kombinasi menunjukkan fallback mengurangi okupansi tetapi dapat meningkatkan refault dan stall. Tidak ada klaim performa kernel.

## 3. PENDAHULUAN

Tujuan ialah mengurangi bias model kapasitas dan kandidat serta memvalidasi failure atomicity. Ruang lingkup metadata userspace, tanpa integrasi kernel, swap atau backing device nyata.

## 4. LANDASAN TEKNIS

Kapasitas compressed-RAM tetap memakai RAM fisik. Model menghitung payload dan metadata terpisah; ukuran fisik simulasi tidak identik dengan RSS. Latensi kernel tidak dapat disamakan dengan benchmark codec userspace.

## 5. AUDIT TASK 004

Status Git awal memuat perubahan lokal TASK 004; dipertahankan. Source placement, contoh research, CSV TASK 003/004, dan laporan dibaca.

| Risiko | Temuan | Penanganan |
|---|---|---|
| Tinggi terhadap validitas | compressed diberi kapasitas mandiri | physical budget bersama |
| Tinggi terhadap validitas | selector berbeda antar policy | kandidat exogenous identik |
| Sedang | fallback menahan semua halaman | recency fallback terbatas |
| Sedang | clone O(n) per event | dipertahankan untuk rollback |
| Sedang | biaya asumsi tanpa kalibrasi | proxy dari throughput TASK 003 |

Tingkat risiko ini menyatakan validitas model, bukan kerentanan keamanan host.

## 6. ANALISIS KETERBATASAN MODEL LAMA

TASK 004 dapat memindahkan halaman incompressible ke compressed walaupun tidak menghemat physical RAM. Stall hanya menghitung okupansi RAM raw. Refault nol pada fallback tidak berarti tekanan teratasi. Simulator historis placement.rs dan artefaknya tidak diubah.

## 7. PERANCANGAN SHARED PHYSICAL MEMORY

Modul physical.rs independen mempertahankan API historis. Budget eksplisit meliputi resident raw+compressed+metadata. Metadata tetap untuk seluruh grup, termasuk grup backing; cache backing dan allocator tetap tidak diketahui. Buffer migrasi memiliki allowance terpisah 8192 byte, dicatat pada peak_temporary_bytes. Jadi budget permanen bukan batas total termasuk scratch atau RSS.

## 8. MODEL MATEMATIS DAN INVARIANT

P = used[RAM]+used[Compressed]+metadata ≤ physical_budget. Transisi mengurangi sumber lalu menambah tujuan pada proyeksi, seluruhnya checked. Backing payload tidak masuk P. Kapasitas tier dan scratch juga diperiksa. Cost tetap persamaan TASK 004 dalam ns ekuivalen; kelayakan shared RAM diterapkan sebelum placement. Input invalid, overflow dan kegagalan event memakai transaksi clone sehingga state tidak berubah.

## 9. PERANCANGAN FAIR POLICY EVALUATION

Trace recency exogenous memilih indeks dengan akses historis terkecil, tie indeks; tidak tergantung tier atau policy. Kandidat sama untuk lru, compression, aura dan ablation. Varian round_robin khusus mengubah selector. Selector exogenous dapat terus menunjuk halaman backing yang sama; ini ancaman validitas yang terukur, bukan alasan mengubah trace untuk memenangkan AURA.

## 10. IMPLEMENTASI

physical.rs menambah budget fisik, metadata, temporary limit, physical_usage, projected feasible dan event_candidate. Fallback recency menggunakan aturan LRU (dingin, tekanan aktif, eligibility, cooldown, CPU/I/O, rate dan max migrasi). fallback_recency=false mempertahankan fallback original. use_cost=false mengganti hanya kriteria placement dengan compression-first sementara confidence guard tetap aktif. placement.rs historis tidak dimodifikasi.

## 11. METODOLOGI EKSPERIMEN

Sembilan trace TASK 004, 512 event, seed 1/42/2026. Sepuluh varian: lru, compression, aura, old_fallback, no_hysteresis (exit699), no_cooldown (0), no_confidence (threshold0), no_cost, round_robin, legacy_params. Metadata48 byte/grup dari TASK003; budget66304 byte (65536 payload awal+768 metadata). Tier limits65536/16384/131072, CPU10000 ns/tick dan IO8192 byte/tick. Scratch8192. Kalibrasi terbatas memakai zero LZ4 payload27 dan entropy4114. Biaya proxy codec =4096/(throughput MiB/s×1048576)×1e9: zero compress718/decompress1978 ns; entropy1355/1977 ns dari baris LZ4 TASK003. Ini wall-time proxy, bukan CPU time kernel. Profil lain, restore backing100000 ns, prior confidence dan bobot kapasitas tetap asumsi. legacy_params mempertahankan biaya/ukuran TASK004, selector tick%16 dan fallback lama; metadata dan definisi stall fisik berubah secara eksplisit. Experiment A membandingkan legacy_params dengan research historis yang dijalankan ulang pada task-005-legacy.csv; bukan pengujian satu variabel murni karena accounting/stall ikut diperbaiki. B memakai kandidat identik; C aura vs old_fallback; D membandingkan aura dengan setiap no_* satu parameter. Dua proses akhir dibandingkan persis kecuali timing.

## 12. HASIL QUALITY GATES

31 unit test lulus: 18 AURA termasuk7 test physical baru,13 HyperMemory. Formatting, Clippy all-targets -D warnings, test workspace, check workspace, build workspace release dan build examples release lulus. Log pada task-005-quality-gates.txt. Satu kegagalan awal test fallback mengharapkan migrasi pada tick1; cooldown4 membuat penolakan benar. Test diperbaiki memeriksa tick4, tanpa menonaktifkan cooldown.

## 13. HASIL SIMULASI

| Trace | Varian | Refault | Stall ns | Migrasi | Physical byte | Peak byte | CPU ns | I/O byte | Fallback |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| interactive | lru | 0.0 | 5120000.0 | 1.0 | 62208.0 | 66304.0 | 0.0 | 4096.0 | 0.0 |
| interactive | compression | 6.0 | 5150000.0 | 7.0 | 62235.0 | 66304.0 | 5392.0 | 24576.0 | 0.0 |
| interactive | aura | 4.0 | 5140000.0 | 5.0 | 62235.0 | 66304.0 | 10701.0 | 0.0 | 0.0 |
| interactive | old_fallback | 4.0 | 5140000.0 | 5.0 | 62235.0 | 66304.0 | 10701.0 | 0.0 | 0.0 |
| interactive | no_hysteresis | 4.0 | 5140000.0 | 5.0 | 62235.0 | 66304.0 | 10701.0 | 0.0 | 0.0 |
| interactive | no_cooldown | 1.0 | 5120000.0 | 3.0 | 62235.0 | 66304.0 | 3414.0 | 0.0 | 0.0 |
| interactive | no_confidence | 4.0 | 5140000.0 | 5.0 | 62235.0 | 66304.0 | 10701.0 | 0.0 | 0.0 |
| interactive | no_cost | 6.0 | 5150000.0 | 7.0 | 62235.0 | 66304.0 | 5392.0 | 24576.0 | 0.0 |
| interactive | round_robin | 66.0 | 120000.0 | 141.0 | 41944.0 | 66304.0 | 187581.0 | 0.0 | 0.0 |
| interactive | legacy_params | 64.0 | 430000.0 | 74.0 | 41270.0 | 66304.0 | 276000.0 | 0.0 | 0.0 |
| background | lru | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 0.0 |
| background | compression | 30.0 | 5160000.0 | 53.0 | 62226.0 | 66304.0 | 38164.0 | 118784.0 | 0.0 |
| background | aura | 15.33 | 5136666.67 | 28.0 | 63591.33 | 66304.0 | 41044.33 | 0.0 | 0.0 |
| background | old_fallback | 15.33 | 5136666.67 | 28.0 | 63591.33 | 66304.0 | 41044.33 | 0.0 | 0.0 |
| background | no_hysteresis | 15.33 | 5136666.67 | 28.0 | 63591.33 | 66304.0 | 41044.33 | 0.0 | 0.0 |
| background | no_cooldown | 13.0 | 5120000.0 | 26.67 | 63591.33 | 66304.0 | 35526.67 | 0.0 | 0.0 |
| background | no_confidence | 15.33 | 5136666.67 | 28.0 | 63591.33 | 66304.0 | 41044.33 | 0.0 | 0.0 |
| background | no_cost | 30.0 | 5160000.0 | 53.0 | 62226.0 | 66304.0 | 38164.0 | 118784.0 | 0.0 |
| background | round_robin | 259.33 | 2076666.67 | 432.67 | 46013.0 | 66304.0 | 722240.33 | 0.0 | 0.0 |
| background | legacy_params | 243.33 | 2150000.0 | 413.0 | 46042.67 | 66304.0 | 1151333.33 | 0.0 | 0.0 |
| switching | lru | 105.0 | 5570000.0 | 121.0 | 62208.0 | 66304.0 | 0.0 | 679936.0 | 0.0 |
| switching | compression | 111.0 | 5600000.0 | 127.0 | 62235.0 | 66304.0 | 115942.0 | 393216.0 | 0.0 |
| switching | aura | 79.0 | 5440000.0 | 95.0 | 62235.0 | 66304.0 | 200886.0 | 0.0 | 0.0 |
| switching | old_fallback | 79.0 | 5440000.0 | 95.0 | 62235.0 | 66304.0 | 200886.0 | 0.0 | 0.0 |
| switching | no_hysteresis | 79.0 | 5440000.0 | 95.0 | 62235.0 | 66304.0 | 200886.0 | 0.0 | 0.0 |
| switching | no_cooldown | 31.0 | 5120000.0 | 63.0 | 62235.0 | 66304.0 | 84294.0 | 0.0 | 0.0 |
| switching | no_confidence | 79.0 | 5440000.0 | 95.0 | 62235.0 | 66304.0 | 200886.0 | 0.0 | 0.0 |
| switching | no_cost | 111.0 | 5600000.0 | 127.0 | 62235.0 | 66304.0 | 115942.0 | 393216.0 | 0.0 |
| switching | round_robin | 100.0 | 440000.0 | 211.0 | 33806.0 | 66304.0 | 292129.0 | 0.0 | 0.0 |
| switching | legacy_params | 96.0 | 600000.0 | 152.0 | 41270.0 | 66304.0 | 450000.0 | 0.0 | 0.0 |
| pressure | lru | 10.33 | 1546666.67 | 20.33 | 62208.0 | 66304.0 | 0.0 | 86016.0 | 0.0 |
| pressure | compression | 10.33 | 1546666.67 | 20.33 | 62226.0 | 66304.0 | 13958.67 | 42325.33 | 0.0 |
| pressure | aura | 5.0 | 1540000.0 | 10.67 | 63591.33 | 66304.0 | 13958.67 | 0.0 | 0.0 |
| pressure | old_fallback | 5.0 | 1540000.0 | 10.67 | 63591.33 | 66304.0 | 13958.67 | 0.0 | 0.0 |
| pressure | no_hysteresis | 5.0 | 1540000.0 | 10.67 | 63591.33 | 66304.0 | 13958.67 | 0.0 | 0.0 |
| pressure | no_cooldown | 5.0 | 1540000.0 | 10.67 | 63591.33 | 66304.0 | 13958.67 | 0.0 | 0.0 |
| pressure | no_confidence | 5.0 | 1540000.0 | 10.67 | 63591.33 | 66304.0 | 13958.67 | 0.0 | 0.0 |
| pressure | no_cost | 10.33 | 1546666.67 | 20.33 | 62226.0 | 66304.0 | 13958.67 | 42325.33 | 0.0 |
| pressure | round_robin | 77.67 | 723333.33 | 131.33 | 46013.0 | 66304.0 | 219967.67 | 0.0 | 0.0 |
| pressure | legacy_params | 76.0 | 753333.33 | 131.67 | 46042.67 | 66304.0 | 368000.0 | 0.0 | 0.0 |
| entropy | lru | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 0.0 |
| entropy | compression | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 0.0 |
| entropy | aura | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 0.0 |
| entropy | old_fallback | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 0.0 |
| entropy | no_hysteresis | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 0.0 |
| entropy | no_cooldown | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 0.0 |
| entropy | no_confidence | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 0.0 |
| entropy | no_cost | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 0.0 |
| entropy | round_robin | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 0.0 |
| entropy | legacy_params | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 0.0 |
| compressible | lru | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 0.0 |
| compressible | compression | 30.0 | 5160000.0 | 53.0 | 62235.0 | 66304.0 | 78726.0 | 0.0 | 0.0 |
| compressible | aura | 30.0 | 5160000.0 | 53.0 | 62235.0 | 66304.0 | 78726.0 | 0.0 | 0.0 |
| compressible | old_fallback | 30.0 | 5160000.0 | 53.0 | 62235.0 | 66304.0 | 78726.0 | 0.0 | 0.0 |
| compressible | no_hysteresis | 30.0 | 5160000.0 | 53.0 | 62235.0 | 66304.0 | 78726.0 | 0.0 | 0.0 |
| compressible | no_cooldown | 26.0 | 5120000.0 | 53.0 | 62235.0 | 66304.0 | 70814.0 | 0.0 | 0.0 |
| compressible | no_confidence | 30.0 | 5160000.0 | 53.0 | 62235.0 | 66304.0 | 78726.0 | 0.0 | 0.0 |
| compressible | no_cost | 30.0 | 5160000.0 | 53.0 | 62235.0 | 66304.0 | 78726.0 | 0.0 | 0.0 |
| compressible | round_robin | 321.33 | 736666.67 | 512.0 | 31039.33 | 66304.0 | 822516.67 | 0.0 | 0.0 |
| compressible | legacy_params | 320.67 | 733333.33 | 512.0 | 32853.33 | 66304.0 | 1483333.33 | 0.0 | 0.0 |
| bursty | lru | 7.33 | 5133333.33 | 13.33 | 60842.67 | 66304.0 | 0.0 | 60074.67 | 0.0 |
| bursty | compression | 7.33 | 5133333.33 | 13.33 | 60860.67 | 66304.0 | 9646.0 | 31402.67 | 0.0 |
| bursty | aura | 6.67 | 5156666.67 | 7.33 | 62241.0 | 66304.0 | 16934.33 | 0.0 | 0.0 |
| bursty | old_fallback | 6.67 | 5156666.67 | 7.33 | 62241.0 | 66304.0 | 16934.33 | 0.0 | 0.0 |
| bursty | no_hysteresis | 6.67 | 5156666.67 | 7.33 | 62241.0 | 66304.0 | 16934.33 | 0.0 | 0.0 |
| bursty | no_cooldown | 2.67 | 5120000.0 | 6.0 | 63591.33 | 66304.0 | 7668.0 | 0.0 | 0.0 |
| bursty | no_confidence | 6.67 | 5156666.67 | 7.33 | 62241.0 | 66304.0 | 16934.33 | 0.0 | 0.0 |
| bursty | no_cost | 7.33 | 5133333.33 | 13.33 | 60860.67 | 66304.0 | 9646.0 | 31402.67 | 0.0 |
| bursty | round_robin | 194.67 | 1773333.33 | 204.67 | 44650.67 | 66304.0 | 473908.67 | 0.0 | 0.0 |
| bursty | legacy_params | 131.67 | 1276666.67 | 174.67 | 43647.33 | 66304.0 | 578333.33 | 0.0 | 0.0 |
| adversarial | lru | 508.0 | 3200000.0 | 508.0 | 49920.0 | 66304.0 | 0.0 | 3129344.0 | 0.0 |
| adversarial | compression | 510.0 | 3200000.0 | 510.0 | 58139.0 | 66304.0 | 596294.0 | 1568768.0 | 0.0 |
| adversarial | aura | 255.0 | 2560000.0 | 255.0 | 62235.0 | 66304.0 | 596294.0 | 0.0 | 0.0 |
| adversarial | old_fallback | 255.0 | 2560000.0 | 255.0 | 62235.0 | 66304.0 | 596294.0 | 0.0 | 0.0 |
| adversarial | no_hysteresis | 255.0 | 2560000.0 | 255.0 | 62235.0 | 66304.0 | 596294.0 | 0.0 | 0.0 |
| adversarial | no_cooldown | 255.0 | 2560000.0 | 511.0 | 62235.0 | 66304.0 | 688198.0 | 0.0 | 0.0 |
| adversarial | no_confidence | 255.0 | 2560000.0 | 255.0 | 62235.0 | 66304.0 | 596294.0 | 0.0 | 0.0 |
| adversarial | no_cost | 510.0 | 3200000.0 | 510.0 | 58139.0 | 66304.0 | 596294.0 | 1568768.0 | 0.0 |
| adversarial | round_robin | 203.0 | 1520000.0 | 379.0 | 58184.0 | 66304.0 | 583193.0 | 0.0 | 0.0 |
| adversarial | legacy_params | 255.0 | 2560000.0 | 255.0 | 62720.0 | 66304.0 | 1021000.0 | 0.0 | 0.0 |
| unreliable | lru | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 0.0 |
| unreliable | compression | 30.0 | 5160000.0 | 53.0 | 62226.0 | 66304.0 | 38164.0 | 118784.0 | 0.0 |
| unreliable | aura | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 512.0 |
| unreliable | old_fallback | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 512.0 |
| unreliable | no_hysteresis | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 512.0 |
| unreliable | no_cooldown | 26.0 | 5120000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 217088.0 | 512.0 |
| unreliable | no_confidence | 15.33 | 5136666.67 | 28.0 | 63591.33 | 66304.0 | 41044.33 | 0.0 | 0.0 |
| unreliable | no_cost | 30.0 | 5160000.0 | 53.0 | 62208.0 | 66304.0 | 0.0 | 233472.0 | 512.0 |
| unreliable | round_robin | 277.33 | 513333.33 | 476.33 | 34901.33 | 66304.0 | 0.0 | 2127189.33 | 512.0 |
| unreliable | legacy_params | 0.0 | 5120000.0 | 0.0 | 66304.0 | 66304.0 | 0.0 | 0.0 | 512.0 |

CSV mentah270 baris, ringkasan90 baris. Restore latency, compression count, oscillation, backing, compressed resident dan event timing disimpan pada CSV, bukan dihilangkan. capacity_violations=0 pada event berhasil: seluruh transisi disaring dan postcondition shared budget diperiksa; ini bukan jumlah kandidat yang ditolak. capacity_rejections belum diinstrumentasi. Waktu event/probe adalah pengukuran Instant nyata, berbeda dari semua biaya simulasi.

## 14. ANALISIS FALLBACK

Unreliable: aura/fallback baru refault30, stall5160000 ns, migrasi53, physical62208 byte; original refault0, stall5120000 ns, migrasi0, physical66304. Fallback baru menurunkan okupansi4096 byte namun memperburuk refault dan stall pada model ini. Ini skenario yang tidak menguntungkan fallback baru. Budget tetap dihormati dan cooldown masih aktif.

## 15. ANALISIS COMPLEXITY

Clone transaksi tetap O(n) waktu/ruang, bounded64 grup. Dipertahankan karena refault dan placement dapat memutasi dua grup, engine dan banyak counter sebelum overflow ditemukan; journal belum divalidasi. Selector recency eksperimen O(n); keputusan3 tier O(1). Model baru tidak mengklaim peningkatan efisiensi transaksi. Duplikasi modul historis/baru menambah beban maintenance tetapi melindungi reproduksi.

## 16. VALIDASI MEMORY ACCOUNTING

Test shared_transition merekonsiliasi penggunaan tier dan memastikan kompresi mengurangi P. entropy_no_free_capacity menolak ekspansi pada initial penuh. Test metadata menolak overcommit; penjumlahan u64::MAX menghasilkan Overflow. Atomic error memeriksa snapshot identik. Sequence512×3 policy memeriksa determinisme dan invariant kapasitas. Scratch nol menolak migrasi. Budget metadata48 merupakan parameter model; profil simulator sendiri berukuran80 byte dan runtime clone tidak masuk P. Tidak mengklaim seluruh overhead allocator diketahui.

## 17. PEMBAHASAN TRADE-OFF

Pada entropy aura tidak bermigrasi dan stall5120000; baseline LRU memindahkan53 kali dengan stall5160000. Pada interactive LRU refault0, aura4. Perubahan shared model menghilangkan kapasitas gratis tetapi tidak otomatis menghasilkan kebijakan efektif. Kompresi terkalibrasi murah bukan bukti optimasi kernel. Threshold stall >75% physical budget dan external pressure dapat mendominasi hasil saat selector exogenous stagnan.

## 18. KETERBATASAN DAN ANCAMAN VALIDITAS

Kalibrasi menggunakan hanya zero/entropy dan satu benchmark historis; profil ukuran lain belum dipetakan. CPU proxy berasal dari wall-time codec. Trace sintetis, selector exogenous dan fixed order, tiga seed tanpa interval kepercayaan. Scratch dicatat terpisah sehingga total transien bisa melebihi budget permanen; backing cache diabaikan. Clone tetap; belum ada journal, pengukuran RSS atau coverage-guided fuzzing. CPU/I/O budget mengatur admission migrasi; akses mahal tetap dilayani dengan penalti stall. Tidak ada pengujian kernel nyata. Perbandingan A bukan isolasi kausal kapasitas saja.

## 19. KESIMPULAN

Shared accounting, perbandingan kandidat sama, fallback, eksperimen ablation dan pengujian deterministik selesai. Hasil menunjukkan kelemahan kebijakan dan ketidakpastian model; TASK005 selesai sebagai penelitian userspace tanpa klaim peningkatan produksi.

## 20. REKOMENDASI TASK 006

Validasi selector yang menjamin progres tanpa bias policy, pisahkan eksperimen accounting vs definisi stall, gunakan calibration multi-dataset dan baseline dengan fitur sama. Desain journal dua grup lalu uji equivalence terhadap clone sebelum mengganti transaksi. Validasi total peak termasuk scratch dan backing cache.

## 21. DAFTAR PUSTAKA

Sumber primer digunakan: placement.rs dan laporan TASK004; physical.rs dan examples/validation.rs; docs/benchmarks/task-003-summary.csv untuk kalibrasi. Seluruhnya tersedia pada repository. Tidak memakai referensi paper eksternal atau mengklaim validasi Linux kernel.

## 22. LAMPIRAN DAN BUKTI PENGUJIAN

Perintah: cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo check --workspace; cargo build --workspace --release; cargo build -p xynoos-aura --examples --release; target/release/examples/validation > docs/benchmarks/task-005-raw.csv; python3 scripts/summarize_task005.py. research historis dieksekusi ke task-005-legacy.csv. CSV mentah/ringkasan, quality log dan reproducibility.txt berada pada docs/benchmarks. Source baru physical.rs, validation.rs, skrip summary; lib.rs mengekspor physical; README/arsitektur/laporan diperbarui. Storage awal139 MiB, akhir153 MiB; tersedia17855725568 byte pada pengukuran. Tidak ada dependency baru, instalasi besar, commit/push, perubahan kernel/swap atau penghapusan artefak sebelumnya.
