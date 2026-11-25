# Arsitektur milestone 0.1

Alur data: caller → PageStore → LZ4 block → HashMap<PageId, Entry>. Pengambilan mendekompresi ke buffer baru; data asli tidak disimpan permanen. ID monoton per instance, tidak digunakan ulang, dan dapat habis. Penghapusan membebaskan buffer milik entri. Store tidak menyediakan persistensi atau sinkronisasi sendiri.

AURA menerima tekanan dan estimasi biaya melalui trait `DecisionPolicy`; baseline memilih memory saat tekanan rendah dan compressed pada tekanan sedang/tinggi, tanpa menggunakan estimasi biaya. Tier adalah representasi keputusan, bukan backend terintegrasi. Interface dapat diperluas untuk eksperimen HyperTier, RefaultSense, dan PageFusion; implementasinya belum ada.

Statistik adalah snapshot logis, bukan pengukuran RSS. Entry memuat Vec payload serta CRC32 IEEE payload dan data asli. Metadata adalah ukuran statis pasangan key dan Entry per entri, tidak termasuk bucket kosong dan overhead allocator. Operasi sementara dan kapasitas HashMap dapat menggunakan memori tambahan.


TASK 002 membatasi page_size pada 1..=1.048.576 byte. Konstruktor memvalidasi batas perkalian kapasitas dengan batas output codec dan metadata. `with_capacity` mengembalikan Result, sehingga caller TASK 001 perlu menambahkan `?` atau menangani error. Format internal berubah, tetapi belum ada format serialisasi atau persistensi yang perlu dimigrasikan.

Get memeriksa CRC payload sebelum alokasi, membuat output sebesar page_size konfigurasi privat, menjalankan decompress_into, memeriksa panjang tertulis dan CRC asli. Tidak ada panjang hasil yang diambil dari payload. Kegagalan get tidak mengubah store. Try-reserve menangani kegagalan reservasi map/output, tetapi alokasi codec sendiri tetap dapat abort pada OOM. ID hanya unik dalam satu store; u64::MAX tidak diterbitkan karena penghitung berikutnya harus tersedia.

Stats menambahkan kapasitas map (jumlah entri yang dapat ditampung) dan ukuran statis store. Overhead HashMap dalam byte dan allocator belum diukur; kapasitas map tidak boleh dianggap jumlah bucket fisik. RSS dan peak RSS tersedia melalui harness Linux, terpisah dari statistik logis.


## Penyimpanan adaptif TASK 003

`PageStore::adaptive(page_size, capacity, budget)` mengompresi terlebih dahulu. Bila kapasitas LZ4 lebih kecil dari panjang input, LZ4 pasti lebih kecil dari kapasitas kandidat raw dan raw tidak dialokasikan. Jika tidak, raw dialokasikan dengan `try_reserve_exact`; kapasitas kedua Vec dibandingkan, raw dipilih jika kapasitasnya kurang dari atau sama dengan LZ4. Kedua representasi memiliki struktur metadata yang sama. Kapasitas yang dipakai adalah kapasitas yang dilaporkan Vec; ukuran alokasi fisik allocator tidak diketahui. Metadata tetap dan kapasitas Vec masuk budget, dengan checked arithmetic sebelum commit. HashMap, buffer sementara, alokasi saat get, overhead allocator dan RSS tidak dibatasi budget. Kompresi terjadi sebelum pengecekan budget dan dapat gagal fatal pada OOM. `new` dan `with_capacity` tetap selalu LZ4.

CRC memeriksa payload raw maupun LZ4 dan hasil aslinya. Statistik `stored_bytes` adalah jumlah payload kedua representasi; `compressed_bytes` hanya untuk LZ4. `compression_ratio` adalah stored_bytes/original_bytes. Metadata entry, kapasitas map, payload length, dan kapasitas buffer bukan RSS.

## AURA placement TASK 004

API baseline lama dipertahankan. Modul `placement` menyediakan SystemState, PageGroupProfile, Config, Engine, Cost dan Simulator. RAM, compressed dan backing adalah tier logis mandiri; compressed tidak memakai HyperMemory atau codec nyata pada simulator ini. Kapasitas dan I/O dalam byte, CPU/restore dalam ns; probabilitas, confidence dan tekanan dalam permille 0..1000. Cost menyetarakan capacity dengan ns/byte yang dapat dikonfigurasi. Tidak ada floating point pada algoritma.

Satu event mengakses grup, menghitung refault bila bukan RAM dan mencoba promosi sesuai kapasitas/cooldown/budget migrasi, lalu mengevaluasi satu kandidat. AURA dan compression-first menggunakan kandidat round-robin; LRU memindai maksimal 64 grup untuk grup RAM paling lama diakses. LRU ini tidak mewakili MGLRU. Event dikloning untuk rollback error: O(n) waktu dan ruang, dibatasi 64 grup. Engine.decide sendiri O(1) dan clone O(1). Simulator single-thread tanpa lock. API state publik untuk eksperimen harus dipakai dengan invariant kapasitas/accounting yang valid; membuat Simulator baru adalah jalur validasi konfigurasi.

