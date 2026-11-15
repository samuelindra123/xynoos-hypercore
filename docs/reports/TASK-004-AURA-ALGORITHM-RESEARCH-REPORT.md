# HALAMAN IDENTITAS

**Judul:** Model biaya AURA dan simulasi penempatan/refault deterministik  
**Proyek:** Xynoos HyperCore — Xynoos APEX  
**Task:** 004 — AURA Algorithm Research  
**Tanggal:** 15 November 2026  
**Milestone:** riset userspace 0.1.0  
**Status:** SELESAI

## ABSTRAK

AURA sebelumnya hanya menyediakan pemilihan tier berdasarkan tekanan. Pengembangan ini membangun model biaya integer, mesin keputusan, dan simulator tiga tier yang dapat dievaluasi terhadap LRU sederhana dan compression-first. Eksperimen meliputi sembilan trace, tiga seed, 512 event per kombinasi, dengan kapasitas dan biaya yang sama antar kebijakan. Dua eksekusi identik pada seluruh metrik simulasi; 24 unit test serta lima quality gate lulus. AURA mengurangi restore latency simulasi pada sejumlah trace, tetapi stall lebih tinggi pada entropy dan fallback prediksi tidak andal. Ini hasil model sintetis, bukan pengukuran reclaim kernel atau peningkatan responsivitas perangkat.

## 1. PENDAHULUAN

Rumusan masalah meliputi pemilihan tier yang menghormati kapasitas, biaya prediksi refault, pembatasan migrasi dan penanganan ketidakpastian. Tujuannya menghasilkan implementasi yang dapat diaudit dan diuji, serta mengungkap kelemahan kebijakan. Ruang lingkup adalah simulator metadata kelompok halaman, bukan penyimpanan data halaman nyata, reclaim kernel, atau sistem swap.

## 2. TINJAUAN TEKNIS

Penempatan tier menukar okupansi memori dengan biaya pemulihan. Refault dalam penelitian ini berarti akses pada grup yang saat akses bukan RAM; ini tidak sama persis dengan counter refault Linux. Hysteresis menggunakan ambang masuk/keluar berbeda, cooldown menahan migrasi berulang, dan confidence threshold menyediakan abstention/fallback. Semua konsep diterapkan eksplisit dalam kode lokal. LRU adalah baseline recency paling sederhana, tidak mewakili MGLRU atau mekanisme Linux lainnya.

## 3. AUDIT IMPLEMENTASI SEBELUMNYA

Audit membaca status Git, manifest workspace dan kelima crate, API core/AURA, antarmuka dan checked arithmetic HyperMemory, README, arsitektur, laporan TASK 003, serta CSV benchmark. Git bersih pada awal audit; perubahan lama telah tersedia dalam repository. Tidak ada source atau laporan TASK 001–003 dihapus.

AURA lama hanya memetakan Low ke Memory dan tekanan lain ke Compressed, tanpa kapasitas, biaya, atau transisi. HyperMemory menyediakan data lossless dan budget kapasitas yang dihitung, tetapi overhead allocator/RSS tidak terisolasi. CSV TASK 003 menunjukkan ekspansi entropy oleh LZ4 dan penalti percobaan kompresi; angka latency hot path tidak langsung menjadi latency reclaim. Oleh karena itu TASK 004 memakai biaya sintetis yang dinyatakan secara terbuka, tidak mengklaim kalibrasi hardware berdasarkan CSV tersebut. Risiko utama ialah ketidaktepatan prediksi, double accounting pada perpindahan, overflow dan thrashing. API lama dipertahankan; modul baru independen menghindari regresi library penyimpanan.

## 4. MODEL MATEMATIS AURA

Semua biaya model adalah nanodetik ekuivalen. Byte dikonversi memakai bobot kapasitas ns/byte, sementara probabilitas dan tekanan menggunakan integer permille; perkalian dibagi 1000 dengan pembulatan ke bawah.

Untuk kandidat tier t, q adalah refault_permille/1000 dan h adalah pressure_permille/1000:

`C(t) = q*restore[t] + CPU(t) + IO(t) + h*bytes[t]*capacity_weight + refault_risk(t) + stall_risk(t)`.

