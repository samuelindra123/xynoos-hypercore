# Investigasi regresi FastPath

Baseline dibangun offline dari Git `d082f60` dan current dari branch TASK012, dengan harness `typed_research.rs`, compiler serta profile release identik. Empat subprocess dijalankan dalam urutan baseline/current/current/baseline; setiap kombinasi memiliki satu warm-up dan tiga sample untuk masing-masing tiga seed. Empat trace dan tiga ukuran menghasilkan 72 batch per ukuran/mode/versi. Semua metrik simulasi identik; allocator tracker aktif pada keduanya.

Untuk satu grup, median batch 128 event: baseline 44.012,5 ns, current 52.112 ns. Rentang baseline 32.250–76.332 ns, current 39.945–135.562 ns. Inspeksi median 18.736 vs 18.772,5 ns. Median selisih event dikurangi inspeksi per record 25.738,5 vs 33.251 ns; nilai ini **mencakup** prepare, commit, timer, bookkeeping dan overhead lain, sehingga bukan pengukuran tersendiri dari salah satu fase.

Pada 16 grup median FastPath baseline/current 296.663/312.823,5 ns; pada 64 grup 1.194.401,5/1.240.112,5 ns. Rentang tumpang tindih dan hanya dua subprocess per versi; data tidak cukup untuk interval kepercayaan atau menggeneralisasi besaran regresi. Waktu terinstrumentasi tidak disamakan dengan runtime tanpa instrumentasi.

Pemeriksaan `nm -S -C` menunjukkan simbol `fast_event_inner` sebesar 0x46b byte di baseline dan 0x7cb byte pada current; audit TASK011 juga mencatat perbedaan stack dan jumlah call memcpy statis. Bukti konsisten dengan kemungkinan tambahan pemindahan prepared state di luar inspeksi, tetapi instruksi statis tidak menunjukkan jalur yang dieksekusi. Penyebab kausal belum terbukti; tidak ada profiler sampling/hardware counter yang digunakan. Tidak ada optimasi semantik yang dipaksakan. FastPath tetap experimental dan oracle tetap default.

Data: `task-012-raw.csv`, `task-012-summary.csv`, `task-012-phase-estimates.csv`. Rekonstruksi: `python3 scripts/benchmark_task012.py` setelah build examples release; Cargo baseline memakai `--offline` dan target sementara.