CPU/I/O budget berlaku per tick untuk penerimaan migrasi dan dikurangi biaya layanan akses sebelum keputusan kandidat. Akses yang melebihi budget tetap dilayani secara model dengan penalti stall; ini bukan scheduler CPU/I/O. Rate limit mencakup promosi dan keputusan kandidat, maksimum total migrasi mencakup keduanya. Fallback menahan kandidat, emergency juga mencegah promosi. Confidence rendah menahan keputusan AURA, bukan melarang akses halaman.

TASK005: modul physical mempertahankan simulator placement historis. Physical usage adalah RAM raw+compressed+metadata; metadata bookkeeping backing tetap ada. Scratch migrasi dibatasi terpisah dan bukan bagian budget permanen. event_candidate menerima kandidat eksternal identik untuk eksperimen fair. Fallback recency mematuhi guard yang sama. Transaksi clone tetap dipertahankan; kompleksitas event O(n), maksimum64 grup. Parameter terkalibrasi memakai zero/entropy TASK003; bukan biaya kernel. Lihat laporan TASK005 untuk batas model dan kegagalan fallback.

TASK006 completion: state_aware menilai maksimal64 preview event untuk seluruh kandidat; O(n²) karena event clone O(n). Feasibility target menggunakan fungsi yang sama dengan commit. Journal menyimpan dua profil dan state/counter konstan, default tetap clone. Peak total dicatat permanent sebelum migrasi + buffer tujuan saat sumber masih hidup. Stall override memungkinkan ablation kapasitas dan definisi stall secara terpisah. Feasible target tidak selalu dipilih policy; no-feasible dibedakan dari no-policy-migration.

TASK007 FastPath eksperimental memisahkan prepare_access sekali, inspect_prepared per kandidat dengan snapshot fixed-size, dan complete_candidate saat commit. Satu Vec clone pada prepare membuat total O(n) waktu/ruang; tidak ada replay akses per kandidat. Oracle step/state_aware tetap default. Differential test membandingkan seluruh state/error pada1/16/64grup. Sumber fase menyalin formula oracle, sehingga perubahan ke depan wajib mempertahankan equivalence test. Waktu inspeksi dan RSS proses dijelaskan laporan TASK007.

TASK008 mengevaluasi fixture valid 1–64 grup. Allocator wrapper hanya terdapat pada executable scalability dan meneruskan operasi ke System; kode library tidak menggunakan allocator tersebut. Pengukuran menghitung allocation traffic selama hot path, bukan peak live heap atau RSS spesifik store. Oracle tetap independen dan default karena duplikasi formula belum dapat dikonsolidasikan tanpa kehilangan referensi independen.

TASK009: heap tracker khusus executable memakai register pointer4096slot statis dan tidak mengalokasikan dalam callback. Requested-live scope dipisahkan dari fixture lama/RSS. Realloc dan dealloc lintas boundary dilacak menurut alamat. Fault hooks private sebelum publication menguji error dan unwind tanpa memutasi simulator awal; System OOM abort tetap di luar jaminan. Default tetap oracle.

TASK010: FastPath memakai try_clone_metadata: validasi 1..64 grup, try_reserve_exact, copy profil fixed-size dan clone field tanpa heap. AllocationReservation terpisah dari Capacity simulator; tidak membuat semua jalur OOM-safe. Tracker010 memegang lock saat begin, menolak scope overlap objek dan memiliki epoch global. Validasi empat thread adalah process-global terkontrol, bukan atribusi thread-local. Journal masih experimental dan tidak dijamin rollback panic.

TASK011: `Prepared` memiliki simulator privat dan hanya menyediakan inspeksi serta validasi consuming; `CommitReady` dibuat setelah placement valid dan memiliki metode publikasi consuming. Linearization point adalah assignment simulator pada publish, diikuti cursor tanpa operasi fallible. Journal in-place tetap experimental karena rollback hanya terjadi pada Result::Err, bukan panic-unwind. Default tetap oracle.

TASK012: `event_journal` menggunakan guard RAII yang menyimpan state, engine, metrics, tick dan kedua profil. Drop mengembalikan snapshot saat Err atau unwind sebelum commit; step tidak boleh memodifikasi grup lain atau panjang Vec. Cursor selector terpisah perlu di-stage caller. Abort tidak ditangani; journal tetap experimental dan default tetap clone.

