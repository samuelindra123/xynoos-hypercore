## 1. HALAMAN IDENTITAS

Xynoos HyperCore — TASK 011: Transaction Safety. Tanggal 15 November 2026. Baseline d082f60ade87b30697fa03384ace1a8089f036d3; branch research/task-011-transaction-safety. Status teknis: SELESAI; status publikasi diverifikasi melalui GitHub.

## 2. ABSTRAK

Type-state private membedakan Prepared dan CommitReady. Lima puluh satu unit test, satu compile-fail doctest dan enam quality gate lulus. Enam ratus empat puluh delapan pasangan benchmark terhadap baseline identik pada field non-timing. Journal tetap experimental karena belum dibuktikan unwind-safe. Audit unsafe adalah self-review, bukan review eksternal.

## 3. PENDAHULUAN

Tujuan memperjelas boundary transaksi tanpa mengubah model biaya, hasil policy atau default. Simulator tetap userspace dan tidak berhubungan dengan reclaim kernel.

## 4. LANDASAN TEKNIS

Ownership dan consuming method membatasi urutan operasi; atomicity Result berbeda dari unwind dan abort. Differential test membandingkan oracle independen; tidak membuktikan semua kemungkinan state.

## 5. AUDIT TASK 010

Main remote diverifikasi d082f60; ss tidak terlacak dipertahankan.

| Komponen | Perilaku | Risiko | Perubahan |
|---|---|---|---|
| FastPath | prepared Simulator lokal | urutan API hanya konvensi | tipe Prepared dan CommitReady private |
| Journal | mutasi in-place, rollback pada Err | panic melewati rollback | tetap experimental, audit |
| Tracker010 | unsafe System forwarding | scope global/reentransi | audit terpisah; tidak dipromosikan |
| Oracle | event clone dan preview | alokasi infallible | dipertahankan independen |

## 6. THREAT MODEL

Input invalid, overflow dan kegagalan reservasi terkontrol masuk scope. Caller aman tidak boleh mempublikasikan Prepared. Actual ENOMEM, process abort, corrupt memory, host kernel dan profiler umum tidak divalidasi.

## 7. ANALISIS TRANSACTION BOUNDARY

Transaksi dimulai setelah validasi urutan; hook0 mendahului reservasi. State clone privat, prepare memproses akses sekali, inspeksi menyimpan dan memulihkan state kandidat, validate menjalankan complete_candidate sebelum publikasi. Tidak ada original mutation sebelum validate sukses. Linearization point ialah assignment *original = ready.simulation pada publish; assignment cursor langsung sesudahnya adalah metadata selector. Keduanya tidak memiliki operasi fallible atau alokasi yang diketahui di antaranya. Inspeksi measured accumulator dapat berubah walau transaksi kemudian gagal; itu telemetry eksternal, bukan state simulator.

## 8. PERANCANGAN TYPE-STATE

Prepared hanya menyediakan new, inspect, validate. validate mengonsumsi self dan menghasilkan CommitReady hanya setelah complete_candidate berhasil. CommitReady memiliki publish yang mengonsumsi self. Kedua tipe serta field private, tidak dapat dibangun caller eksternal; doctest compile_fail memeriksa privasi. Type-state tidak memformalkan urutan pemeriksaan semua kandidat secara umum: itu tetap kewajiban loop selector dan test.

## 9. ANALISIS OWNERSHIP DAN LIFETIME

Prepared memiliki Simulator sendiri; original hanya dipinjam saat clone lalu tidak dimutasi hingga publish. Simulator dan PageGroup memakai field fixed-size serta Vec sendiri; tidak ada user destructor kustom. Ready tidak meminjam original. Penggunaan mem::drop pada jalur error membebaskan Vec privat. Rust panic-unwind pada hook0..4 sebelum publikasi diuji terhadap snapshot; process abort tidak tertangkap.

## 10. IMPLEMENTASI

selector.rs menambah Prepared/CommitReady private dan mengubah alur fast_event_hook; API publik, oracle, hooks dan seluruh test lama dipertahankan. typed_research.rs memakai tracker TASK010 dan trace terbatas. Tidak ada dependency baru atau unsafe dalam library.

## 11. JOURNAL PANIC SAFETY

