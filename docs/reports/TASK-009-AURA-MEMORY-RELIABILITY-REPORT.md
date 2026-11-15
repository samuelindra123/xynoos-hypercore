## 1. HALAMAN IDENTITAS

Xynoos HyperCore TASK009 — AURA Memory Reliability. Tanggal15 November2026. Milestone riset0.1.0; base ee19a947b872a624a836a78af1a0bfe876e981a9. Status teknis SELESAI; commit/PR/merge ditentukan bukti remote.

## 2. ABSTRAK

Ditambahkan atribusi pointer berbatas pada executable penelitian dan hook transaksi deterministik.46 unit test serta enam quality gate lulus.1458 record dari dua subprocess identik non-timing, meliputi sembilan trace dan tiga ukuran. Pada64 grup peak live requested heap scope median clone15360byte dan FastPath10240byte. Pengukuran bukan RSS per-mode atau bukti OOM recovery. Oracle tetap default.

## 3. PENDAHULUAN

Penelitian menilai live heap dan rollback, bukan hanya kecepatan. Seluruh source dan artefak lama dipertahankan; file ss tidak masuk task. Tidak mengubah host Linux.

## 4. LANDASAN TEKNIS

Requested byte tidak mencakup header allocator, fragmentasi atau residency. Scope atribusi memerlukan identitas objek ketika deallocation melintasi batas. Recoverable Result, unwind panic dan abort berbeda: catch_unwind tidak dapat memulihkan process abort.

## 5. AUDIT TASK008

Git main dan remote diverifikasi ee19a94; branch baru dari origin/main. Allocator lama hanya menghitung traffic dan tidak mengenali pointer pra-scope. FastPath clone Vec masih infallible; HyperMemory memakai try_reserve untuk sebagian buffer/map tetapi codec LZ4 tetap dapat abort OOM. Library melarang unsafe; unsafe allocator hanya executable benchmark. Counter dan accounting checked mengembalikan error; expect invariant HyperMemory dapat panic bila invariant internal dilanggar. Oracle/journal belum dijamin panic-safe.

## 6. ANALISIS MEMORY SAFETY

Wrapper baru meneruskan pointer/Layout identik ke System dan hanya menyimpan alamat numerik, tidak mendereferensinya. Register dilindungi spin lock, callback tanpa logging/heap dan tidak memicu alokasi rekursif. Instrumen terpisah dari library. Asumsi target Linux: sentinel0 dan usizeMAX bukan pointer valid System. Realloc gagal tidak mengubah record; sukses menghapus pointer lama lalu mencatat baru, termasuk ukuran baru saat pointer sama.

## 7. PERANCANGAN HEAP ATTRIBUTION

Register4096 slot statis, hash alamat dan probing, tombstone untuk deletion. begin mensyaratkan outstanding0, membersihkan register dan counter; callback dealloc mencari identitas meski scope disabled. Objek pra-scope tidak ditemukan sehingga tidak mengurangi scope live. Objek scope yang dilepas sesudah disable tetap dikeluarkan. Realloc tracked mempertahankan atribusi; untracked yang direalloc saat scope aktif diadopsi sebagai objek baru dari saat itu. Table penuh menandai INVALID dan eksperimen gagal, bukan mengeluarkan angka diam-diam. Peak dihitung atas jumlah requested bytes objek hidup yang tercatat. Transient internal System.realloc tidak diketahui sehingga peak physical allocator tidak dijamin. Register statis dan allocator overhead tidak termasuk requested heap.

## 8. PERANCANGAN FAILURE INJECTION

Hook private di selector menerima fase0 sebelum clone,1 sesudah prepare,2 sesudah inspeksi,3 sebelum complete dan4 sesudah complete sebelum publication. Normal API memakai closure no-op. Test mengembalikan Error::Capacity deterministik pada tiap fase dan memverifikasi snapshot. Reservasi Vec usizeMAX ditolak secara recoverable tanpa alokasi nyata; ini capacity-overflow rejection, bukan ENOMEM System. Tidak ada global allocator yang sengaja mengembalikan null.

## 9. MODEL TRANSACTIONAL CORRECTNESS

Prepared state privat dibuang bila hook/error/panic sebelum publication terjadi. Simulator dan selector baru dipublikasikan setelah semua operasi fallible selesai. Test catch_unwind memeriksa original simulator dan selector sama, bukan hanya panic tertangkap. Hook tidak dijalankan setelah publication. Tidak ada klaim rollback journal saat panic atau allocator abort.