TASK013: crate `xynoos-pressureiq` adalah observer read-only independen dari AURA. Collector membaca PSI memori, meminfo dan vmstat secara berurutan; parser fixture independen dari I/O; Snapshot membawa root, clock dan Result per sumber. SnapshotDelta memakai monotonic dan checked subtraction. Sampling dibatasi dan CSV tidak melakukan kontrol kernel, reclaim atau perubahan swap. Procfs container dapat mencerminkan cakupan host/namespace yang berbeda.

## PressureIQ: snapshot terpadu (TASK 014)

`xynoos_pressureiq::system::collect_system()` mengembalikan `SystemSnapshot`
yang memiliki timestamp wall-clock, `Instant`, durasi, `Snapshot` memori lama,
dan `Result` terpisah untuk CPU, PSI, disk serta cgroup. Pemanggil harus
memeriksa setiap hasil; sumber dibaca berurutan dan bukan snapshot atomik.
Pemakaian controller/IPC mendatang harus melakukan serialisasi dengan skema
versi dan mengekspos error, umur data, identitas sumber, serta batas cakupan;
TASK 014 tidak menambah daemon atau loop kontrol.

`/proc/stat` menghasilkan tick kumulatif delapan kategori pertama; delta
busy/(busy+idle+iowait) menghasilkan persen hanya jika dua sampel valid.
Diskstats menyatakan sektor Linux 512 byte; `disk_delta` memeriksa identitas,
reset dan overflow. CSV memilih device pertama sebagaimana diurutkan kernel,
bukan total semua disk dan bukan I/O aplikasi. `cpu_usage_us` dan I/O cgroup
adalah counter kumulatif; `memory.current` adalah byte saat pembacaan. CPU PSI,
I/O PSI dan memory PSI `some.avg10` adalah rata-rata persen 10 detik. Semua
pengukuran antar-sumber dapat mencerminkan namespace yang berbeda.

CSV `system sample` memiliki header nama dan satuan, kosong berarti tidak ada
nilai yang valid; baris pertama tidak memiliki delta. `elapsed_ms` memakai
monotonic clock. `snapshot_ns` mencakup seluruh koleksi terpadu dan
`memory_snapshot_ns` hanya kolektor TASK 013. `gpu_cards`, `thermal_sensors`
dan `power_supplies` adalah hitungan entri yang ditemukan, bukan indikasi
utilisasi atau dukungan. Antarmuka `system snapshot` dan Rust API menampilkan
error sumber; CSV ringkas tidak mengkodekan penyebab sel kosong. Atribut sysfs
berupa teks mentah: thermal `temp` adalah miliderajat Celsius, power `capacity`
persen, `power_now` mikrowatt, `energy_now` mikrowatt-jam, dan vendor DRM
berbentuk ID heksadesimal; atribut tidak tersedia tetap berupa error. GPU
utilisasi, decode dan akselerasi tidak disimpulkan dari keberadaan DRM.

Setiap file dibatasi 1 MiB, disk maksimum 4096 entri, dan pemindaian tiap
direktori maksimum 256 entri (hasil dapat terpotong pada sistem besar).
Sampling dibatasi 1–60 sampel dengan interval 100–60000 ms dan sleep antar
sampel; data disimpan maksimal dua snapshot dalam CLI. Read-only collector ini
tidak mengubah kernel, cgroup, swap atau kebijakan AURA.

## HyperMemory Milestone A & B: Correctness, Deterministic Eviction & Observability

Pembaruan HyperMemory melengkapi engine penyimpanan memori userspace dengan jaminan correctness, penggusuran terikat (bounded deterministic eviction), dan observabilitas operasional:

1. **Siklus Hidup Halaman Eksplisit (4-State Semantics)**:
   - `PageLifecycleState` memodelkan 4 status halaman secara terverifikasi:
     - `Active`: Halaman aktif dan valid di dalam store.
     - `Evicted`: Halaman pernah dibuat tetapi telah digusur (evicted) oleh kebijakan LRU/FIFO karena batas kapasitas/budget.
     - `Removed`: Halaman telah dihapus secara eksplisit oleh pemanggil via `remove()`.
     - `NeverCreated`: ID halaman belum pernah dialokasikan (`PageId >= next`).
   - `PageStore::contains(&self, id: PageId) -> bool` memeriksa keberadaan halaman secara O(1).
   - `PageStore::lifecycle_state(&self, id: PageId) -> PageLifecycleState` memberikan inspeksi status tanpa mutasi.

2. **Pengambilan Tanpa Alokasi (`get_into`)**:
   - `get_into(&self, id: PageId, out: &mut [u8]) -> Result<usize, Error>` melakukan dekompresi langsung ke buffer milik pemanggil.
   - Menghindari overhead alokasi `Vec<u8>` pada hot-path pembacaan.
   - Mengembalikan `Error::OutputBufferTooSmall` bila ukuran buffer pemanggil kurang dari `page_size`.
   - `get(&self, id: PageId)` tetap tersedia untuk backward compatibility dengan mengalokasikan buffer dan mendelegasikan ke `get_into`.