CPU compressed = biaya compress jika berpindah ke compressed ditambah q×decompress. IO backing = restore[backing] untuk model biaya tulis jika berpindah ke backing (asumsi latensi tulis sama dengan baca). Capacity cost backing nol. Refault risk = q×refault_penalty untuk tier non-RAM. Stall risk = stall_penalty untuk kandidat RAM ketika tekanan ≥ ambang masuk. Ini skor keputusan, bukan pencatatan durasi event; CPU dan latency saling berkaitan secara fisik namun dipakai sebagai dua preferensi biaya eksplisit, bukan dijumlahkan sebagai klaim waktu wall-clock nyata.

AURA menambah prior refault hingga 200 permille berdasarkan min(accesses,20)×10 bila recency tidak melebihi cooldown. Ini heuristik sederhana, bukan estimasi probabilitas yang telah dikalibrasi. Cost publik menghitung profil yang diberikan; Engine memakai profil hasil heuristik itu. Confidence <600 menghasilkan Fallback. Tidak memakai float sehingga NaN/Infinity tidak dapat direpresentasikan. Checked arithmetic mengembalikan Error::Overflow; input permille di luar 0..1000 ditolak.

Default: enter=700, exit=500, cooldown=4 tick, rate limit=2 migrasi/tick, maksimum=512 migrasi/eksperimen, bobot kapasitas=4 ns/byte, penalti refault=5000 ns dan stall=10000 ns. Nilai tersebut parameter penelitian, bukan hasil karakterisasi hardware.

## 5. ARSITEKTUR SIMULATOR

SystemState mencakup kapasitas/penggunaan [RAM,compressed,backing], CPU budget ns/tick, I/O budget byte/tick, restore latency, tekanan dan emergency. PageGroupProfile mencakup identitas, ukuran raw/compressed, jumlah akses, last-access, probabilitas dan confidence, biaya codec, tier, eligibility dan last-migration. Tier adalah enum sehingga nilai tier yang tidak dikenal tidak dapat dibuat melalui API safe.

Satu event melayani akses, mencatat refault/biaya, mencoba promosi jika feasible, lalu mengevaluasi satu kandidat cold. AURA dan compression-first memakai round-robin, LRU memilih grup RAM dengan last-access terkecil (tie berdasarkan ID). Akses non-RAM dapat dilayani di tempat jika RAM/budget tidak memungkinkan promosi. Tekanan eksternal adalah input trace; stall tambahan 10000 ns dicatat ketika tekanan tinggi dan RAM terisi >75%. Ini model penalti, bukan blocking thread nyata.

CPU/I/O layanan akses mengurangi budget yang tersedia untuk keputusan migrasi pada tick tersebut; budget dikembalikan untuk tick berikutnya. Akses yang lebih mahal tetap dilayani dengan penalti stall, sehingga budget adalah pengendali admission migrasi, bukan hard limit CPU/I/O keseluruhan. Backing sepenuhnya virtual; tidak ada operasi disk, swap, atau page-fault Linux.

## 6. DESAIN ALGORITMA DAN STRUKTUR DATA

Engine mengevaluasi tiga kandidat dengan tie-break mempertahankan tier saat ini. Kapasitas, eligibility, CPU dan I/O diperiksa sebelum perpindahan; count migrasi mencakup promosi dan placement. Hysteresis aktif pada tekanan ≥700 dan berhenti ≤500. Cooldown berlaku pada perpindahan dua arah. Kebijakan tidak memindai seluruh grup untuk AURA; simulator membatasi jumlah grup pada 64. Engine.decide menggunakan snapshot kecil untuk rollback saat error; Simulator.event memakai clone metadata untuk transaksi penuh. Pengurangan okupansi checked dan penambahan checked mencegah state parsial. Fallback mempertahankan kandidat; emergency juga menghentikan promosi. Risiko tidak terselesaikannya tekanan sengaja terlihat dalam metrik.

## 7. IMPLEMENTASI

File baru `crates/xynoos-aura/src/placement.rs`: SystemState, PageGroupProfile, Config, Cost, Decision, Engine, Simulator, Metrics dan fixture eksperimen. `src/lib.rs` mengekspor modul baru tanpa mengubah trait baseline lama. `examples/research.rs` menghasilkan eksperimen CSV release. `scripts/summarize_task004.py` memvalidasi kapasitas, jumlah baris, batas migrasi dan membuat rata-rata tiga seed. README dan ARCHITECTURE diperbarui. Tidak ada dependency baru.

## 8. METODOLOGI PENGUJIAN

