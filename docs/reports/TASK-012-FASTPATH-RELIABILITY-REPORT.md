## 1. HALAMAN IDENTITAS

Xynoos HyperCore — TASK 012: FastPath Reliability. Tanggal 15 November 2026. Baseline main 8f9824c95d2e9c7dccc1ebd399e0c401b34191cc; pembanding performa TASK010 d082f60ade87b30697fa03384ace1a8089f036d3. Milestone riset userspace 0.1.0. Status teknis SELESAI; status GitHub diverifikasi terpisah.

## 2. ABSTRAK

Penelitian mengukur ulang dugaan regresi FastPath dan mengimplementasikan rollback guard journal berbasis safe Rust. Lima puluh empat unit test dan enam quality gate lulus. Eksperimen ABBA menghasilkan 1.296 record dengan metrik simulasi ekuivalen antara baseline TASK010 dan versi saat ini. Pada satu grup median batch128 FastPath berubah dari 44.012,5 menjadi 52.112 ns; inspeksi hampir sama dan penyebab kausal belum terbukti. Audit allocator adalah self-audit, bukan review eksternal.

## 3. PENDAHULUAN

TASK011 menunjukkan perbedaan waktu dan potensi rollback journal yang tidak aman ketika unwind. Penelitian ini mengutamakan correctness dengan eksperimen terbatas dan tetap mempertahankan oracle sebagai default.

## 4. LANDASAN TEKNIS

Rust Drop saat unwinding memungkinkan guard memulihkan state jika proses tidak abort. Benchmark batch mengukur waktu userspace termasuk instrumentasi. Requested bytes berbeda dari heap fisik dan RSS.

## 5. AUDIT TASK 011

Status Git main dan remote diverifikasi pada 8f9824c; file lokal ss tidak terlacak dipertahankan. Prepared dan CommitReady membatasi publikasi; FastPath memproses akses sekali. Journal memutasi simulator in-place dan sebelumnya rollback hanya saat Err. Tracker010 menggunakan GlobalAlloc, register statis dan spin lock. Laporan TASK011 menunjukkan perbedaan code generation, bukan penyebab kausal.

## 6. ANALISIS REGRESI FASTPATH

Pada grup1, TASK010 baseline dan current dengan harness sama menghasilkan median 44.012,5 vs 52.112 ns per128 event; rentang masing-masing 32.250–76.332 dan 39.945–135.562 ns. Inspeksi median 18.736 vs 18.772,5 ns. Hasil ini menunjukkan selisih pada pengukuran, bukan bukti regresi kernel. Investigasi lengkap task-012-regression-investigation.md.

## 7. HIPOTESIS DAN METODOLOGI

Hipotesis: pemindahan Prepared/CommitReady menambah copy stack di luar inspeksi. ABBA memakai executable TASK010 dan current dengan compiler/flags/harness identik, fresh subprocess, warm-up, tiga seed dan empat workload. Alternatif yang belum dieliminasi: urutan fixed di dalam executable, efek allocator dan penjadwalan. Hasil tidak cukup untuk menentukan kausalitas; tidak ada optimasi FastPath semantik yang dipaksakan.

## 8. HASIL PROFILING

Selisih event minus inspeksi dihitung per record dan mencakup prepare, commit, timer dan bookkeeping; bukan pengukuran langsung masing-masing fase. Median grup1 baseline/current 25.738,5/33.251 ns per batch. Profil sampling CPU dan hardware counters tidak dijalankan. Data fase tersimpan task-012-phase-estimates.csv.

## 9. EVALUASI CODE GENERATION

nm -S -C menunjukkan fast_event_inner ukuran 0x46b baseline vs 0x7cb current. Audit TASK011 menemukan frame stack 0x3d8 vs 0x788 dan call memcpy statis 3 vs7 pada executable tersebut. Temuan konsisten dengan hipotesis copy, tetapi statis dan tidak menunjukkan frekuensi eksekusi; keyakinan kausal rendah.

## 10. IMPLEMENTASI OPTIMASI

Tidak ada optimasi FastPath baru karena penyebab belum terbukti. Oracle dan Prepared/CommitReady dipertahankan. Perubahan implementasi terbatas pada guard journal dan test; model biaya, kebijakan, public API dan default tidak diubah.

## 11. DESAIN JOURNAL ROLLBACK GUARD

JournalGuard meminjam Simulator secara eksklusif dan menyimpan Option snapshot state, engine, metrics, tick, dua indeks dan dua profil. Pada Err atau unwind Drop memulihkan snapshot; commit mengosongkan Option hanya setelah step dan hook pra-commit berhasil. Fixed-size snapshot tidak clone Vec. Default event tetap clone.

## 12. ANALISIS PANIC-UNWIND SAFETY

