## 1. HALAMAN IDENTITAS

Xynoos HyperCore — TASK 013: PressureIQ v1 Linux Memory Pressure Telemetry. Tanggal 15 November 2026; baseline e793f37c51b862270f2028b2d87bea2702fde570; milestone observer userspace 0.1.0. Status teknis: SELESAI, status GitHub diverifikasi secara terpisah.

## 2. ABSTRAK

PressureIQ membaca PSI memori, meminfo dan vmstat secara read-only melalui parser yang terpisah dari I/O. Snapshot parsial dan delta berbasis monotonic tersedia; CLI melakukan sampling terbatas. Lima sampel live dan tiga fixture parser diukur; 65 unit/integration test dan seluruh quality gate lulus. Data mencerminkan procfs lingkungan Codespaces, bukan validasi simulator atau klaim penghematan memori.

## 3. PENDAHULUAN

Masalah: simulator AURA memakai tekanan dan refault sintetis tanpa observasi Linux. Tujuan: fondasi observasi yang aman dan dapat diuji. Lingkup hanya tiga sumber procfs, tidak ada kontrol kernel atau kebijakan migrasi.

## 4. LANDASAN TEKNIS

Dokumentasi PSI kernel mendefinisikan some sebagai waktu setidaknya sebagian task terhambat, full sebagai semua task non-idle terhambat, average sebagai persentase dan total sebagai mikrodetik. MemAvailable berbeda dari MemFree. Counter vmstat kumulatif membutuhkan selisih dan interval. Tidak ada kesetaraan langsung dengan simulator.

## 5. AUDIT TASK 012