Lingkungan: Ubuntu 24.04 Codespaces, Linux 6.8.0-1070-azure x86_64, 4 vCPU AMD EPYC, rustc 1.100.0. Fixture memuat 16 grup ×4096 byte pada RAM awal 65536 byte; kapasitas compressed 16384 dan backing 131072 byte. Ukuran compressed 512 untuk ID genap dan 4114 untuk ganjil. CPU budget 10000 ns/tick, I/O 8192 byte/tick; restore RAM/comp/backing =0/1000/100000 ns, compress=2000 dan decompress=3000 ns. Prior refault 900 untuk ID<4 dan 100 untuk sisanya, confidence default900. Grup eligible seluruhnya.

Trace deterministik 512 event dengan seed xorshift64 1,42,2026: interactive mengakses empat grup; background akses uniform 16 grup (proksi akses rendah reuse lokal, bukan idle time nyata); switching berpindah blok empat grup tiap32 event; pressure naik 0..1000; entropy semua ukuran compressed4114; compressible semua512; bursty 24 dari32 akses ke grup0; adversarial bergilir16 grup dengan tekanan690/710; unreliable memakai confidence100. Selain pressure/adversarial tekanan800. Semua policy menerima trace dan initial state identik untuk setiap seed/skenario. Profil priors dapat salah untuk trace tertentu; tidak disetel ulang untuk memenangkan AURA.

Dijalankan 81 kombinasi atau 41472 event. Dua eksekusi akhir membandingkan seluruh kolom non-timing secara persis. Dataset dibuat di luar timer. Event_execution_ns adalah jumlah waktu Instant di sekitar event (termasuk clone, accounting dan pemilihan), bukan waktu keputusan saja. Decision_probe_ns adalah panggilan tambahan pada salinan Engine setelah event terhadap profil yang baru diakses; probe tidak mengubah simulator dan dapat menghasilkan Keep/Defer berbeda dari keputusan kandidat. Karena itu probe adalah microbenchmark representative, bukan isolasi waktu keputusan aktual di event. Timer beresolusi terbatas, urutan policy tetap, tanpa warm-up/statistical confidence; tidak ada klaim peringkat kinerja nyata dari nanodetik tersebut.

## 9. HASIL CORRECTNESS

24 unit test workspace lulus: 11 AURA (termasuk baseline lama) dan 13 HyperMemory. Test AURA mencakup perhitungan cost tepat, overflow, determinisme semua policy, rekonsiliasi okupansi tiap event, batas migrasi, kapasitas, rollback invalid index/pressure/clock, hysteresis, cooldown, budget CPU/I/O nol, rate limit, max migrasi, confidence fallback, emergency, refault dan duplicate ID/accounting awal. Test generatif adversarial 1024 event memeriksa maksimum8 migrasi. Kasus cost overflow tidak mengubah Engine. Tidak ada framework property-testing baru; pengujian sequence deterministik dibatasi. Belum ada coverage-guided fuzzing atau bukti formal.

Lima quality gate dan build example release lulus, exit0: fmt --check; clippy workspace/all-targets dengan -D warnings; test workspace; check workspace; build workspace release. Log penuh: docs/benchmarks/task-004-quality-gates.txt.

## 10. HASIL BENCHMARK DAN SIMULASI

Tabel berikut adalah rata-rata tiga seed, seluruh nilai selain waktu eksekusi adalah hasil simulasi. Pecahan count muncul akibat rata-rata, bukan event parsial.