## 10. IMPLEMENTASI

selector.rs menambah fast_event_hook internal dan tiga test. reliability.rs executable baru memakai support/tracker.rs untuk atribusi; benchmark_task009.py menjalankan, memvalidasi dan merangkum. API publik dan default tidak berubah; tidak ada dependency baru. Table/counter digunakan global dalam proses benchmark satu thread, bukan profiler thread-local umum.

## 11. METODOLOGI PENGUJIAN

Ubuntu24.04, Linux6.8 Azure x86_64, rustc1.100.0,4vCPU. Fixture valid1/16/64grup, policy Aura, raw4096 dan metadata80byte/grup, budgetRAM n×4176, scratch8192; ukuran terkompresi512 atau4114, entropy4114, compressible27, unreliable confidence0. Sembilan trace0interactive,1background,2switching,3pressure,4entropy,5compressible,6bursty,7adversarial,8unreliable. Proksi background memakai distribusi modular deterministik, bukan workload idle nyata. Seed1/42/2026,128event, warmup1 dan3sample, mode dirotasi,2subprocess. Fixture/order/trace/output di luar hot window; counter live diambil sebelum drop simulator, lalu drop dan assert outstanding0. Seluruh43test lama tetap, termasuk ukuran4/8/32 yang tidak diulang pada benchmark besar.

## 12. HASIL UNIT TEST

46test lulus:33AURA dan13HyperMemory. Tambahan3test: error fase, unwind fase dan controlled allocation rejection. Test lama mempertahankan differential fullstate multi-event enam ukuran, invalidindex/permutation, overflow, capacity, CPU/IO, cooldown, scratch dan fallback.

## 13. HASIL FAULT INJECTION

Lima error stage dan lima unwind stage menghasilkan simulator serta selector identik snapshot awal. Controlled reservation usizeMAX menghasilkan Capacity dan rollback. Log task-009-fault-injection.txt memuat panic message yang diharapkan serta hasil test lulus; panic tersebut bukan error yang disembunyikan. Tidak menguji actual ENOMEM/OOM abort.

## 14. HASIL DIFFERENTIAL TEST

1458 record valid (9trace×3ukuran×3seed×3mode×3sample×2process). Seluruh metrik simulasi identik terhadap clone pada seed/trace/ukuran/sample sama, sementara runtime dan atribusi heap memang berbeda. Dua proses identik pada seluruh kolom selain timing, termasuk heap counters. Unit test oracle independen tetap memeriksa seluruh PageGroup/Engine/Metrics/Error dan cursor, bukan agregat saja.

## 15. HASIL QUALITY GATES

Setelah perubahan akhir, fmt --check, clippy workspace/all-targets -D warnings, test workspace, check workspace, build workspace release dan build examples release semuanya exit0. Log task-009-quality-gates.txt. Test injeksi juga dijalankan --nocapture untuk bukti.

## 16. HASIL BENCHMARK RUNTIME

| Grup | Mode | Median batch ns | Inspeksi ns | Alloc count | Requested byte | Dealloc | Outstanding byte | Peak live byte |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 1 | clone | 47358.0 | 27075.0 | 384.0 | 30720.0 | 383.0 | 80.0 | 240.0 |
| 1 | journal | 41898.0 | 27987.0 | 256.0 | 20480.0 | 256.0 | 0.0 | 160.0 |
| 1 | fast | 38607.5 | 15971.0 | 128.0 | 10240.0 | 127.0 | 80.0 | 160.0 |
| 16 | clone | 589614.5 | 558347.0 | 4224.0 | 5406720.0 | 4223.0 | 1280.0 | 3840.0 |
| 16 | journal | 582897.0 | 561920.0 | 4096.0 | 5242880.0 | 4096.0 | 0.0 | 2560.0 |
| 16 | fast | 303511.0 | 271626.5 | 128.0 | 163840.0 | 127.0 | 1280.0 | 2560.0 |
| 64 | clone | 4036200.0 | 3988725.5 | 16512.0 | 84541440.0 | 16511.0 | 5120.0 | 15360.0 |
| 64 | journal | 3908432.0 | 3886747.0 | 16384.0 | 83886080.0 | 16384.0 | 0.0 | 10240.0 |
| 64 | fast | 1274181.0 | 1205305.0 | 128.0 | 655360.0 | 127.0 | 5120.0 | 10240.0 |