event_journal menyimpan state/engine/metrics/tick/dua profil, menjalankan step in-place, lalu mengembalikan snapshot hanya jika Result::Err. Panic dari step melewati cabang rollback dan dapat meninggalkan state parsial; belum ada RAII guard. Mengubah ownership untuk guard aman berpotensi lebih luas dari task minimal; journal tetap experimental. Test journal/clone pada Err tetap, tidak ada klaim unwind safety.

## 12. UNSAFE AUDIT

Audit dilakukan agent yang sama secara metodologis terpisah, bukan audit eksternal independen. Tabel lengkap dan semua lokasi allocator TASK008–010: docs/benchmarks/task-011-safety-audit.md. Tidak ditemukan high severity yang terkonfirmasi pada workload ini. Risiko sedang: reentrancy System di dalam spinlock, sentinel target Linux, epoch global dan counter overflow di luar batas eksperimen. Tidak mengklaim bebas bug.

## 13. METODOLOGI PENGUJIAN

Lingkungan: Ubuntu 24.04, Linux 6.8 Azure x86_64, rustc 1.100.0, 4 vCPU. Grup1/16/64, trace 0 interactive,4 entropy,7 adversarial,8 unreliable, seed1/42/2026, policy Aura, 128 event, warmup1, tiga sample, dua subprocess, urutan mode diputar. Fixture dan trace di luar hot window. Tracker010 aktif sehingga atomik/lock memengaruhi timing. Perbandingan baseline dibangun offline di /tmp dari d082f60 memakai harness yang sama. Eksekusi baseline dan baru tidak diacak berpasangan; bias urutan dan host tetap mungkin.

## 14. HASIL UNIT TEST

51 unit test lulus (38 AURA,13 HyperMemory), ditambah satu compile-fail doctest. Seluruh50 unit test lama dipertahankan. Test baru memeriksa original tetap sama setelah prepare dan dua inspect, validate belum mempublikasikan, lalu hasil publish identik oracle untuk akses/kandidat sama.

## 15. HASIL DIFFERENTIAL TESTING

Test multi-event sebelumnya meliputi grup1/4/8/16/32/64, budget CPU/IO nol, confidence rendah, scratch nol, invalidindex/permutation dan overflow. Seluruh simulator, selector cursor, Decision/Error dan metrik dibandingkan. Benchmark memvalidasi 648 record dan seluruh field non-timing identik pada 648 pasangan dengan executable baseline; dua subprocess juga identik non-timing. Unit dan CSV bukan bukti formal.

## 16. HASIL FAULT INJECTION

Test reservation dan injected TASK010 dijalankan kembali dengan --nocapture. Hook error dan panic lima fase mempertahankan simulator dan cursor; expected panic tercatat pada log dan test lulus. Validasi try_reserve capacity-overflow dibedakan dari real ENOMEM. Same candidate/access index diuji dengan publish terhadap oracle.

## 17. HASIL QUALITY GATES

Setelah perubahan Rust terakhir: cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace, cargo check --workspace, cargo build --workspace --release, cargo build -p xynoos-aura --examples --release seluruhnya exit0. Log task-011-quality-gates.txt.

## 18. HASIL BENCHMARK REGRESI

| Grup | Mode | Median batch ns | Inspeksi ns | Alloc count | Requested byte | Peak live byte |
|---:|---|---:|---:|---:|---:|---:|
| 1 | clone | 52467.5 | 30294.0 | 384.0 | 30720.0 | 240.0 |
| 1 | journal | 45189.0 | 29002.0 | 256.0 | 20480.0 | 160.0 |
| 1 | fast | 51841.5 | 18646.5 | 128.0 | 10240.0 | 160.0 |
| 16 | clone | 591562.5 | 560349.5 | 4224.0 | 5406720.0 | 3840.0 |
| 16 | journal | 587756.0 | 564747.5 | 4096.0 | 5242880.0 | 2560.0 |
| 16 | fast | 297284.0 | 255734.0 | 128.0 | 163840.0 | 2560.0 |
| 64 | clone | 3774978.0 | 3730038.0 | 16512.0 | 84541440.0 | 15360.0 |
| 64 | journal | 3682821.5 | 3659883.5 | 16384.0 | 83886080.0 | 10240.0 |
| 64 | fast | 1168403.5 | 1107200.0 | 128.0 | 655360.0 | 10240.0 |