| Trace | Kebijakan | Refault | Restore ns | Stall ns | Migrasi | Kompresi | CPU ns | I/O byte | Fallback |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| interactive | Lru | 0.0 | 0.0 | 60000.0 | 12.0 | 0.0 | 0.0 | 49152.0 | 0.0 |
| interactive | CompressionFirst | 254.0 | 13304000.0 | 1300000.0 | 267.0 | 70.0 | 518000.0 | 811008.0 | 0.0 |
| interactive | Aura | 64.0 | 256000.0 | 370000.0 | 74.0 | 42.0 | 276000.0 | 0.0 | 0.0 |
| background | Lru | 392.33 | 39233333.33 | 1483333.33 | 512.0 | 0.0 | 0.0 | 2680149.33 | 0.0 |
| background | CompressionFirst | 320.67 | 17058666.67 | 723333.33 | 512.0 | 128.0 | 725000.0 | 1216512.0 | 0.0 |
| background | Aura | 243.33 | 973333.33 | 460000.0 | 413.0 | 210.67 | 1151333.33 | 0.0 | 0.0 |
| switching | Lru | 60.0 | 6000000.0 | 60000.0 | 132.0 | 0.0 | 0.0 | 540672.0 | 0.0 |
| switching | CompressionFirst | 242.0 | 12680000.0 | 1020000.0 | 302.0 | 79.0 | 518000.0 | 827392.0 | 0.0 |
| switching | Aura | 96.0 | 384000.0 | 300000.0 | 152.0 | 81.0 | 450000.0 | 0.0 | 0.0 |
| pressure | Lru | 120.67 | 12066666.67 | 236666.67 | 213.0 | 0.0 | 0.0 | 957098.67 | 0.0 |
| pressure | CompressionFirst | 98.0 | 5000000.0 | 193333.33 | 174.0 | 46.0 | 242000.0 | 386389.33 | 0.0 |
| pressure | Aura | 76.33 | 305333.33 | 166666.67 | 133.67 | 71.0 | 371000.0 | 0.0 | 0.0 |
| entropy | Lru | 392.33 | 39233333.33 | 1483333.33 | 512.0 | 0.0 | 0.0 | 2680149.33 | 0.0 |
| entropy | CompressionFirst | 320.67 | 32066666.67 | 723333.33 | 512.0 | 0.0 | 0.0 | 2381141.33 | 0.0 |
| entropy | Aura | 87.67 | 350666.67 | 5250000.0 | 152.33 | 77.67 | 418333.33 | 0.0 | 0.0 |
| compressible | Lru | 392.33 | 39233333.33 | 1483333.33 | 512.0 | 0.0 | 0.0 | 2680149.33 | 0.0 |
| compressible | CompressionFirst | 320.67 | 1282666.67 | 723333.33 | 512.0 | 260.67 | 1483333.33 | 0.0 | 0.0 |
| compressible | Aura | 320.67 | 1282666.67 | 723333.33 | 512.0 | 260.67 | 1483333.33 | 0.0 | 0.0 |
| bursty | Lru | 128.33 | 12833333.33 | 220000.0 | 237.67 | 0.0 | 0.0 | 1039018.67 | 0.0 |
| bursty | CompressionFirst | 223.33 | 5629333.33 | 980000.0 | 267.33 | 86.67 | 695333.33 | 416426.67 | 0.0 |
| bursty | Aura | 131.67 | 526666.67 | 536666.67 | 174.67 | 91.67 | 578333.33 | 0.0 | 0.0 |
| adversarial | Lru | 448.0 | 44800000.0 | 2010000.0 | 512.0 | 0.0 | 0.0 | 2908160.0 | 0.0 |
| adversarial | CompressionFirst | 510.0 | 26520000.0 | 3040000.0 | 510.0 | 128.0 | 1021000.0 | 1568768.0 | 0.0 |
| adversarial | Aura | 255.0 | 1020000.0 | 2240000.0 | 255.0 | 128.0 | 1021000.0 | 0.0 | 0.0 |
| unreliable | Lru | 392.33 | 39233333.33 | 1483333.33 | 512.0 | 0.0 | 0.0 | 2680149.33 | 0.0 |
| unreliable | CompressionFirst | 320.67 | 17058666.67 | 723333.33 | 512.0 | 128.0 | 725000.0 | 1216512.0 | 0.0 |
| unreliable | Aura | 0.0 | 0.0 | 5120000.0 | 0.0 | 0.0 | 0.0 | 0.0 | 512.0 |