Test menginjeksi panic sebelum mutasi, setelah akses profil, setelah Engine.decide, setelah placement/counter dan sebelum commit, untuk access/candidate sama dan berbeda. Seluruh Simulator dibandingkan dengan snapshot. Drop memakai get_mut sehingga tidak panic pada index; correctness bergantung invariant step tidak mengubah panjang pages atau grup selain dua indeks. Field snapshot saat ini tanpa Drop kustom yang panic. Ini jaminan terbatas untuk unwind jalur diuji, bukan panic=abort, allocator abort atau perubahan masa depan. Cursor selector eksternal bukan milik journal; test memakai staged selector dan menerbitkannya hanya setelah sukses.

## 13. UNSAFE REVIEW CHECKLIST

Checklist source tracker010, scope global, epoch, tombstone, pointer reuse, realloc, sentinel, ordering, reentrancy dan concurrency tersedia task-012-unsafe-review-checklist.md. Review dilakukan agent yang sama; tidak ada reviewer independen. System.realloc dalam spinlock bergantung pada non-reentrancy backend dan menjadi risiko untuk perluasan. Tidak ada unsafe baru dalam library atau temuan high severity yang terbukti pada scope ini.

## 14. METODOLOGI PENGUJIAN

Ubuntu24.04, Linux6.8 Azure x86_64, rustc1.100.0,4vCPU. Trace interactive/entropy/adversarial/unreliable; grup1/16/64, seed1/42/2026, mode clone/journal/fast, 128 event, satu warm-up dan tiga sample, dua proses per versi ABBA. Mode dirotasi menurut ukuran+seed pada harness. Dua versi ×2proses ×4trace ×3ukuran ×3seed ×3mode ×3sample=1296 record. Semua simulasi dibandingkan ke baseline pada key identik; pengujian unit meliputi enam ukuran.

## 15. HASIL UNIT TEST

54 unit test lulus:41 AURA dan13 HyperMemory; satu compile-fail doctest lulus. Seluruh51 unit test lama dipertahankan. Tiga test baru memeriksa 10 kombinasi phase/index panic, rollback overflow dan rollback staged cursor/capacity error.

## 16. HASIL DIFFERENTIAL TEST

Test lama membandingkan oracle dan FastPath untuk1/4/8/16/32/64 grup, invalid index/order, budget, cooldown, fallback, scratch dan overflow, termasuk seluruh state/cursor. Test journal/clone lama tetap lulus setelah guard. Semua field simulasi pada1296 record identik terhadap TASK010 untuk key yang sama; allocator dan timing tidak dipaksa identik.

## 17. HASIL FAULT INJECTION

Log task-012-fault-injection.txt memuat tiga guard test dengan expected panic, targeted FastPath differential dan reservation tests. Hook0..4 memvalidasi rollback saat unwind. Overflow migration counter dan Capacity dari checked_sub diuji setelah perubahan state; state awal yang sengaja tidak konsisten tetap persis sama setelah error. AllocationReservation tercakup pada test reservation FastPath; journal sendiri tidak melakukan reservasi dan tidak mengeluarkan error tersebut.

## 18. HASIL QUALITY GATES

Setelah perubahan Rust final, fmt --check, clippy workspace all-targets -D warnings, test workspace, check workspace, build workspace release dan build examples release semua exit0. Log lengkap tersedia. Clippy awal menolak tuple snapshot kompleks; diperbaiki dengan type alias lalu semua gate diulang.

## 19. HASIL BENCHMARK

| Grup | Mode | Versi | Median ns/batch | Min–maks ns/batch | Inspeksi median ns | Alloc count | Peak live B |
|---:|---|---|---:|---:|---:|---:|---:|
| 1 | clone | baseline | 54026.0 | 38752–108011 | 29882.5 | 384.0 | 240.0 |
| 1 | clone | current | 52663.5 | 38381–74349 | 29809.5 | 384.0 | 240.0 |
| 1 | journal | baseline | 45033.5 | 34043–97973 | 27648.0 | 256.0 | 160.0 |
| 1 | journal | current | 45945.5 | 35076–78767 | 27663.0 | 256.0 | 160.0 |
| 1 | fast | baseline | 44012.5 | 32250–76332 | 18736.0 | 128.0 | 160.0 |
| 1 | fast | current | 52112.0 | 39945–135562 | 18772.5 | 128.0 | 160.0 |
| 16 | clone | baseline | 588447.0 | 534587–1056681 | 556499.0 | 4224.0 | 3840.0 |
| 16 | clone | current | 569652.0 | 518667–1060357 | 538589.0 | 4224.0 | 3840.0 |
| 16 | journal | baseline | 577206.0 | 527814–1087959 | 555777.0 | 4096.0 | 2560.0 |
| 16 | journal | current | 565304.0 | 513598–1063423 | 544720.0 | 4096.0 | 2560.0 |
| 16 | fast | baseline | 296663.0 | 279060–509019 | 260000.0 | 128.0 | 2560.0 |
| 16 | fast | current | 312823.5 | 285001–511283 | 264106.5 | 128.0 | 2560.0 |
| 64 | clone | baseline | 3779317.0 | 3452337–6798686 | 3733144.5 | 16512.0 | 15360.0 |
| 64 | clone | current | 3798162.0 | 3439903–6543781 | 3753143.0 | 16512.0 | 15360.0 |
| 64 | journal | baseline | 3812904.0 | 3488123–6648216 | 3790754.5 | 16384.0 | 10240.0 |
| 64 | journal | current | 3603083.5 | 3297007–4583245 | 3579144.0 | 16384.0 | 10240.0 |
| 64 | fast | baseline | 1194401.5 | 1039609–1589674 | 1142152.5 | 128.0 | 10240.0 |
| 64 | fast | current | 1240112.5 | 1033246–1877451 | 1178331.5 | 128.0 | 10240.0 |