Angka ns adalah median batch128, bukan latency satu event. Ringkasan menggabungkan162sample per ukuran/mode dari sembilan workload; data per workload tersedia pada CSV mentah. Tidak menafsirkan median gabungan sebagai performa setiap trace. Instrumen register/spinlock menambah overhead, sehingga tidak dibandingkan langsung dengan TASK008.

## 17. ANALISIS HEAP DAN RSS

Pada64grup, clone outstanding sebelum drop5120byte (satu Vec hasil transaksi); journal0 karena mempertahankan Vec pra-scope; FastPath5120. Peak scope clone15360, journal10240 dan Fast10240byte. Ini subset heap yang dialokasikan dalam scope, tidak termasuk Vec fixture/order/trace lama; bukan footprint total mode. Peak RSS maksimum subprocess15980KiB mencakup seluruh runtime dan inisialisasi. Self-check menolak pengurangan live dari Box pra-scope, menguji Vec realloc dan dealloc sesudah scope disabled. Semua run assert tablevalid dan LIVE0 setelah drop.

## 18. EVALUASI KELAYAKAN DEFAULT FASTPATH

Oracle tetap default. Equivalence dan rollback yang diuji mendukung eksperimen, tetapi Vec.clone masih infallible, process abort tidak ditangani, allocator multi-thread belum divalidasi menyeluruh, dan oracle/fase masih duplikatif. Tidak cukup bukti untuk menjanjikan recovery OOM atau default produksi.

## 19. ANALISIS RISIKO

Allocator register menambah unsafe benchmark; safety harus ditinjau bila mendukung pointer/layout/platform lain. Callback tidak boleh panic/logging. Table terbatas, tidak digunakan sebagai profiler aplikasi umum. Hooks private tidak menambah antarmuka publik yang dapat menginjeksi production error. Closure no-op pada normal API tetap diuji quality gate.

## 20. KETERBATASAN DAN ANCAMAN VALIDITAS

Peak live adalah requested bytes scope, bukan usable allocator size atau RSS. Realloc sukses dimodelkan sebagai pelepasan recordlama lalu allocationbaru; peak transient copy internal tidak tersedia. Tidak ada null allocation injection actual, Miri/sanitizer atau realOOM. Sembilan trace adalah versi sintetis kompatibel konsep, seed modular bukan rekaman aplikasi. Benchmark ukuran1/16/64 terstruktur; ukuran4/8/32 hanya correctness suite. Dua subprocess dan timer instrumentation tidak memberi interval kepercayaan perangkat.

## 21. KESIMPULAN

Atribusi objek lintas boundary, controlled recoverable rejection, rollback error/unwind sebelum commit dan multi-workload benchmark selesai. Angka heap dapat diaudit sesuai scope dan batas model. Tidak mengklaim OOM recovery atau peningkatan kernel.

## 22. REKOMENDASI TASK010

Tambahkan boundary test allocator lebih luas, stale scope/epoch dan concurrency validation; desain try_clone fallible untuk prepared metadata, lalu uji backend allocation injection nyata berbasis dependency tanpa abortglobal. Pertahankan oracle sampai audit safety independen dan pengukuran workload nyata. Model yang sesuai ialah model coding Rust dengan kemampuan audit unsafe, misalnya GPT-6.1 Terra.

## 23. DAFTAR PUSTAKA

Source primer repository: TASK008 allocator dan laporan, physical.rs, selector.rs, reliability.rs dan support/tracker.rs. Bukti dari test/log/CSV TASK009; tidak menggunakan paper eksternal untuk mengklaim hasil validasi.

## 24. LAMPIRAN DAN BUKTI

Reproduksi: enam perintah quality gate sesuai task; cargo test -p xynoos-aura injected_tests -- --nocapture; python3 scripts/benchmark_task009.py setelah build examples release. Artefak task-009-{raw.csv,summary.csv,quality-gates.txt,reproducibility.txt,memory.txt,fault-injection.txt}. Sourcebaseee19a947; commit dan PR disimpan pada GitHub. Perubahan hanya selectorhook/tests, executable/tracker, runner, README/arsitektur/laporan/artefak. Storage awal190MiB, akhir200MiB, filesystem tersedia17802203136byte saat diukur. ss dipertahankan. Tidak ada dependency besar atau operasi kernel/swap.