| Trace | Kebijakan | Peak RAM / compressed / backing byte | Waktu event nyata ns/event | Probe keputusan nyata ns/call |
|---|---|---:|---:|---:|
| interactive | Lru | 65536.0 / 0.0 / 49152.0 | 186.87 | 97.07 |
| interactive | CompressionFirst | 65536.0 / 4096.0 / 32768.0 | 156.23 | 56.74 |
| interactive | Aura | 65536.0 / 15926.0 / 0.0 | 152.66 | 66.95 |
| background | Lru | 65536.0 / 0.0 / 61440.0 | 211.59 | 40.18 |
| background | CompressionFirst | 65536.0 / 4096.0 / 31402.67 | 163.85 | 46.85 |
| background | Aura | 65536.0 / 15926.0 / 0.0 | 167.38 | 55.42 |
| switching | Lru | 65536.0 / 0.0 / 49152.0 | 185.12 | 83.08 |
| switching | CompressionFirst | 65536.0 / 4096.0 / 32768.0 | 169.17 | 61.51 |
| switching | Aura | 65536.0 / 15926.0 / 0.0 | 155.64 | 66.91 |
| pressure | Lru | 65536.0 / 0.0 / 61440.0 | 177.75 | 37.61 |
| pressure | CompressionFirst | 65536.0 / 3925.33 / 31402.67 | 121.92 | 38.79 |
| pressure | Aura | 65536.0 / 15926.0 / 0.0 | 120.98 | 41.20 |
| entropy | Lru | 65536.0 / 0.0 / 61440.0 | 195.78 | 39.66 |
| entropy | CompressionFirst | 65536.0 / 0.0 / 55978.67 | 166.92 | 45.22 |
| entropy | Aura | 65536.0 / 12342.0 / 0.0 | 160.14 | 63.42 |
| compressible | Lru | 65536.0 / 0.0 / 61440.0 | 225.79 | 44.30 |
| compressible | CompressionFirst | 65536.0 / 6997.33 / 0.0 | 215.63 | 54.30 |
| compressible | Aura | 65536.0 / 6997.33 / 0.0 | 203.69 | 54.92 |
| bursty | Lru | 65536.0 / 0.0 / 61440.0 | 266.11 | 85.63 |
| bursty | CompressionFirst | 65536.0 / 4096.0 / 32768.0 | 204.79 | 74.59 |
| bursty | Aura | 65536.0 / 15926.0 / 0.0 | 198.94 | 74.49 |
| adversarial | Lru | 65536.0 / 0.0 / 65536.0 | 152.80 | 35.18 |
| adversarial | CompressionFirst | 65536.0 / 4096.0 / 32768.0 | 159.99 | 44.51 |
| adversarial | Aura | 65536.0 / 4096.0 / 0.0 | 166.67 | 57.79 |
| unreliable | Lru | 65536.0 / 0.0 / 61440.0 | 198.46 | 40.10 |
| unreliable | CompressionFirst | 65536.0 / 4096.0 / 31402.67 | 163.89 | 46.72 |
| unreliable | Aura | 65536.0 / 0.0 / 0.0 | 100.25 | 34.42 |

Seluruh oscillation counter bernilai0; definisinya adalah promosi ulang dalam jendela cooldown, yang memang dilarang guard. Nilai nol bukan bukti tidak adanya thrashing di luar jendela. Migrasi total dan refault tetap tinggi pada beberapa trace. Peak RAM selalu65536 karena initial state penuh. CSV mentah mencantumkan semua peak dan counter per seed; ringkasan memuat mean, tanpa interval ketidakpastian.

## 11. PERBANDINGAN ALGORITMA BASELINE

Pada interactive, LRU menghasilkan0 refault sedangkan AURA64: pemilihan kandidat round-robin dan priors AURA tidak melindungi working set sebaik LRU pada skenario ini. Pada background AURA lebih rendah restore latency simulasi daripada LRU, dengan biaya CPU kompresi. Pada compressible AURA dan compression-first menghasilkan metrik simulasi yang sama. Pada entropy, AURA memiliki stall5250000 ns dibanding LRU1483333,33 dan compression-first723333,33; memindahkan halaman yang sedikit membesar ke tier compressed mandiri tidak mengatasi model tekanan RAM dengan baik. Pada unreliable, fallback512 kali menahan semua halaman sehingga stall5120000 ns, meskipun refault0. Mengoptimalkan refault saja dapat menyesatkan. Tidak ada dominasi tunggal atas seluruh metrik dan trace.

## 12. ANALISIS KOMPLEKSITAS

Cost dan Engine.decide O(1) waktu/ruang (tiga tier). Seleksi kandidat AURA/compression-first O(1), LRU O(n) scan bounded64. Transaksi Simulator.event menyalin Vec profil sehingga O(n) waktu dan ruang; bukan klaim hot path seluruh simulator O(1). Constructor memeriksa duplicate ID dengan O(n²), sekali pada setup. Profile berukuran80 byte pada x86_64, 1280 byte untuk16 grup sebelum capacity Vec, struktur simulator dan clone. Model tidak menyimpan data raw4KiB sungguhan. Tidak ada lock atau contention internal; single-thread. Skalabilitas melampaui64 grup sengaja ditolak; mengganti transaksi clone dan pemilihan kandidat adalah prasyarat skala lebih besar.