3. **Pembaruan Halaman Atomik (`update`)**:
   - `update(&mut self, id: PageId, data: &[u8]) -> Result<(), Error>` mengganti isi halaman secara in-place.
   - Melakukan pre-validasi tick exhaustion dan delta budget sebelum komit; jika budget terlampaui atau input tidak valid, modifikasi dibatalkan tanpa mengubah state yang ada (atomic rollback).
   - Menghitung ulang CRC32 payload dan original data serta memperbarui `last_accessed_tick`.

4. **Mesin Penggusuran Deterministik & Keamanan Transaksional (`EvictionPolicy`)**:
   - `EvictionPolicy::Reject`: perilaku default historis (mengembalikan `Error::Full` atau `Error::BudgetExceeded`).
   - `EvictionPolicy::Lru`: menggusur halaman yang paling lama tidak diakses (`last_accessed_tick` terendah; seri dipecahkan secara deterministik dengan `PageId.0` terendah).
   - `EvictionPolicy::Fifo`: menggusur halaman yang paling awal disisipkan (`inserted_tick` terendah; seri dipecahkan dengan `PageId.0`).
   - `insert_with_policy(&mut self, data: &[u8], policy: EvictionPolicy) -> Result<PageId, Error>`: menerapkan kontrak fail-closed transaction safety:
     - Melakukan pre-validasi kehabisan ID (`next.checked_add(1)`) dan Tick (`tick.checked_add(1)`).
     - Menghitung biaya kompresi kandidat dan memvalidasi `cost <= budget`.
     - Melakukan pre-reserve slot HashMap (`try_reserve(1)`) sebelum proses penggusuran.
     - Tidak ada halaman yang digusur apabila operasi penyisipan berpotensi gagal.
   - `evict(&mut self) -> Result<Option<PageId>, Error>`: API eksplisit untuk menggusur satu kandidat LRU.

5. **Observabilitas & Telemetri Operasional (`Telemetry`)**:
   - `telemetry(&self) -> Telemetry` mencatat metrik runtime ringan:
     - `hit_count`, `miss_count`, `eviction_count`, `insert_count`, `update_count`, `remove_count`, `corruption_count`.
     - `hit_ratio(&self) -> Option<f64>` menghitung rasio keberhasilan cache secara tepat.
   - `reset_telemetry(&self)` mereset counter operasional.
   - `peek(&self, id: PageId) -> Result<Vec<u8>, Error>` memungkinkan inspeksi diagnostik non-intrusif tanpa memajukan access tick atau menambah hit counter.
   - Struktur `Stats` tetap memegang akuntansi penyimpanan fisik murni (`stored_bytes`, `accounted_bytes`, `buffer_capacity_bytes`, `metadata_bytes_lower_bound`) menjaga invarian historis.


## HyperCore Milestone C: AURA Adaptive Memory Tiering

Milestone C mengintegrasikan engine telemetri Linux `PressureIQ`, mesin kebijakan `AURA`, dan penyimpanan fisik `HyperMemory` untuk mengorkestrasi migrasi tingkatan memori (*tiering*) userspace secara otomatis, adaptif, dan deterministik:

1. **Tingkat Tekanan Memori Terstruktur (`MemoryPressure`)**:
   - `MemoryPressure::Low`: Kapasitas memori longgar. Memprioritaskan throughput baca dan latensi terendah dengan mempertahankan halaman dalam `StorageTier::Memory` (raw byte buffer tanpa overhead dekompresi CPU).
   - `MemoryPressure::Moderate`: Tekanan mulai terdeteksi (PSI elevated atau memori bebas < 25%). Halaman yang sering/baru diakses (*hot*, latensi <= threshold) dipertahankan di `Memory`, sedangkan halaman jarang diakses (*cold*) dimigrasikan ke `StorageTier::Compressed`.
   - `MemoryPressure::High`: Tekanan tinggi (PSI sustained, direct reclaim aktif, atau memori bebas < 15%). Seluruh halaman aktif dimigrasikan ke `StorageTier::Compressed` untuk membebaskan ruang memori fisik.
   - `MemoryPressure::Critical`: Tekanan kritis (swap thrashing terdeteksi atau memori bebas < 5%). Seluruh halaman aktif dipaksa ke `StorageTier::Compressed` guna meminimalkan jejak memori userspace dan mencegah kernel OOM killing.

2. **Jembatan Telemetri Fail-Safe (`PressureBridge`)**:
   - `PressureBridge::evaluate_pressure(diagnosis: &Diagnosis) -> MemoryPressure` memetakan diagnosis sistem dari `PressureIQ`.
   - *Kontrak Fallback Telemetri*: Bila telemetri tidak andal (`diagnosis.reliable == false`) akibat pembacaan kedaluwarsa atau kegagalan pembacaan kernel PSI/cgroup, jembatan secara konservatif mengembalikan `MemoryPressure::Moderate`. Hal ini mencegah kepanikan sistem atau dekompresi berlebih saat kondisi sistem tidak dapat dipastikan.

