# TASK 014 — Unified Linux System Intelligence

**Tanggal:** 15 November 2026 UTC
**Proyek:** Xynoos HyperCore
**Baseline:** `242ae9936efbaca3f11b5386c809018c71267fa6`
**Lingkup:** observer Linux userspace, pembacaan read-only
**Status teknis:** terpenuhi pada interface yang tersedia; hardware fisik yang tidak tersedia **NOT VERIFIED**.

## Ringkasan eksekutif

PressureIQ diperluas untuk membaca CPU, PSI, diskstats, cgroup v2 dan atribut
DRM, thermal serta power di samping memori TASK 013. Antarmuka `system snapshot`
dan `system sample` telah dijalankan pada Linux nyata dengan workload kecil yang
dibatasi. Median pengumpulan snapshot terpadu pada percobaan ini 662.375 ns;
angka ini bukan overhead pada laptop N5000 dan bukan bukti optimasi Linux.

## Arsitektur dan antarmuka

Modul `xynoos_pressureiq::system` memakai collector terpisah dari parser.
`SystemSnapshot` membawa waktu wall-clock dan monotonic, durasi serta `Result`
per sumber; snapshot memori lama tetap utuh termasuk kegagalan parsial. Tipe
CPU dan disk menyimpan counter dengan unit dan identitas eksplisit; atribut
sensor menyimpan sumber, teks mentah, unit dan hasil parsing numerik jika
berlaku. Fungsi delta menolak reset, overflow, identitas disk berbeda dan
interval nol. Pengumpulan terjadi secara berurutan, bukan atomik. Error dan
sumber tetap dapat diperiksa konsumen API; CSV ringkas menggunakan sel kosong
untuk nilai yang tidak tersedia tanpa merinci penyebabnya.

Perintah `system snapshot` menampilkan snapshot manusia, termasuk hasil per
sumber. `system sample --count 5 --interval-ms 200` mengeluarkan CSV; stdout
dapat diarahkan ke file. Perintah `pressure` lama dipertahankan. Batas sampling
1–60 dan interval 100–60000 ms. Model ini dapat menjadi masukan service atau
controller mendatang setelah menambahkan skema IPC berversi, freshness dan
penanganan error; TASK 014 tidak membuat controller atau memicu reclaim.

## Kemampuan dan satuan

| Sumber | Status live | Makna |
|---|---|---|
| `/proc/stat` | PASS | counter tick kumulatif; persen diperoleh dari delta total dan idle/iowait |
| PSI memory, CPU, I/O | PASS | `avg10` persen rata-rata; `total` mikrodetik kumulatif |
| meminfo, vmstat | PASS | byte meminfo setelah konversi; vmstat kumulatif |
| diskstats | PASS | sektor 512 byte; delta per identitas major/minor/device |
| cgroup v2 CPU, memori, I/O | PASS | `usage_usec`, `memory.current` byte, `rbytes/wbytes` kumulatif |
| DRM | PASS penemuan | card0 vendor 0x1414; utilisasi dan kemampuan decode NOT VERIFIED |
| thermal dan power | NOT VERIFIED | tidak ada thermal zone atau power supply yang terlihat |
| frekuensi CPU0 | NOT VERIFIED | cpufreq sysfs tidak tersedia |

Disk CSV memilih device diskstats pertama (pada eksperimen ini loop0), bukan
seluruh disk dan bukan aktivitas workload tertentu. `system snapshot` dan API
menyediakan seluruh daftar yang terbaca. Cgroup mengacu ke membership proses,
bukan keseluruhan host. Hitungan capability dalam CSV adalah jumlah entri,
bukan bukti counter dapat dibaca. Status dan rincian tersedia pada snapshot.

## Lingkungan dan validasi Linux

Linux 6.8.0-1070-azure x86_64, Rust 1.100.0, empat CPU logis pada `/proc/stat`;
cgroup v2 terlihat pada `/sys/fs/cgroup/init`. Ini VM Codespaces; namespace dan
pengukuran pada perangkat fisik dapat berbeda. Bukti machine-readable adalah
`docs/benchmarks/task-014-raw.csv`. Pada run workload, CPU cgroup bertambah dari
1.810.454.816 menjadi 1.810.727.743 µs dan write cgroup dari 9.369.694.208
menjadi 9.372.839.936 byte; CPU agregat antarsampel berkisar 6,329–25,926%.
Counter juga mencakup aktivitas lain di cgroup dan tidak dapat diatribusikan
pasti ke workload tersebut. Memory PSI some avg10 pada sampel workload bernilai
0%. Disk loop0 menunjukkan delta nol; ini tidak membantah aktivitas device lain.
Tidak ada pressure buatan, operasi kernel istimewa atau stress tak terbatas.

## Matriks pengujian

