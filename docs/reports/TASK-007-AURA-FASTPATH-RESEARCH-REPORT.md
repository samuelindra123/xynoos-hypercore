## 1. HALAMAN IDENTITAS

Xynoos HyperCore TASK007 — AURA FastPath. Tanggal15 November2026. Base source6276c3b, branch research/task-007-aura-fastpath. Implementasi dan validasi teknis selesai; status publikasi/merge diverifikasi melalui PR. Milestone penelitian userspace0.1.0.

## 2. ABSTRAK

FastPath menghilangkan replay akses per kandidat dengan prepared state dan inspeksi snapshot konstan.42 test dan enam gate lulus.1944 record benchmark diverifikasi; seluruh metrik non-timing FastPath dan journal identik dengan bounded oracle. Pada interactive/AURA median waktu batch512 turun dari2180598,5 menjadi1191113,5 ns. Ini pengukuran userspace lokal, bukan peningkatan performa kernel.

## 3. PENDAHULUAN

Task mempertahankan semantik TASK006, termasuk error, counter diagnostic, fallback dan peak scratch. Optimasi hanya jalur eksekusi. Tidak mengubah cost model, trace atau kebijakan penempatan.

## 4. LANDASAN TEKNIS

Prepared state memisahkan layanan akses dari keputusan placement. Snapshot metadata kecil dapat dipulihkan setelah setiap inspeksi, sementara simulator committed tidak disentuh sampai semua validasi sukses. Differential testing memeriksa seluruh struktur, bukan hanya jumlah migrasi.

## 5. AUDIT TASK006

step melakukan tick/pressure, akses/refault, promosi dan pengurangan budget, lalu diagnostic feasibility, policy, placement, stall, peak dan pemulihan budget. Selector state_aware meng-clone simulator, kemudian event_candidate meng-clone lagi; keduanya mengulang seluruh akses untuk tiap kandidat. File ss lokal tak terlacak berukuran3314 byte dipertahankan dan tidak dimasukkan task. Main remote6276c3b cocok milestone; source step/state_aware oracle tetap utuh.

## 6. ANALISIS BOTTLENECK

Untuk n grup: oracle melakukan2n clone Vec pada preview dan1 pada commit, serta n+1 layanan akses. Journal masih2n clone preview tetapi commit dua grup. Setiap Vec clone menyalin n profil, menyebabkan O(n²). Ini hitungan berdasarkan call graph, bukan pengukuran allocator instrumented.

## 7. PERANCANGAN FASTPATH

PhaseA: satu clone transaksi, prepare_access sekali. PhaseB: inspect_prepared menyimpan state/engine/metrics dan satu profil kandidat, menjalankan complete_candidate, mengambil hasil lalu memulihkan snapshot. PhaseC memilih kandidat pertama yang diputuskan bermigrasi dalam urutan/cursor yang sama dan mengakumulasi SearchStats identik. PhaseD menjalankan complete_candidate satu kali dan memindahkan prepared simulator ke committed state. Bila error, prepared dibuang; selector cursor juga tidak berubah.

## 8. MODEL STATE TRANSITION

Prepare menaikkan tick, pressure dan accesscounter, melayani refault, memakai budget, mempromosikan bila layak, serta menghitung scratch. Inspection menjalankan diagnostic, Engine.decide, placement dan finalisasi terhadap salinan privat prepared. State/engine/candidate/metrics dipulihkan setelah preview, termasuk ketika indexakses==candidate. Commit mengembalikan budget config, menerapkan stall dan peak sekali. Semua formula checked/fallback memakai helper feasible/Engine yang sama. Source phase baru diturunkan dari oracle dengan duplikasi yang disengaja; drift perlu ditahan differential test.

## 9. ANALISIS KOMPLEKSITAS

Prepare clone O(n), n inspeksi O(1), commit O(1); total O(n), peakspace O(n). Di dalam loop tidak ada Vec clone/alokasi baru dari simulator; profil adalah80byte fixedfields. Engine/state/metrics clone adalah ukuran tetap. SearchStats menghitung n evaluasi yang sama; tidak melakukan early exit agar error kandidat berikutnya tetap sama dengan oracle. Clone oracle O(n²); journal hanya mengurangi biaya commit. Snapshot tambahan O(1); tidak ada lock atau contention internal.

## 10. IMPLEMENTASI

physical.rs menambahkan prepare_access, complete_candidate, inspect_prepared; selector.rs menambahkan fast_event dan fast_event_measured. Default event/state_aware tetap oracle. fastpath.rs executable eksperimen baru; benchmark_task007.py menjalankan subprocess, validasi dan merangkum. Tidak ada dependency baru atau unsafe. API baru tidak mengganti kontrak lama.

## 11. METODOLOGI PENGUJIAN