3. **Migrasi Halaman In-Place & Transaksional (`migrate_page`)**:
   - `PageStore::migrate_page(&mut self, id: PageId, target: StorageTier) -> Result<bool, Error>`:
     - **Memory -> Compressed**: Mengompresi payload secara in-place via LZ4, memvalidasi CRC32, dan memutakhirkan akuntansi kapasitas (mengurangi `accounted_bytes`). Mengembalikan `Ok(true)` bila terjadi migrasi, atau `Ok(false)` bila halaman sudah di tier target.
     - **Compressed -> Memory**: Mendekompresi payload ke buffer sementara, memvalidasi CRC32 data asli, dan memeriksa batas budget (`accounted + diff <= budget`) sebelum komit (*fail-closed*). Jika budget terlampaui, halaman tetap berada di tier terkompresi tanpa perubahan data (*atomic rollback*).
   - `PageStore::page_tier(&self, id: PageId) -> Result<StorageTier, Error>`: Mengetahui status tier halaman saat ini (`Memory` atau `Compressed`) secara O(1).
   - `PageStore::page_cost_estimate(&self, id: PageId) -> Result<CostEstimate, Error>`: Menghitung estimasi latensi dan ukuran byte dengan penalti resensi akses (`recency_ticks`).
   - `PageStore::apply_tiering<F>(&mut self, decide: F) -> Result<TieringReport, Error>`: Menerapkan fungsi keputusan ke seluruh halaman aktif dengan pengurutan ID deterministik, menghasilkan `TieringReport`.

4. **Koordinator Tiering AURA (`AuraTieringCoordinator`)**:
   - Mengorkestrasi evaluasi `PressureIQ` dan eksekusi `HyperMemory` secara non-intrusif:
     - `step(&self, store: &mut PageStore, diagnosis: &Diagnosis) -> Result<TieringReport, Error>`
     - `step_with_pressure(&self, store: &mut PageStore, pressure: MemoryPressure) -> Result<TieringReport, Error>`


## HyperCore Milestone D: Adaptive Policy Reliability, Controlled Migration & Measurable Memory Efficiency

Milestone D memperkokoh keandalan (*reliability*), determinisme, dan efisiensi memori pada jembatan adaptif AURA dan HyperMemory dengan jaminan matematis dan proteksi kegagalan:

1. **Audit & Validasi Telemetri Ketat (`TelemetryValidation`)**:
   - `PressureBridge::validate_telemetry(diagnosis: &Diagnosis) -> TelemetryValidation`
   - Melindungi engine dari sinyal telemetri yang korup, palsu, atau tidak masuk akal secara fisik:
     - Penolakan nilai non-numerik (`NaN`, `Inf`) pada ketersediaan RAM (`available_pct`) dan interval sampling (`interval_seconds`).
     - Batasan domain persentase ketat (`0.0 <= available_pct <= 100.0`) dan interval positif (`interval_seconds > 0.0`).
     - Deteksi sinyal kontradiktif: penolakan status `"normal"` yang bersamaan dengan `"swap_thrashing"`, `"active_direct_reclaim"`, atau `"sustained_pressure"`; serta kombinasi mustahil seperti `"swap_thrashing"` dengan RAM > 50%.
   - Kontrak *fail-safe*: Setiap telemetri yang tidak valid atau kedaluwarsa secara deterministik dialihkan ke `MemoryPressure::Moderate` untuk mencegah dekompresi membabi buta atau kepanikan sistem.

2. **Mesin Status Histeresis Tekanan Memori (`PressureStabilizer`)**:
   - Menghilangkan *oscillation thrashing* akibat fluktuasi telemetri sesaat di batas ambang (*boundary conditions*).
   - *De-eskalasi Asimetris*: Transisi ke tingkat tekanan lebih rendah (misal `High -> Moderate` atau `Moderate -> Low`) memerlukan konfirmasi tick berturut-turut (`deescalation_hold_ticks`, default: 3 tick). Jika terjadi fluktuasi sebelum threshold tercapai, kandidat dibatalkan.
   - *Eskalasi Darurat Seketika*: Transisi ke `MemoryPressure::Critical` selalu dieksekusi secara instan (0 tick delay) tanpa terhalang jeda hold untuk mencegah OOM-kill oleh kernel.
   - State machine murni deterministik berbasis tick diskrit tanpa ketergantungan wall-clock, menjamin reproduktifitas pengujian 100%.