Nilai runtime ns per batch128; requested byte dan allocation count tersedia pada CSV lengkap. Metrik simulator adalah estimasi model, bukan waktu nyata. Median/rentang di atas agregasi empat trace dan tiga seed; jangan anggap setiap workload mempunyai median yang sama.

## 20. ANALISIS MEMORI DAN ALOKASI

Pada64 grup median allocated requests clone/journal/fast 16512/16384/128 dan peak live requested 15360/10240/10240 byte per batch. Requested bytes kumulatif dan peak scope tidak termasuk metadata register statis, allocator overhead, fixture atau RSS total. Peak RSS maksimum subprocess baseline/current masing-masing 17408/16768 KiB, mencakup seluruh proses dan pre-exec; bukan per mode. Tidak dibuktikan ada perubahan RSS yang signifikan.

## 21. EVALUASI DEFAULT FASTPATH

Oracle tetap default. Diferensial bounded lulus tetapi regresi satu grup masih memerlukan profiler kausal dan lebih banyak pengulangan; jurnal punya guard eksperimen yang diuji namun belum membuktikan safety terhadap abort atau perubahan struktur masa depan. Tidak ada alasan untuk mengubah default hanya dari benchmark.

## 22. RISIKO DAN KETERBATASAN

Tracker process-global, sentinel Linux dan System non-reentrant assumptions masih memerlukan review independen. Runtime semua mode terinstrumentasi; uninstrumented dan persiapan/commit individual tidak diukur. Fixed order dalam executable dan hanya empat proses total membatasi inferensi. Noise host, hanya trace sintetis serta seluruh ukuran grup1/16/64 diuji pada benchmark; tidak ada klaim kernel nyata. Peak scratch adalah model simulator, bukan alokasi fisik.

## 23. KESIMPULAN

Journal guard memenuhi rollback Result dan unwind pada skenario yang diuji. Regresi grup1 masih teramati dengan urutan ABBA dan tambahan noninspection, sementara penyebab kausal belum terbukti. Tidak dibuat klaim bahwa type-state menyebabkan regresi atau bahwa kernel lebih cepat.

## 24. REKOMENDASI TASK 013

Lakukan profiling instruction/stack dengan sampling yang aman, pasangan urutan lebih banyak, baseline current dan uninstrumented, serta review allocator independen. Pertimbangkan API staged selector+journal untuk cursor; pertahankan default oracle hingga bukti memadai. Model AI sesuai: GPT-6.1 Terra untuk Rust dan verifikasi, bukan pengganti review independen.

## 25. DAFTAR PUSTAKA

Sumber primer: physical.rs, selector.rs, tracker010.rs, typed_research.rs, laporan TASK011 dan CSV TASK012. Referensi Rust: https://doc.rust-lang.org/std/ops/trait.Drop.html dan https://doc.rust-lang.org/std/panic/fn.catch_unwind.html . Tidak mengklaim hasil benchmark kernel atau review eksternal.

## 26. LAMPIRAN DAN BUKTI

Perintah: enam quality gates; cargo test -p xynoos-aura unwind_guard_tests -- --nocapture; cargo test -p xynoos-aura fast_tests; cargo test -p xynoos-aura reservation_tests; python3 scripts/benchmark_task012.py. Artefak task-012-raw.csv, summary.csv, phase-estimates.csv, quality-gates.txt, fault-injection.txt, reproducibility.txt, regression-investigation.md, journal-safety-audit.md, unsafe-review-checklist.md. Source berubah physical.rs, skrip benchmark, README, arsitektur, dokumen/artefak; data TASK001–011 dipertahankan. Storage awal229MiB dan akhir231MiB; saat pengukuran tersedia17772376064 byte. ss tidak disentuh. Tidak ada dependency baru. Commit dan PR dapat diverifikasi pada GitHub setelah publikasi.