| Pengujian | Hasil |
|---|---|
| Parser CPU, zero interval/reset dan duplikasi | PASS |
| Disk identitas, sektor, reset dan input rusak | PASS |
| Counter cgroup, overflow, path traversal dan atribut numerik | PASS |
| Parser memori/PSI/vmstat dan pengujian lama | PASS |
| CLI output CSV dua sampel dan batas sampling | PASS |
| Enam quality gate serta `cargo doc --workspace --no-deps` | PASS |
| Telemetri live CPU, memori, I/O dan cgroup | PASS |
| GPU utilization/decode dan sensor thermal/baterai fisik | NOT VERIFIED |

`cargo test --workspace`: 69 unit/integration tests dan satu doctest lulus;
seluruh hasil dan perintah tersedia dalam `task-014-quality-gates.txt`.
Pengujian tidak membuktikan dukungan semua variasi kernel, namespace cgroup,
atau atribut sysfs. CSV lima run x lima sampel diperiksa skema, rentang persen,
interval, serta monotonisitas counter CPU cgroup dalam eksperimen.

## Metode dan hasil overhead

Release build; dua warm-up; empat run biasa dan satu run workload; tiap run
lima sampel dengan interval sleep 200 ms. Workload subprocess dibatasi CPU
150 ms, buffer 4 MiB, file sementara 1 MiB, timeout dan cleanup otomatis.
25 observasi snapshot memberikan median 662.375 ns. Median komponen memori
TASK 013 di dalam snapshot ini 145.882 ns; scope tersebut tidak sama dengan
snapshot terpadu. Angka TASK 013 historis diperoleh pada waktu berbeda dan
tidak cukup sebanding untuk menyimpulkan regresi. CSV berukuran 3.986 byte.

CPU collector pada empat run biasa 0,004313–0,004980 detik per run sekitar
0,806 detik wall time (sekitar 0,53–0,62% satu core jika dibagi wall time).
`wait4` melaporkan peak RSS 15.488–15.616 KiB, yang dapat memasukkan high-water
setelah fork sebelum exec. Polling `/proc/<pid>/status` pada proses collector
terpisah mengamati VmHWM 2.364 KiB, dengan keterbatasan polling. Angka ini
bukan estimasi RAM fisik eksklusif collector. Raw dan summary CSV serta skrip
`python3 scripts/task-014-measure.py` menyimpan konfigurasi dan pengukuran;
variabilitas host dan instrumen membatasi generalisasi.

## Keamanan, batas, dan kesiapan

Collector membaca procfs/sysfs saja, tanpa root, perubahan sysctl, swap,
cgroup atau hardware; tidak membaca nama proses, argumen atau environment.
Setiap file dibatasi 1 MiB, disk 4096 entri dan pemindaian direktori 256 entri;
scan yang terpotong tidak menghasilkan peringatan khusus. CSV tidak memuat
error detail dan tidak mengidentifikasi scope namespace secara otomatis. Atribut
thermal/power hardware nyata, multiple mount cgroup kompleks dan behavior pada
sistem non-Linux masih memerlukan verifikasi. Host dan cgroup bukan scope sama;
PSI bukan simulated stall, MemAvailable bukan okupansi AURA, vmstat refault
bukan synthetic refault dan swap counters bukan simulasi backing migration.
Tidak ada klaim validasi simulator kernel atau peningkatan performa produksi.

TASK 015 dapat memulai desain kontrak IPC dan evaluasi pada laptop fisik, dengan
prioritas status eksplisit pada CSV/IPC, pemilihan device I/O, peringatan scan
terpotong dan validasi sensor. Penggunaan sebagai controller otomatis belum
layak tanpa freshness, kebijakan gagal aman dan validasi cakupan tambahan.

## Referensi

- Linux kernel, [Pressure Stall Information](https://docs.kernel.org/accounting/psi.html).
- Linux kernel, [procfs](https://docs.kernel.org/filesystems/proc.html).
- Linux kernel, [I/O statistics](https://docs.kernel.org/admin-guide/iostats.html).
- Linux kernel, [cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html).
- Source repository dan artefak TASK 013–014.

## Lampiran: perubahan dan bukti

Source: `crates/xynoos-pressureiq/src/system.rs`, `src/lib.rs`,
`crates/xynoos-cli/src/main.rs`, `tests/pressure.rs`. Dokumentasi: `README.md`,
`docs/ARCHITECTURE.md`, laporan ini. Pengukuran: `scripts/task-014-measure.py`,
`docs/benchmarks/task-014-{raw.csv,summary.csv,quality-gates.txt,validation.txt,reproducibility.txt,availability.md}`.
Baseline Git tercantum di awal; identitas commit TASK 014 dan PR mengikuti status
Git setelah publikasi, bukan dibuat-buat di dalam laporan. Storage workspace
sebelum pekerjaan sekitar 301 MiB, setelah build sekitar 321 MiB; filesystem
menunjukkan sekitar 17 GiB tersedia. File lokal `ss` tidak diubah.