3. **Anggaran Migrasi & Pembatasan Laju (`MigrationBudget`)**:
   - Mengatur kuota migrasi maksimum per siklus evaluasi: `max_pages` dan `max_bytes`.
   - `PageStore::apply_tiering_with_budget(budget, decide)` membagi migrasi masif menjadi potongan-potongan terukur, mencegah *latency spike* dan *starvation* I/O pada sistem berdaya rendah.
   - Ekstensi `TieringReport` dengan metrik presisi: `inspected_pages`, `migrated_to_compressed`, `migrated_to_memory`, `already_in_tier`, `deferred_by_migration_budget`, `rejected_by_storage_budget`, `failed_migration`, dan `no_change`.
   - Menjamin kontinuitas migrasi (*resumable tiering*) di mana halaman yang tertunda dievaluasi kembali pada siklus berikutnya.

4. **Promosi Selektif & Proteksi Halaman Inkompresibel**:
   - `AdaptiveTieringPolicy::selective_promotion`: Saat memori berada pada tekanan `Low`, sistem tidak lagi mendekompresi seluruh halaman secara membabi buta. Hanya halaman *hot* yang sering/baru diakses (`latency <= promotion_latency_threshold_ns`) yang dipromosikan ke tier `Memory`; halaman *cold* tetap aman terkompresi di LZ4, menghemat RAM fisik hingga >50%.
   - *Incompressible Page Protection*: `PageStore::migrate_page` membandingkan kapasitas payload hasil kompresi dengan kapasitas dekompresi. Halaman berkadar entropi tinggi/acak yang menghasilkan ekspansi memori (`compressed.capacity() >= decompressed.capacity()`) ditolak dari kompresi dan dipertahankan dalam format raw, mencegah pemborosan memori.

5. **Keamanan Numerik & Akuntansi Kapasitas (`net_accounted_delta`)**:
   - Seluruh konversi ukuran memori dan perhitungan selisih akuntansi menggunakan aritmatika terikat (*checked arithmetic*) dengan validasi batas eksplisit (`isize::try_from`), mencegah *integer wraparound* dan inkonsistensi budget.

## HyperCore Milestone E: Adaptive Kernel Interplay & Multi-Page Extent Clustering

Milestone E memperluas jangkauan HyperCore dari manajemen memori userspace murni menuju koordinasi terpadu tingkat kernel (*kernel-userspace control interplay*) dan efisiensi kompresi memori multi-halaman (*multi-page extent clustering*):

1. **Pengendalian Multi-Aksi cgroup v2 Linux (`xynoos-linux-control`)**:
   - Memperluas enum `Action` dengan dukungan operasional untuk:
     - `Action::MemoryLow { bytes }`: Batas proteksi minimum memori cgroup (reclaim throttle perlindungan proses esensial).
     - `Action::MemoryHigh { bytes }`: Batas ambang penahanan memori cgroup (memicu throttle alokasi dan direct reclaim asinkron sebelum OOM).
     - `Action::MemoryMax { bytes }`: Batas keras kapasitas memori cgroup (mencegah eksploitasi runaway allocation).
   - Pengerasan batas keamanan (`MAX_LOW_BYTES = 64 MiB`, `MAX_HIGH_BYTES = 512 MiB`, `MAX_MAX_BYTES = 1024 MiB`) dan validasi kelipatan 4096-byte terikat.
   - Peningkatan trait `Backend` dan `DisposableCgroup` dengan antarmuka terpadu `read_control` dan `write_control` yang mendukung rollback atomik dan verifikasi identitas cgroup non-intrusif.

2. **Koordinator Interaksi Kernel-Userspace Terpadu (`AuraKernelCoordinator`)**:
   - Menghubungkan engine tiering `AuraTieringCoordinator` dengan sistem kontrol kernel `xynoos-linux-control`.
   - Menerjemahkan status tekanan memori yang telah distabilkan (`MemoryPressure`) menjadi rekomendasi aksi kernel terikat (`KernelTuningRecommendation`):
     - `MemoryPressure::Low`: `KernelTuningRecommendation::None` (bebas penahanan alokasi).
     - `MemoryPressure::Moderate`: `KernelTuningRecommendation::ProtectEssential` (menetapkan `memory.low` protektif pada cgroup proses penting).
     - `MemoryPressure::High`: `KernelTuningRecommendation::ThrottleBackground` (menerapkan `memory.high` limit pada cgroup latar belakang untuk meredam eskalasi tekanan).
     - `MemoryPressure::Critical`: `KernelTuningRecommendation::EmergencyLimit` (menerapkan pembatasan darurat `memory.high` ketat guna mencegah aktivasi kernel OOM-killer).
   - Menjalankan orkestrasi transaksional fail-closed: jika target disposable cgroup disediakan dan terotorisasi, operasi kernel dieksekusi dan dipulihkan secara aman dengan laporan gabungan `InterplayReport`.