Workspace memakai members crates/*; core menyediakan tipe simulasi, AURA memiliki policy sintetis tanpa kolektor, CLI tanpa argumen hanya mendemonstrasikan HyperMemory. Crate baru menghindari coupling dan dependency cycle. Git main e793f37 diverifikasi; file lokal ss dipertahankan. Cargo workspace wildcard otomatis mencakup crate baru tanpa perubahan konfigurasi.

## 6. ARSITEKTUR PRESSUREIQ

ProcFS → read → parse → Source<Result<T,Error>> → Snapshot → snapshot_delta atau CLI CSV. xynoos-pressureiq hanya bergantung standard library; CLI bergantung path pada crate tersebut. Tidak ada sambungan ke AURA.

## 7. PERANCANGAN COLLECTOR

collect membaca /proc/pressure/memory, /proc/meminfo, /proc/vmstat secara berurutan; collect_from menerima root fixture. File::open dan Read saja; input dibatasi 1 MiB per sumber. Tidak menulis trigger, sysctl, cgroup atau swap. Error Unsupported, Permission, Io, Malformed, Missing, Duplicate, Overflow dan Bounds eksplisit. Source mencatat waktu read dan parse masing-masing.

## 8. MODEL PSI

Psi.some wajib, Psi.full Option. Field avg10/60/300 f64 divalidasi finite dan 0..100, termasuk penolakan nilai negatif; total u64 mikrodetik. Urutan tidak penting, unknown field diabaikan, duplikat field/baris ditolak. Angka rata-rata adalah persentase, bukan byte.

## 9. MODEL MEMINFO

MemInfo menyimpan field yang dikenali sebagai map byte. MemTotal wajib, field lain Option melalui lookup. Satuan procfs kB dikali 1024 dengan checked_mul; satuan lain dan overflow ditolak. MemAvailable digunakan sebagai nilai kernel apa adanya, bukan rumus dari MemFree.

## 10. MODEL VMSTAT

Counter u64 meliputi pgfault, pgmajfault, pswpin/out, pgscan_*, pgsteal_*, workingset_refault_*; prefix lengkap dipertahankan. Counter yang tidak tersedia tidak diberi nilai nol. Delta hanya untuk key yang ada pada kedua snapshot; pengurangan checked, reset mengembalikan CounterReset. Delta belum menghasilkan rate otomatis.

## 11. TYPED SNAPSHOT

Snapshot mencatat root sumber, SystemTime, Instant dan tiap Source dengan hasil serta durasi. partial menandakan ada sumber error; full PSI dan field meminfo opsional dapat None tanpa error. snapshot_delta menggunakan interval monotonic positif, menolak root berbeda dan mendeteksi reset VMStat serta PSI; sumber yang gagal menghasilkan None. Tiga pembacaan bukan snapshot atomik kernel.

## 12. SAMPLING ENGINE

sample menerima count 1..60 dan interval 100 ms..60 s; default CLI 10 sampel dengan jeda 1000 ms. Thread sleep digunakan, bukan busy loop. Jeda dimulai setelah pengambilan sebelumnya sehingga ada drift waktu koleksi; total pengumpulan tetap bounded. Tidak ada daemon.

## 13. IMPLEMENTASI CLI

`cargo run -p xynoos-cli -- pressure snapshot` menghasilkan CSV satu snapshot; `pressure snapshot --human` ringkasan manusia. `pressure sample --interval-ms 1000 --count 30 --output pressure.csv` membuat file baru tanpa overwrite. Output diarahkan menjauhi /proc, /sys dan /dev; no-argument tetap demo lama. Error sumber parsial ditampilkan pada stderr dan status CSV, tanpa mengubah sumber lain.

## 14. METODOLOGI PENGUJIAN

Linux 6.8.0-1070-azure x86_64, rustc 1.100.0, Codespaces. Fixture sintetik memeriksa PSI normal dan missing/duplicate/NaN/infinity/negatif/overflow, meminfo optional/unit/overflow, vmstat prefix/delta/reset, unsupported root, CSV dan batas sampling. CLI diuji untuk error opsi, snapshot CSV/manusia, output 2 sampel dan no-overwrite. Data live 5 sampel, sleep 200 ms, tanpa tekanan buatan; parser fixture 10.000 iterasi per sumber, dua eksekusi case untuk cek struktur reproduksi.

## 15. HASIL UNIT TEST

65 unit/integration test lulus: 41 AURA, 13 HyperMemory, 7 PressureIQ dan 4 CLI; satu compile-fail doctest lulus. Nilai waktu live dan fixture tidak diharapkan identik; source dan schema fixture divalidasi.

## 16. HASIL QUALITY GATES

fmt --check, clippy workspace/all-targets -D warnings, test workspace, check workspace, build workspace release dan build seluruh examples release seluruhnya lulus. Log task-013-quality-gates.txt. Benchmark eksplorasi awal menemukan collision nama example `research` antarcrate; example PressureIQ diberi nama unik dan seluruh gate serta benchmark diulang.

## 17. HASIL LIVE TELEMETRY

Seluruh lima snapshot tersedia dan tidak parsial. Sampel pertama dan terakhir: PSI some/full avg10 0/0%; total some 10051489 µs pada awal dan 10051489 µs pada akhir. MemTotal 16770469888 byte; MemAvailable 14015090688 hingga 14017646592 byte. pgfault 106039634 hingga 106058730; pgmajfault 16212 hingga 16212; pswpin/out 0/0. Nilai kumulatif dan interval aktual tidak ditafsirkan sebagai laju otomatis.

## 18. HASIL BENCHMARK OVERHEAD

| Metrik | Nilai | Satuan | Lingkup |
|---|---:|---|---|
| read_psi_ns | 27582 | ns | live |
| parse_psi_ns | 4107 | ns | live |
| read_mem_ns | 32591 | ns | live |
| parse_mem_ns | 6993 | ns | live |
| read_vm_ns | 62897 | ns | live |
| parse_vm_ns | 19566 | ns | live |
| snapshot_ns | 170498 | ns | live |
| fixture_psi | 610 | ns_per_parse | syntetis |
| fixture_mem | 189 | ns_per_parse | syntetis |
| fixture_vm | 239 | ns_per_parse | syntetis |
| cpu_total | 2180000.0 | ns | seluruh_proses |
| peak_rss | 15360 | KiB | seluruh_proses |
| wall_total | 803013457 | ns | seluruh_proses |
| csv_size | 904 | byte | live |

Read/parse/snapshot adalah median lima sampel live. Fixture adalah rata-rata integer satu batch 10.000 parse atas input singkat, bukan pengukuran live atau distribusi. CPU time dan peak RSS adalah seluruh subprocess; peak RSS dapat mencakup pre-exec, fixture bukan bagian sampel live. Wall time 803 ms mencakup empat kali sleep 200 ms, startup dan koleksi. Ukuran output 904 byte pada run ini. Tidak ada instrumentation allocator; alokasi belum diukur.

## 19. PEMETAAN METRIK LINUX KE SIMULATOR

PSI some/full adalah waktu stall task agregat, bukan penalty_ns yang ditetapkan simulator. MemAvailable adalah estimasi kernel, bukan physical_budget atau occupancy simulator. workingset_refault_* adalah counter kernel bergantung versi dan tipe halaman, bukan jumlah event simulated refault. pswpin/out adalah counter aktivitas swap, bukan byte atau hitungan migrasi backing virtual simulator. Dibutuhkan eksperimen penyetaraan dan kalibrasi sebelum perbandingan.

## 20. ANALISIS KEAMANAN DAN PRIVASI

Collector tidak membutuhkan root dan tidak membuka sumber dengan write. Tidak membaca nama proses, argumen proses, environment atau token. Output berisi counter agregat dan timestamp yang tetap dapat mengungkap kondisi host; pembagian data harus memperhatikan konteks. Uji dan benchmark tidak membangkitkan memory pressure.

## 21. KETERBATASAN DAN ANCAMAN VALIDITAS

Procfs bisa tidak tersedia atau cakupannya berbeda di container; hasil Codespaces bukan data laptop N5000. Nilai PSI nol tetap nilai observasi, bukan kegagalan. `full` dan field meminfo optional; detil fitur berubah antarkernel. Pembacaan berurutan menimbulkan skew; sample sleep drift. CSV CLI hanya mengekspor subset counter VMStat, lengkapnya tersedia pada API. Tidak ada CPU/IO PSI, daemon, persistensi otomatis, automatic rate, heap attribution atau integrasi AURA. RSS dan CPU bukan biaya parser terisolasi; lima sampel dan fixture kecil tidak membuktikan performa universal.

## 22. KESIMPULAN

Observer read-only, parser, snapshot, delta dan sampling teruji. Hasil live memperlihatkan availability sumber pada lingkungan uji. Tidak ada bukti dampak terhadap reclaim, kecepatan atau RAM sistem karena kebijakan host tidak diubah.

## 23. REKOMENDASI TASK 014

Evaluasi cgroup-aware telemetry dan sumber kernel lebih beragam; tambahkan ekspor delta/rate yang menyatakan interval dan reset, pengukuran overhead multi-proses, serta validasi korelasi terhadap workload sintetis terkendali tanpa memodifikasi host. Kembangkan integrasi AURA hanya setelah semantik dan validitasnya dibuktikan.

## 24. DAFTAR PUSTAKA

1. Linux kernel PSI: https://docs.kernel.org/accounting/psi.html — semantik some/full, average dan total.
2. Linux kernel procfs: https://docs.kernel.org/filesystems/proc.html — antarmuka dan meminfo.
3. Source `crates/xynoos-pressureiq` dan data TASK013 pada repository ini.

## 25. LAMPIRAN DAN BUKTI

Perintah: cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo check --workspace; cargo build --workspace --release; cargo build --workspace --examples --release; python3 scripts/benchmark_task013.py. Artefak: task-013-raw.csv (live), task-013-fixture.csv (sintetik), task-013-summary.csv, task-013-quality-gates.txt, task-013-reproducibility.txt dan task-013-telemetry-availability.md. Perubahan: crate PressureIQ, CLI/tests, Cargo.lock, README, ARCHITECTURE dan artefak. Storage awal 231 MiB dan akhir sekitar 301 MiB; ruang tersedia 17.692.618.752 byte pada pengukuran. Target build diabaikan Git; ss dipertahankan. Commit dan PR dicatat GitHub.