Angka ns per batch128; agregat 72 sample tiap ukuran/mode dari empat trace, tiga seed, tiga sample dan dua proses. Baseline TASK010 FastPath median grup1/16/64: 46110,5 / 293432 / 1266541,5 ns. TASK011: 51841,5 / 297284 / 1168403,5 ns. Nilai simulator (refault/migrasi/stall/occupancy) tetap cocok secara individual. Rata-rata terhadap nol atau speedup kernel tidak diklaim.

## 19. ANALISIS MEMORI DAN ALOKASI

Pada64 grup, clone 16512 requests/84541440 byte, journal16384/83886080 dan FastPath128/655360 per batch; peak live requested 15360/10240/10240 byte. RSS maksimum subprocess keseluruhan14976 KiB termasuk runtime dan fixture; bukan peak per-mode. Counter allocator mencakup overhead instrumen, bukan physical allocator footprint.

## 20. EVALUASI KESIAPAN FASTPATH

Default tetap oracle. Ekuivalensi bounded mendukung eksperimen namun tidak mencakup OOM abort, seluruh environment atau journal unwind. Recheck tambahan memperlihatkan grup1 baseline45975,5 dan typed54245,5 ns/batch, sementara16/64 hampir sama atau lebih rendah. Alloc count/byte tetap sama. nm -S -C memperlihatkan fast_event_inner tumbuh dari 0x46b ke 0x7cb byte, dan objdump menunjukkan stack frame 0x3d8 menjadi 0x788 byte. Objdump juga menunjukkan 498 instruksi dan 7 call memcpy, dibanding 295 instruksi dan 3 call baseline. Ini mengidentifikasi tambahan code generation dan copy yang konsisten dengan hipotesis overhead, tetapi belum membuktikan kontribusi kausalnya. Selisih sekitar65 ns/event pada ulangan bukan bukti kausal; waktu instrumen dan fixed order membatasi kesimpulan. Data recheck tersimpan.

## 21. ANALISIS RISIKO DAN KETERBATASAN

Oracle/phase formulas masih terduplikasi; type-state private tidak otomatis mencegah drift. Journal tidak panic-safe. Snapshot benchmark mengukur satu trace family sintetis per kelompok, bukan workload kernel. Dua proses dan tiga seed tidak menghasilkan interval kepercayaan. Actual OOM dan panic dari allocator tidak diuji. Tracker memiliki asumsi target dan batas concurrency sebagaimana audit.

## 22. KESIMPULAN

Type-state dan test publication menegaskan boundary tanpa mengubah hasil observable yang diuji. Quality gates lulus dan benchmark menunjukkan trade-off yang harus diteliti lebih lanjut sebelum default berubah. Tidak diklaim peningkatan performa Linux nyata.

## 23. REKOMENDASI TASK 012

Profil instruksi dan stack pada grup1 secara berpasangan acak, audit allocator independen, dan desain guard journal unwind-safe jika risiko sesuai. Pertahankan oracle sebagai default. Model yang sesuai ialah GPT-6.1 Terra untuk audit Rust dan differential verification; pengujian tetap wajib.

## 24. DAFTAR PUSTAKA

Sumber primer: selector.rs, physical.rs, tracker010.rs dan test repository; laporan TASK010; CSV TASK011. Referensi API Rust: https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html dan https://doc.rust-lang.org/std/panic/fn.catch_unwind.html .

## 25. LAMPIRAN DAN BUKTI

Artefak task-011-{raw.csv,summary.csv,baseline.csv,baseline-validation.txt,regression-recheck.csv,regression-investigation.txt,quality-gates.txt,reproducibility.txt,safety-audit.md,fault-injection.txt,memory.txt}. Jalankan enam gate; cargo test -p xynoos-aura injected_tests -- --nocapture; cargo test -p xynoos-aura reservation_tests -- --nocapture; cargo build -p xynoos-aura --examples --release; python3 scripts/benchmark_task011.py. Baseline dibuat git archive d082f60 ke /tmp, harness typed_research sama, cargo build --offline --release dengan target terpisah. Storage workspace awal220 MiB, akhir229 MiB; baseline sementara di /tmp tidak termasuk angka workspace. File ss tidak disentuh.