Ubuntu24.04 Codespaces, Linux6.8.0-1070-azure x86_64,4vCPU, rustc1.100.0. Sembilan trace kompatibel completion TASK006, seed1/42/2026,512event. Empatmode historical/bounded/journal/fast; mode dirotasi menurut seed, satu runwarmup penuh lalu3sample dari freshfixture. Dua subprocess menghasilkan1944 record (972/proses). Data/trace dibuat di luar timer; timer event mencakup order allocation, selection, commit dan counter. Inspection diukur terpisah: oracle meliputi replay+clone, FastPath hanya loop PhaseB. Historical inspection0 berarti tidak diinstrumentasi, bukan kerja nol. Median/p95 ringkasan atas18sample batch per trace/policy/mode; p95 batch bukan p95 operasi individual. Urutan tidak sepenuhnya randomized/Latin-square dan process count kecil.

## 12. HASIL CORRECTNESS

42 test workspace lulus:29AURA dan13HyperMemory. Seluruh39test lama tetap. Tiga test baru meliputi multi-event, injectionerror dan invalidorder. Semua occupancy, pageprofile, engine, tick, peak serta metrics dibandingkan dengan PartialEq. Implementasi default tidak diganti.

## 13. HASIL DIFFERENTIAL TEST

2304 event comparison (ukuran1/16/64 ×3policy ×256event), termasuk invalidindex setiap31event. Lima injection: tickmax, accesscountermax, migrationcountermax, bobotcostmax dan stallcountermax. Hasil error dan seluruh state cocok; rollback mengembalikan snapshot awal. Invalidpermutation ditolak. Pada benchmark setiap record FastPath/journal dibandingkan bounded dengan trace/seed/sample/proses sama; semua field non-timing identik. Kedua subprocess juga identik non-timing. Tidak ada klaim bukti formal, OOM atau panic-unwind injection.

## 14. HASIL PERFORMANCE BENCHMARK

| Trace | Policy | Mode | Median ns/event | p95 ns/batch512 | Refault mean | Migrasi mean |
|---|---|---|---:|---:|---:|---:|
| 0 | Aura | bounded | 4258.98 | 3947191 | 128 | 268 |
| 0 | Aura | journal | 4129.04 | 3749385 | 128 | 268 |
| 0 | Aura | fast | 2326.39 | 2044674 | 128 | 268 |
| 0 | Aura | historical | 215.24 | 198828 | 6 | 7 |
| 1 | Aura | bounded | 4023.34 | 3043542 | 397.67 | 512 |
| 1 | Aura | journal | 3972.11 | 3513644 | 397.67 | 512 |
| 1 | Aura | fast | 2036.02 | 1410899 | 397.67 | 512 |
| 1 | Aura | historical | 227.99 | 189052 | 27 | 46.67 |
| 2 | Aura | bounded | 4309.5 | 2437867 | 210 | 370 |
| 2 | Aura | journal | 4416.58 | 3196130 | 210 | 370 |
| 2 | Aura | fast | 2334.09 | 1661231 | 210 | 370 |
| 2 | Aura | historical | 241.62 | 190732 | 132 | 120 |
| 3 | Aura | bounded | 3495.99 | 2499771 | 116.33 | 200 |
| 3 | Aura | journal | 3406.1 | 1977073 | 116.33 | 200 |
| 3 | Aura | fast | 1804.35 | 942003 | 116.33 | 200 |
| 3 | Aura | historical | 164.06 | 91437 | 9.33 | 18 |
| 4 | Aura | bounded | 4127.17 | 3923698 | 0 | 0 |
| 4 | Aura | journal | 4046.2 | 3255826 | 0 | 0 |
| 4 | Aura | fast | 2366.05 | 1817666 | 0 | 0 |
| 4 | Aura | historical | 211.88 | 148898 | 0 | 0 |
| 5 | Aura | bounded | 3912.28 | 2674334 | 401.67 | 512 |
| 5 | Aura | journal | 3851.27 | 2742749 | 401.67 | 512 |
| 5 | Aura | fast | 1966.92 | 1638529 | 401.67 | 512 |
| 5 | Aura | historical | 225.46 | 203652 | 30 | 53 |
| 6 | Aura | bounded | 4330.92 | 2739749 | 293 | 295.33 |
| 6 | Aura | journal | 4257.66 | 4022538 | 293 | 295.33 |
| 6 | Aura | fast | 2349.96 | 2039011 | 293 | 295.33 |
| 6 | Aura | historical | 228.04 | 225350 | 8 | 12.33 |
| 7 | Aura | bounded | 3719.16 | 2766868 | 312 | 512 |
| 7 | Aura | journal | 3699.96 | 2853927 | 312 | 512 |
| 7 | Aura | fast | 1814.37 | 978828 | 312 | 512 |
| 7 | Aura | historical | 235.42 | 147662 | 446 | 446 |
| 8 | Aura | bounded | 4124.34 | 2194343 | 395.33 | 512 |
| 8 | Aura | journal | 4069.36 | 2866349 | 395.33 | 512 |
| 8 | Aura | fast | 2155.87 | 1187449 | 395.33 | 512 |
| 8 | Aura | historical | 232.63 | 131656 | 30 | 53 |

