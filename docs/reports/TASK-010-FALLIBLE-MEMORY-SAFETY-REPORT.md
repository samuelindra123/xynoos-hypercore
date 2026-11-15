## 1. HALAMAN IDENTITAS

**Proyek:** Xynoos HyperCore. **Task:** 010 — Fallible Memory Transactions. **Tanggal:** 15 November 2026. **Milestone:** riset userspace 0.1.0. **Base:** 6138a7a71f54e146b5a834df9afd27c03e0e34b3. **Status teknis:** SELESAI; status publikasi dicatat melalui commit dan Pull Request GitHub.

## 2. ABSTRAK

Prepared Vec FastPath kini memakai reservasi fallible dan error khusus. Pengujian reservasi, rollback, epoch tracker dan concurrency berbatas diselesaikan. Lima puluh unit test serta enam quality gate lulus. Sebanyak 1.458 record benchmark tervalidasi dengan hasil simulasi ekuivalen. Oracle tetap default; tidak ada klaim recovery dari OOM nyata.

## 3. PENDAHULUAN

Task membatasi perubahan pada boundary metadata dan instrumentasi penelitian. Sasaran adalah error yang jelas dan failure atomicity; tidak mengubah cost model, kebijakan AURA, kernel atau swap.

## 4. LANDASAN TEKNIS

Result tidak menjadikan setiap alokasi fallible. TryReserveError dapat mewakili capacity overflow atau kegagalan allocator; keduanya berbeda dari kapasitas fisik simulator. Unwind dapat membuang prepared state privat, tetapi abort tidak dapat ditangkap catch_unwind. Requested bytes tidak sama dengan ukuran allocator fisik.

## 5. AUDIT TASK 009

Remote dan main cocok dengan milestone 6138a7a; file ss dipertahankan. Source physical, selector, placement, HyperMemory, hooks, tracker, runner, README, arsitektur serta laporan TASK007/008/009 diperiksa. Prepared FastPath memakai sim.clone yang menyalin Vec secara infallible. Metadata profil hanya integer, enum dan Option integer; tidak menyimpan String/Vec sehingga clone setiap profil tidak mengalokasikan.

## 6. ANALISIS RISIKO ALOKASI

| Jalur | Jenis | Risiko / tindakan |
|---|---|---|
| Prepared FastPath Vec | metadata transaksi | diganti try_reserve_exact |
| Oracle event dan preview | Vec clone infallible | tetap oracle independen, dapat abort |
| Journal snapshot | field fixed-size | tidak memiliki heap baru |
| Inspeksi FastPath | state/engine/metrics/profil clone | fixed-size, tanpa Vec |
| HyperMemory raw/output/map | try_reserve | sebagian recoverable |
| LZ4 compress | eksternal/infallible | OOM abort masih mungkin |
| Fixture/trace/CSV | di luar transaksi | tidak diklaim fallible |
| Error AllocationReservation | enum tanpa String | tidak mengalokasikan |

Expect invariant HyperMemory masih dapat panic bila invariant dilanggar. Tidak semua alokasi Rust maupun eksternal dijamin recoverable.

## 7. PERANCANGAN FALLIBLE PREPARED STATE

Simulator::try_clone_metadata memvalidasi 1..=64 grup, membuat Vec kosong, try_reserve_exact(len), memeriksa kapasitas lalu menyalin profil fixed-size. State/engine/metrics disalin tanpa heap. Overflow ukuran ditangani Vec; dengan batas64 dan profil80 byte, ukuran normal maksimal5120 byte. Reservasi dipetakan ke Error::AllocationReservation, berbeda dari Error::Capacity model. Penambahan enum variant dapat memerlukan pembaruan exhaustive match caller; versi library tetap penelitian0.1. API FastPath tidak berubah.

## 8. MEKANISME FAULT INJECTION

Helper crate-private try_clone_metadata_with menerima closure reservasi yang sama dengan jalur normal; test dapat menolak tepat boundary tersebut. Closure yang mengembalikan sukses tanpa kapasitas ditolak sebelum extend agar tidak mengaktifkan alokasi infallible tersembunyi. Test actual try_reserve_exact(usizeMAX) menghasilkan TryReserveError tanpa OOM, dipetakan ke AllocationReservation. Ini capacity-overflow reservasi, bukan real ENOMEM. Hook fase0 menginjeksi AllocationReservation pada transaksi untuk memeriksa simulator dan cursor. Hooks fase1..4 dan unwind lama tetap dipertahankan.

## 9. MODEL FAILURE ATOMICITY

Original tidak berubah sampai prepared complete berhasil; error reserve membuang Vec privat sebelum akses, sedangkan error setelah prepare/inspection/commit preparation membuang state privat. Publikasi simulator dan cursor dilakukan terakhir tanpa langkah fallible di antaranya. Snapshot test membandingkan semua page groups, engine, counter, occupancy, metrics dan cursor. Kandidat sama dengan halaman akses tercakup dalam differential sequence.

## 10. PANIC SAFETY REVIEW

