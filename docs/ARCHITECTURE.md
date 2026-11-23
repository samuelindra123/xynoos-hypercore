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

1. **Siklus Hidup Halaman Eksplisit**:
   - `PageLifecycleState::Active` dan `PageLifecycleState::Evicted` memodelkan status halaman secara terverifikasi.
   - `PageStore::contains(&self, id: PageId) -> bool` memeriksa keberadaan halaman secara O(1).
   - `PageStore::lifecycle_state(&self, id: PageId) -> PageLifecycleState` memberikan inspeksi status tanpa mutasi.

2. **Pengambilan Tanpa Alokasi (`get_into`)**:
   - `get_into(&self, id: PageId, out: &mut [u8]) -> Result<usize, Error>` melakukan dekompresi langsung ke buffer milik pemanggil.
   - Menghindari overhead alokasi `Vec<u8>` pada hot-path pembacaan.
   - Mengembalikan `Error::OutputBufferTooSmall` bila ukuran buffer pemanggil kurang dari `page_size`.
   - `get(&self, id: PageId)` tetap tersedia untuk backward compatibility dengan mengalokasikan buffer dan mendelegasikan ke `get_into`.

3. **Pembaruan Halaman Atomik (`update`)**:
   - `update(&mut self, id: PageId, data: &[u8]) -> Result<(), Error>` mengganti isi halaman secara in-place.
   - Mengevaluasi delta budget sebelum komit; jika budget terlampaui atau input tidak valid, modifikasi dibatalkan tanpa mengubah state yang ada (atomic rollback).
   - Menghitung ulang CRC32 payload dan original data serta memperbarui `last_accessed_tick`.

4. **Mesin Penggusuran Deterministik (`EvictionPolicy`)**:
   - `EvictionPolicy::Reject`: perilaku default historis (mengembalikan `Error::Full` atau `Error::BudgetExceeded`).
   - `EvictionPolicy::Lru`: menggusur halaman yang paling lama tidak diakses (`last_accessed_tick` terendah; seri dipecahkan secara deterministik dengan `PageId.0` terendah).
   - `EvictionPolicy::Fifo`: menggusur halaman yang paling awal disisipkan (`inserted_tick` terendah; seri dipecahkan dengan `PageId.0`).
   - `insert_with_policy(&mut self, data: &[u8], policy: EvictionPolicy) -> Result<PageId, Error>`: secara otomatis menggusur halaman jika kapasitas penuh atau budget byte terlampaui sampai entri baru dapat dimuat.
   - `evict(&mut self) -> Result<Option<PageId>, Error>`: API eksplisit untuk menggusur satu kandidat LRU.

5. **Observabilitas & Telemetri Operasional (`Telemetry`)**:
   - `telemetry(&self) -> Telemetry` mencatat metrik runtime ringan:
     - `hit_count`, `miss_count`, `eviction_count`, `insert_count`, `update_count`, `remove_count`, `corruption_count`.
     - `hit_ratio(&self) -> Option<f64>` menghitung rasio keberhasilan cache secara tepat.
   - `reset_telemetry(&self)` mereset counter operasional.
   - `peek(&self, id: PageId) -> Result<Vec<u8>, Error>` memungkinkan inspeksi diagnostik non-intrusif tanpa memajukan access tick atau menambah hit counter.
   - Struktur `Stats` tetap memegang akuntansi penyimpanan fisik murni (`stored_bytes`, `accounted_bytes`, `buffer_capacity_bytes`, `metadata_bytes_lower_bound`) menjaga invarian historis.