Interactive/AURA median batch bounded2180598,5ns, journal2114070ns, Fast1191113,5ns; inspection median2055535,5/2023999,5/1054441ns. Perbedaan terukur berlaku eksekusi ini saja. Historical110201ns tetapi semantiknya berbeda (kandidat tunggal) sehingga bukan baseline ekuivalen. Semua angka simulasi (refault/stall/CPU/IO/memori) tetap identik untuk tiga boundedmode. Waktu tersebut mencakup overhead Instant dan counter.

## 15. ANALISIS ALOKASI MEMORI

Oracle: model hitungan2n+1 Vecclone, journal2n, fast1. Ukuran grup sesuai fixture16, benchmark n16. Alokasi orderVec satu per event tetap pada semua mode, tidak dioptimasi pada task ini. Tanpa customallocator maka jumlah alokasi heap aktual tidak diukur. Scratch model tidak berubah dan bukan alokasi buffer nyata. resource.getrusage(RUSAGE_CHILDREN) melaporkan peakRSS maksimum16628KiB seluruh subprocess benchmark; dapat mencakup pre-exec inheritance dan tidak dapat diatribusikan per-mode. /usr/bin/time tidak tersedia, tidak diinstal. PeakRSS bukan ukuran preparedsnapshot.

## 16. PERBANDINGAN CLONE JOURNAL DAN FASTPATH

Oracle tetap referensi dan default. Journal mempertahankan replaycandidate; FastPath mengurangi layananakses menjadi1. Keduanya memakai helperfase yang sama hanya pada FastPath, sedangkan oracle lama dipertahankan terpisah untuk differential. Perbandingan waktu journal vsfast meliputi persiapan, seleksi dan commit, bukan kernel.

## 17. ANALISIS RISIKO

Duplikasi oracle/fase berpotensi drift. Perubahan field atau mutasi lain perlu memperluas snapshot; test equivalence harus tetap. Error pada kandidat setelah kandidatpilihan ditemukan tetap diperiksa agar semantik gagal tidak berubah. Tidak ada perubahan kernel, swap, device atau hardware; benchmark dibatasi.

## 18. KETERBATASAN DAN ANCAMAN VALIDITAS

Hanya synthetic traces, fixedcost,16grup pada benchmark;64grup diuji correctness bukan performance. Modeorder dirotasi terbatas; dua proses dan18batch tidak cukup untuk generalisasi hardware. Hitungan clone/allocation berdasarkan source, bukan profil allocator. RSS agregat. Model scratch/metadata dan fallback tetap asumsi TASK006; tidak diklaim tervalidasi kernel. Default belum dipindahkan keFastPath meskipun equivalence terbatas lulus.

## 19. KESIMPULAN

Akses diproses sekali dan inspeksi tidak lagi mengclone/replay seluruh simulator. Kompleksitas O(n) berasal dari analisis loop dan struktur fixed-size; pengukuran lokal menunjukkan penurunan waktuevent ekuivalen pada contoh yang dilaporkan. Semua quality gate lulus; status merge ditentukan bukti remote setelah PR.

## 20. REKOMENDASI TASK008

Tambahkan profiling alokasi yang terisolasi, benchmark size1..64 dan lebih banyak subprocess, serta validasi prepared state melalui type-state tanpa menyalin rumus oracle. Evaluasi penggantian default setelah pengujian lebih luas. Model AI yang sesuai: model coding Rust dengan kemampuan differentialtesting, misalnya GPT-6.1 Terra; rekomendasi operasional, bukan hasil benchmark model.

## 21. DAFTAR PUSTAKA

Sumber primer: source physical/selector dan contoh completion TASK006 pada6276c3b; laporan TASK006; CSVkalibrasi TASK003; source baru dan artefak TASK007 repository ini. Tidak ada paper atau benchmark kernel eksternal yang diklaim.

## 22. LAMPIRAN DAN BUKTI

Reproduksi: cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo check --workspace; cargo build --workspace --release; cargo build -p xynoos-aura --examples --release; python3 scripts/benchmark_task007.py. Artefak docs/benchmarks/task-007-{raw.csv,summary.csv,quality-gates.txt,reproducibility.txt,process-memory.txt}. Base6276c3b, identitas commitpenelitian dan PR dicatat GitHub. Storage awal171MiB, akhir sekitar181MiB, ruangtersisa17826861056byte pada pengukuran. Source/artefak/taskreport saja masuk commit; ss dan target tidak masuk. Workflow memakai squashmerge normal tanpa bypass; proteksi REST sempat403, GraphQL branchProtectionRule null dan rules endpoint kosong, finalstatus diperiksa melalui PR.