Catch_unwind dengan AssertUnwindSafe hanya dipakai pada test yang mengambil snapshot dan memeriksa state sesudah panic. Prepared memiliki field tanpa destructor kustom yang dapat panic; Vec drop membebaskan buffer. Ini bukti terbatas untuk hook sebelum publikasi. Journal in-place hanya rollback saat Result::Err dan belum dijamin panic-safe; tetap experimental. Process abort dan allocator abort di luar jaminan. Tidak menggunakan hook sesudah publikasi.

## 11. HEAP TRACKER ARCHITECTURE

tracker010 terpisah dari tracker historis. Register4096 pointer/size atomik dengan spin lock; callbacks tanpa heap/logging. Begin sekarang memegang lock saat reset dan menolak jika outstanding tidak nol; EPOCH bertambah hanya saat scope baru berhasil. Tidak ada per-record epoch karena scope baru hanya dimulai sesudah objek tracked lama habis. Objek pra-scope tidak mengurangi live; objek tracked tetap dilacak saat deallocation sesudah disabled. Table full menandai INVALID. Marker kosong/tombstone berbeda, pointer hanya alamat numerik. Realloc helper sama dipakai callback dan metadata tests untuk alamat sama, alamat berubah dan hasil null; null mempertahankan record lama.

## 12. CONCURRENCY VALIDATION

Executable tracker_validation membuat empat worker sebelum scope, dengan barrier bersama. Masing-masing menjalankan100 Vec allocation/realloc/drop. Scope global diaktifkan sebelum barrier dilepas, dinonaktifkan sesudah barrier selesai, lalu semua worker dijoin. Assert live count/bytes nol dan INVALID false lulus. Ini validasi global attribution berbatas, bukan isolasi per-thread. Lock acquire/release menserialisasi register. Begin tidak boleh dipakai untuk scope independen per-thread; alokasi yang callback-nya melewati boundary diatribusikan menurut saat register diperbarui. Tidak mengklaim general-purpose multithread profiler, starvation lock atau reentrancy seluruh backend.

## 13. IMPLEMENTASI

physical.rs menambah error dan try-clone helper; selector mengganti hanya clone prepared dan menambah empat test. support/tracker010.rs serta tracker_validation.rs baru; fallible.rs memakai tracker baru, reliability lama tetap memakai tracker009. benchmark_task010.py baru, tanpa dependency. Unsafe hanya instrumentasi executable yang meneruskan pointer/Layout ke System; library tetap safe Rust.

## 14. METODOLOGI PENGUJIAN

Ubuntu24.04 Codespaces, Linux6.8 Azure x86_64, rustc1.100.0,4vCPU. Sembilan trace kompatibel TASK009, grup1/16/64 pada benchmark dan1/4/8/16/32/64 dalam suite differential. Seed1/42/2026,128 event, warm-up1 dan3 sample, tiga mode dengan rotasi urutan, dua subprocess. Total1.458 record,162 sample agregat ukuran/mode. Fixture/order/trace/output di luar scope pengukuran; live heap diambil sebelum drop simulator lalu assert zero setelah drop. Peak requested-live bukan RSS.

## 15. HASIL UNIT TEST

50 unit test lulus:37 AURA dan13 HyperMemory. Seluruh46 test lama tetap. Empat tambahan mencakup injected reservation, real capacity-overflow try_reserve, insufficient reservation guard, transaction allocation rejection serta multi-event candidate/access equality (sebagian dalam test yang sama). Test budget/cooldown/fallback/scratch/overflow dan journal equivalence lama tetap lulus.

## 16. HASIL FAULT INJECTION

AllocationReservation berbeda dari Capacity dan test memeriksa error tepat. Snapshot helper/transaction/cursor tidak berubah pada penolakan. Five-stage error dan five-stage unwind TASK009 tetap lulus. Log task-010-fault-injection.txt memuat test injection dan reservation dengan expected panic yang tertangkap. Tidak ada real memory exhaustion atau null allocator global.

## 17. HASIL DIFFERENTIAL TEST

Unit suite membandingkan Result/Error dan seluruh state antara oracle dan FastPath pada enam ukuran; old journal test tetap. Benchmark membandingkan seluruh metrik simulator antar clone/journal/FastPath dan seluruh metrik non-timing antar dua proses; lulus1.458 record. Alokasi dan timing berbeda antar mode sehingga tidak dipaksa sama. Bounded testing bukan bukti formal seluruh state.

## 18. HASIL QUALITY GATES

Keenam gate setelah perubahan akhir exit0: cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo check --workspace; cargo build --workspace --release; cargo build -p xynoos-aura --examples --release. Log lengkap task-010-quality-gates.txt. Clippy awal menemukan helper validation tidak dipakai executable fallible; diperbaiki dengan menjalankan helper pada startup benchmark, bukan menonaktifkan lint.

## 19. HASIL BENCHMARK