3. **Penyimpanan Terkelompok Multi-Halaman (*Page Clustering / Extent Storage*) (`ClusterStore`)**:
   - Mengelompokkan $N$ halaman kontigu (2, 4, 8, atau 16 halaman; rentang 8 KiB hingga 64 KiB) ke dalam satu unit kluster terpadu (`ClusterStore`).
   - *Keuntungan Kompresi Silang Halaman*: Kompresi blok LZ4 pada tingkat kluster (misal 16 KiB) memanfaatkan korelasi kamus antar halaman berdekatan, menghasilkan rasio kompresi jauh lebih unggul dibandingkan kompresi halaman tunggal 4 KiB (mengurangi konsumsi memori hingga >80% pada beban terstruktur).
   - *Pengurangan Overhead Metadata*: Menggabungkan $N$ halaman ke dalam satu entri kluster mengamortisasi header LZ4 dan alokasi `HashMap`, memotong overhead pelacakan per-halaman secara signifikan.
   - *Akses Granular Sub-Halaman*: Menyediakan `get_page_into(cluster_id, page_idx, out)` yang memverifikasi CRC32 individual sub-halaman tanpa merusak integritas kluster.
   - *Proteksi Inkompresibel & Pemulihan*: Jika data kluster berentropi tinggi dan tidak menghasilkan penghematan ukuran, kluster otomatis dipertahankan dalam format raw `StorageTier::Memory`.


---

## 8. Milestone F — Continuous Daemon, Advisory-First Architecture, and System Integration

Milestone F menyatukan seluruh subsistem HyperCore ke dalam integrasi sistem end-to-end dengan arsitektur daemon berkelanjutan yang aman dan deterministik:

1. **Continuous Userspace Daemon (`AuraDaemon`)**:
   - *Lifecycle Prediktif*: Loop evaluasi periodik berbasis waktu (`interval_ms`, default 5000 ms) dengan penanganan sinyal terminasi (`SIGTERM`, `SIGINT`) untuk penutupan bersih dan pemulihan status.
   - *Single-Instance Lock (`PidLock`)*: Penguncian eksklusif berkas PID (`/run/xynoos/hypercore.pid`) untuk mencegah instans ganda.
   - *Exponential Backoff*: Menangani kegagalan pembacaan telemetri atau akses cgroup dengan backoff eksponensial adaptif (1s -> 2s -> 4s -> max 60s).
   - *Batas Memori Terikat*: Mengawasi memori residen (RSS) agar tetap berada di bawah batas ambang 64 MiB (realisasi pengamatan aktual <15 MiB).

2. **Arsitektur Keamanan Advisory-First & Sadar Beban (*Workload-Aware*)**:
   - *Tiga Mode Operasi*:
     - `ObserveOnly` (Default): Hanya memantau telemetri, menghitung tekanan, dan mencatat status. Sama sekali tidak memodifikasi memori userspace atau cgroup kernel.
     - `Advisory`: Mengevaluasi tiering userspace dan menghasilkan rekomendasi cgroup terstruktur tanpa mengeksekusi penulisan kernel.
     - `Transactional`: Menjalankan penyesuaian cgroup transaksional hanya jika diotorisasi secara eksplisit oleh operator.
   - *Limit Sadar Beban*: Menghindari nilai statis buta (seperti 16 MiB atau 32 MiB tetap). Limit dihitung dinamis proporsional terhadap konsumsi aktual cgroup saat ini (110% untuk proteksi, 125% untuk penahanan, 80% untuk batas darurat).

3. **Ketahanan Transaksi & Pemulihan Crash (*Crash Recovery Protocol*)**:
   - *Jurnal Transaksi Lokal (`TransactionJournal`)*: Menulis metadata transaksi (`Identity`, `Action`, `original_value`, `requested_value`) ke disk/tmpfs sebelum melakukan penulisan cgroup.
   - *Rekonsiliasi Startup*: Jika daemon mengalami crash unclean atau `SIGKILL`, daemon yang di-restart membaca jurnal, memverifikasi kecocokan inode cgroup, dan mengembalikan limit ke nilai awal baseline secara fail-safe.

4. **Workload Shadow Replay Profiler (`ShadowProfiler` & `ShadowPlayer`)**:
   - *Perekaman Sadar Privasi*: Hanya mencatat metadata performa non-intrusif (stempel waktu, persentase memori tersedia, rata-rata PSI, penggunaan cgroup). Sama sekali tidak menyalin isi memori aplikasi atau data pengguna.
   - *Simulasi Kontrafaktual Deterministik*: Memutar ulang jejak rekaman terhadap `AuraKernelCoordinator` dan `PageStore` untuk mengukur estimasi penghematan memori, rasio kompresi, dan mitigasi PSI secara akurat tanpa menyentuh beban kerja produksi.