## 13. ANALISIS RISIKO DAN KEAMANAN

Semua eksekusi userspace, tidak menyentuh kernel/swap/device. Library melarang unsafe. Overflow dan transisi gagal mengembalikan Error dengan snapshot rollback. Field publik memungkinkan konfigurasi eksperimen; caller tetap bertanggung jawab menjaga invariant setelah mutasi langsung. Constructor memvalidasi initial accounting; state arbitrary yang dimutasi bukan jaminan model sah. Refault service dapat melampaui budget dan diberi stall, jadi budget bukan scheduler. Tidak ada akses data proses lain atau input file backing.

## 14. KETERBATASAN DAN ANCAMAN VALIDITAS

Tier compressed memiliki kapasitas terpisah dan tidak mengurangi kapasitas fisik RAM bersama; ini penyederhanaan signifikan. Byte metadata/allocator tidak ditambahkan ke kapasitas model. Tidak ada codec aktual, integrasi HyperMemory, I/O disk, topology hardware, concurrency, pengukuran PSI, atau kalibrasi biaya. Distribusi akses/hotness/confidence adalah asumsi. Prior refault bukan prediktor probabilitas tervalidasi. Hysteresis mengandalkan tekanan eksternal, tidak menghitungnya dari occupancy. LRU scanning memilih kandidat berbeda dari AURA; perbandingan mencakup pemilihan kandidat dan fungsi keputusan sekaligus. Max migrasi512 dapat membatasi efek thrashing. Kolom io_bytes adalah estimasi volume, bukan waktu I/O yang terukur. Probe keputusan berbeda konteks dari kandidat asli. Tidak ada benchmark MGLRU/zram/zswap/Windows atau hasil responsivitas laptop nyata.

## 15. KESIMPULAN

Mesin keputusan AURA, simulator tier, perlindungan thrashing, baseline dan eksperimen reproducible telah berjalan dan lulus pengujian. Pengamatan menunjukkan manfaat dan kegagalan dalam model yang sama. Target TASK 004 tercapai sebagai simulator riset, dengan batas model dan pengukuran dinyatakan eksplisit.

## 16. REKOMENDASI TASK 005

Prioritas berikutnya ialah kapasitas RAM fisik bersama untuk compressed, validasi prediksi refault, baseline kandidat yang sama, transaksi tanpa clone, dan kalibrasi biaya dari pengukuran terisolasi. Tambahkan trace real userspace anonim serta eksperimen ablation cooldown/cost/confidence dengan urutan benchmark berimbang. Rekomendasi bukan implementasi TASK 005 pada sesi ini.

## 17. DAFTAR PUSTAKA

1. Source primer model dan persamaan: crates/xynoos-aura/src/placement.rs pada repository ini.
2. Source trace dan konfigurasi eksperimen: crates/xynoos-aura/examples/research.rs.
3. Laporan TASK 003 dan docs/benchmarks/task-003-summary.csv; dasar observasi storage sebelumnya.
4. Bukti empiris TASK 004: CSV mentah, ringkasan dan quality log pada lampiran. Tidak menggunakan referensi paper eksternal untuk mengklaim model telah tervalidasi.

## 18. LAMPIRAN

Perintah reproduksi: `cargo run -p xynoos-aura --example research --release > docs/benchmarks/task-004-raw.csv`; `python3 scripts/summarize_task004.py`. Untuk pengujian: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; `cargo check --workspace`; `cargo build --workspace --release`.

Artefak: task-004-raw.csv (81 baris), task-004-summary.csv (27 baris), task-004-quality-gates.txt dan task-004-reproducibility.txt pada docs/benchmarks. Perbandingan dua eksekusi mengabaikan hanya event_execution_ns dan decision_probe_ns. Kedua kolom itu pengukuran nyata, seluruh kolom biaya lain simulasi, bukan karakteristik hardware empiris.

File berubah: lib.rs AURA, placement.rs, example research, skrip summary, README, ARCHITECTURE, laporan dan artefak TASK 004. Tidak ada dependency baru, commit/push, atau penghapusan hasil lama. Storage awal122 MiB, akhir sekitar139 MiB termasuk build debug/release; filesystem tersedia 17,873,707,008 byte pada pengukuran akhir. Angka filesystem dipengaruhi aktivitas lingkungan lain. Build artifacts diabaikan Git.