| Grup | Mode | Median batch ns | Inspeksi ns | Alloc count | Requested byte | Outstanding byte | Peak live byte |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | clone | 48179.5 | 28274.5 | 384.0 | 30720.0 | 80.0 | 240.0 |
| 1 | journal | 44923.5 | 29931.0 | 256.0 | 20480.0 | 0.0 | 160.0 |
| 1 | fast | 41046.0 | 15845.0 | 128.0 | 10240.0 | 80.0 | 160.0 |
| 16 | clone | 601371.5 | 571891.0 | 4224.0 | 5406720.0 | 1280.0 | 3840.0 |
| 16 | journal | 601342.0 | 579552.0 | 4096.0 | 5242880.0 | 0.0 | 2560.0 |
| 16 | fast | 291899.0 | 257776.0 | 128.0 | 163840.0 | 1280.0 | 2560.0 |
| 64 | clone | 3888880.5 | 3844599.0 | 16512.0 | 84541440.0 | 5120.0 | 15360.0 |
| 64 | journal | 3961812.0 | 3940515.0 | 16384.0 | 83886080.0 | 0.0 | 10240.0 |
| 64 | fast | 1204540.5 | 1145276.0 | 128.0 | 655360.0 | 5120.0 | 10240.0 |

Waktu merupakan median batch128 event yang diinstrumentasi. Data menggabungkan sembilan workload; jangan menafsirkan agregat sebagai latency semua trace. Runtime uninstrumented setara belum diukur pada task ini; benchmark historis berbeda workload/instrumentasi tidak dijadikan kontrol langsung.

## 20. ANALISIS HEAP DAN RSS

Pada64 grup peak requested-live clone15360 byte, journal10240 dan FastPath10240. Outstanding clone/FastPath5120 byte sebelum drop (Vec hasil transaksi); journal0 karena mempertahankan Vec pra-scope. Semua scope berakhir dengan outstanding0. Peak RSS maksimum subprocess15820KiB mencakup runtime/fixture dan kemungkinan pre-exec inheritance; bukan footprint per-mode. Register statis64KiB tidak masuk requested heap. Realloc transient internal System tidak diketahui; requested-live bukan peak fisik.

## 21. EVALUASI DEFAULT FASTPATH

Default oracle dipertahankan. Prepared Vec sudah fallible dan rollback kuat untuk boundary diuji, tetapi oracle/codec/harness tetap infallible, journal belum unwind-safe, dan scope concurrent hanya divalidasi terkontrol. Benchmark lebih cepat bukan alasan tunggal mengubah default. Rollback versi eksperimen tersedia melalui API oracle yang tetap independen.

## 22. ANALISIS RISIKO DAN KETERBATASAN

Heap register menggunakan sentinel0/usizeMAX dengan asumsi pointer Linux valid bukan sentinel. Table penuh menggagalkan measurement; tidak ditawarkan sebagai profiler umum. Epoch guard menolak overlap objek tracked, bukan menyediakan scope per-thread. Overflow counter tracker pada workload tak terbatas belum ditargetkan, eksperimen sangat bounded. Tidak menjalankan Miri/sanitizer atau System ENOMEM nyata. Reservasi5120byte dapat sukses pada sebagian besar host sehingga injection digunakan untuk reproduksi failure.

## 23. KESIMPULAN

Prepared allocation fallible, error terpisah, fault rejection, rollback, tracker boundaries dan concurrency terbatas berhasil diuji. Semua quality gates serta benchmark final tersedia. TASK010 selesai sebagai penelitian userspace tanpa klaim seluruh simulator OOM-safe.

## 24. REKOMENDASI TASK011

Perluas typed prepared state dan audit unsafe independen, implementasikan allocator backend injection non-global jika perlu menguji ENOMEM semantik lebih jauh. Pertahankan oracle sampai test concurrency/reentrancy dan workload nyata memadai. Model AI sesuai: GPT-6.1 Terra untuk audit Rust dan differential verification, tetap wajib quality gates.

## 25. DAFTAR PUSTAKA

Source primer repository: physical.rs, selector.rs, tracker010.rs, tracker_validation.rs dan test; laporan TASK009 dan CSV TASK010. Referensi API Rust: https://doc.rust-lang.org/std/vec/struct.Vec.html#method.try_reserve_exact ; https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html ; https://doc.rust-lang.org/std/panic/fn.catch_unwind.html .

## 26. LAMPIRAN DAN BUKTI

Artefak docs/benchmarks/task-010-{raw.csv,summary.csv,quality-gates.txt,fault-injection.txt,reproducibility.txt,memory.txt,tracker-validation.txt}. Perintah: enam quality gates; target/release/examples/tracker_validation; cargo test -p xynoos-aura injected_tests -- --nocapture; cargo test -p xynoos-aura reservation_tests -- --nocapture; python3 scripts/benchmark_task010.py. Uji metadata meliputi pointer reuse, tombstone, table full, same-address realloc/failure bookkeeping; System tidak dipaksa memilih alamat sama. Zero-sized Box tidak memanggil GlobalAlloc dan assert live0; tidak memanggil alloc Layout nol yang melanggar kontrak. Storage awal200MiB, akhir219MiB, tersedia17779970048byte saat pengukuran. Branch research/task-010-fallible-memory dari main6138a7a; commit/PR diterbitkan setelah audit staged. ss tidak disentuh, tanpa dependency baru atau perubahan host.