5. **Integrasi Hardened Systemd & Debian Packaging**:
   - Menyediakan unit `systemd/xynoos-hypercore.service` dan `systemd/xynoos-hypercore.slice` dengan pengerasan sandboxing sistem (`ProtectSystem=strict`, `ProtectHome=read-only`, `PrivateTmp=yes`, `NoNewPrivileges=yes`, `CapabilityBoundingSet=CAP_SYS_RESOURCE`).
   - Layanan *tidak diaktifkan otomatis* saat instalasi paket Debian, menjaga prinsip netralitas distribusi.

---

## 9. Milestone G — Adaptive Compression, Resource Cost Intelligence & Runtime Reliability

Milestone G meningkatkan efisiensi memori dan keandalan runtime di seluruh subsistem tanpa melanggar kebijakan `#![forbid(unsafe_code)]` maupun batas MSRV Rust 1.82.0:

1. **Arsitektur Kompresi Adaptif (`xynoos-hypermemory/src/codec.rs`)**:
   - Mendefinisikan `CodecId` eksplisit (`Raw`, `Lz4Standard`, `Lz4Hc { level }`), `CompressionConfig`, `CompressedObjectMeta`, dan `CodecScratch` terikat.
   - Mengimplementasikan encoder **LZ4HC pure-safe-Rust** (*hash-chain + lazy matching*) yang menghasilkan blok LZ4 standar yang kompatibel penuh dengan `lz4_flex::block::decompress_into`.
   - Menghilangkan pemborosan kapasitas berlebih melalui kompresi ke buffer scratch terpakai ulang (*reusable scratch*) dan reservasi kapasitas eksak (`try_reserve_exact`).

2. **Resource Cost Intelligence & Mesin Keputusan (`cost_model.rs`, `compression_policy.rs`)**:
   - `ResourceCostModel` deterministik berbasis EWMA (*fixed-point permille*) dan statistik bergulir terikat (32 sampel) yang memisahkan penghematan bersih setelah metadata, biaya scratch sementara, biaya CPU kompresi/rekompresi, dan penalti latensi pembacaan.
   - `CompressionDecisionEngine` menghasilkan keputusan teraudit (`CompressionAction` + `DecisionReason`): memblokir `Lz4Hc` saat tekanan CPU tinggi, melindungi halaman *hot* dan data inkompresibel, serta menegakkan *cooldown* anti-osilasi.

3. **Telemetri Multi-Sumber (`xynoos-pressureiq/src/multi_resource.rs`)**:
   - Mengumpulkan `/proc/pressure/cpu`, `/proc/pressure/memory`, `/proc/pressure/io`, `/proc/meminfo`, `/proc/vmstat`, `/proc/stat`, dan statistik cgroup v2 secara read-only tanpa pernah memalsukan nilai yang hilang, ditolak izinnya, atau terkena reset counter.

4. **Kalibrasi Kebijakan Bayangan (`ShadowCalibrator` & `PolicyConfig`)**:
   - Skema `PolicyConfig` berversi dengan validasi ketat dan jalur rollback ke `PolicyConfig::safe_default()`.
   - `ShadowCalibrator` memisahkan jejak menjadi *calibration* (50%), *validation* (25% anti-overfitting), dan *evaluation* (25% held-out) untuk membandingkan baseline Milestone F, static LZ4, default Milestone G, dan konfigurasi terkalibrasi secara *counterfactual*.

5. **Perbaikan Audit Phase 0 & Akuntansi Overhead Penuh**:
   - Memperbaiki bug *double-tick / double-hit* pada `ClusterStore::get_page_into`, bug *tick-before-commit* pada `insert_cluster`, dan batas riwayat `evicted_history` pada `ClusterStore`.
   - Menambahkan `MemoryAccounting` (`overhead_accounting()`) yang menghitung payload, metadata (`Entry`, `page_crcs`, `evicted_history`), kapasitas teralokasi, scratch buffer, puncak memori, dan penghematan bersih setelah overhead.

6. **Ketahanan Daemon & Pemulihan Crash (`AuraDaemon`, `PidLock`, `TransactionJournal`)**:
   - `AuraDaemon::run_with_signal` kini mengonsumsi telemetri `PressureIQ` nyata, menerapkan *adaptive polling*, histori telemetri terikat (32 entri), pelaporan RSS (`/proc/self/statm`), dan perbaikan perhitungan *workload-aware* bebas overflow.
   - `PidLock` menggunakan pembuatan berkas atomik (`O_CREAT | O_EXCL`) serta memverifikasi `boot_id` dan `start_ticks` dari `/proc/<pid>/stat` untuk mencegah risiko *PID reuse*.
   - `TransactionJournal` menggunakan penulisan atomik (*temp file + fsync + rename + dir sync*), checksum CRC32 integritas jurnal, dan deteksi konflik *fail-closed* terhadap perubahan eksternal atau perbedaan `boot_id`/identitas cgroup.
