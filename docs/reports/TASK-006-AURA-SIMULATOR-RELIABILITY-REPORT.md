## 1. HALAMAN IDENTITAS

Xynoos HyperCore — TASK 006 Completion. AURA Simulator Reliability. Tanggal 15 November 2026; milestone penelitian userspace0.1.0; status SELESAI.

## 2. ABSTRAK

Diselesaikan inspeksi state-aware, journal eksperimental dua grup, peak total memori simultan dan eksperimen yang memisahkan accounting dari stall. Sebanyak39 unit test serta seluruh quality gate lulus. Eksperimen567 kombinasi diulang pada dua proses dengan metrik simulasi identik; journal dan clone identik pada81 pasangan eksperimen. Biaya inspeksi cukup besar dan keuntungan selector tidak universal.

## 3. PENDAHULUAN

Pekerjaan melanjutkan versi parsial tanpa menghapus source atau hasil lama. Sasaran ialah feasibility konsisten dengan commit, fairness protokol, failure atomicity serta bukti empiris yang dapat diaudit. Tidak ada integrasi kernel.

## 4. LANDASAN TEKNIS

Eligibility adalah izin umum berpindah, feasibility adalah kelayakan suatu target menurut state, dan pilihan policy adalah optimasi yang dapat memilih Keep sekalipun target feasible. Progress pemeriksaan tidak menjamin migrasi. Scratch merupakan buffer tujuan sementara ketika sumber masih hidup, berbeda dari permanent occupancy dan RSS.

## 5. AUDIT DAN CHECKLIST

Audit membaca Git status, selector/physical dan transaksi, laporan TASK006 parsial serta CSV TASK005/006 dan kalibrasi TASK003. Perubahan lokal seluruhnya dipertahankan.

| Kriteria tertunda | Hasil final |
|---|---|
| State-aware feasibility | preview event commit yang sama |
| Bounded/non-stagnating | satu putaran n≤64, cursor berotasi, test |
| Fairness mask | tidak ada mask eksogen berbeda pada eksperimen final |
| Peak scratch | dicatat sebelum sumber dilepas, test exact |
| Stall/accounting terpisah | baseline, accounting_only, stall_only, combined |
| Alternatif transaksi | journal dua grup dan differential test |
| Kalibrasi5 dataset | CSV proxy dari TASK003 |
| Quality/benchmark/dokumen | lulus dan disimpan |

Temuan awal: mask bounded lama berbeda dari baseline, counter penolakan bukan feasibility, timer tidak meliputi selector, peak total belum ada, dan clone O(n).

## 6. ANALISIS CANDIDATE STARVATION

Recency historis dapat selalu memilih grup backing yang sama. Selector state-aware mengevaluasi seluruh urutan permutation n grup, lalu memilih keputusan migrasi pertama menurut cursor. Cursor maju satu setiap event sehingga pada state stabil kandidat yang policy-nya bersedia memigrasikan diperiksa dan memperoleh prioritas dalam maksimal n event. Pada state dinamis tidak dijamin starvation hilang; setiap grup tetap diperiksa tiap pencarian. Kapasitas atau policy boleh menolak semua migrasi.

## 7. PERANCANGAN SELECTOR

state_aware memvalidasi permutation, menyalin simulator untuk setiap kandidat dan menjalankan event_candidate lengkap. Dengan demikian CPU/I/O setelah akses, promosi, cooldown, rate limit, eligibility, target tier dan kapasitas sama dengan commit. Simulator asal tidak berubah. Shared feasible dipakai diagnostic dan commit; diagnostic mencoba budget tak terbatas dengan fungsi sama untuk membedakan penolakan kapasitas dari CPU/I/O/scratch. last_feasible_mask bukan keputusan policy. Counter feasible menghitung kandidat dengan sedikitnya satu target layak; rejected menghitung kandidat tanpa target. Capacity/budget menghitung target ditolak, bukan kandidat, sehingga denominator berbeda. No feasible adalah pencarian dengan semua mask kosong; no_policy_migration adalah pencarian tanpa keputusan perpindahan meskipun target mungkin feasible.

## 8. MODEL ACCOUNTING DAN STALL

Physical permanent = resident_raw + resident_compressed + metadata. Peak total diperbarui pada setiap migrasi sebagai permanent_sebelum + scratch_tujuan, sebelum sumber dilepas; bukan peakPermanent+peakScratch. Journal menggunakan pembaruan yang sama. Budget permanent dan allowance scratch terpisah, sehingga total dapat melebihi budget permanent. Contoh test: initial66816+scratch512 menghasilkan67328. Backing payload tidak menambah permanent, tetapi scratch tujuan raw tetap dimodelkan; ini asumsi konservatif transfer buffer. Cache backing/allocator tidak diketahui. Accounting_only menonaktifkan globalshared guard (budget u64MAX), mempertahankan tierlimits dan denominator stall tetap. Stall_only memakai raw occupancy pada penalti tekanan, tanpa mengubah accounting. Combined mengubah keduanya. Komponen layanan refault tetap sama; ini ablation model, bukan perubahan cost policy.

## 9. DESAIN TRANSAKSI

event_journal menyimpan state, engine, metrics, tick serta dua profil: akses dan kandidat. step hanya dapat mengubah kedua profil tersebut; panjang Vec tidak berubah. Jika error, semua snapshot dikembalikan, termasuk saat kedua indeks sama. Clone tetap default. Journal O(1) metadata snapshot dan tidak mengalokasikan Vec profil; perubahan step di masa depan yang menyentuh grup lain wajib memperluas journal dan differential test.

## 10. IMPLEMENTASI

physical.rs menambah diagnostic mask, peak_total_bytes, journal serta override stall; selector.rs menambah state_aware/SearchStats. completion.rs adalah executable final baru. Source contoh historis dipertahankan, output final memakai nama task-006-final-* sehingga CSV parsial tidak tertimpa. Laporan parsial diarsip sebagai TASK-006-PRELIMINARY-REPORT.md. Tidak ada dependency baru.

## 11. METODOLOGI PENGUJIAN

Sembilan trace kompatibel TASK005: 0interactive,1background,2switching,3pressure,4entropy,5compressible,6bursty,7adversarial,8unreliable. Seed1/42/2026 dan512 event. Initial16 grup raw4KiB, metadata1280 byte, budget66816 byte, compressed tier16384/backing131072, scratch8192, CPU10000 ns, IO8192byte/tick. Semua policy menerima trace, urutan indeks eksogen dan parameter sama; feasibility berbeda sesuai state policy. Historical memilih recency terkecil, RR tick%16, bounded/journal memakai urutan0..15 berputar dan pemeriksaan nyata. Tujuh mode ×3policy ×3seed ×9trace=567. Bounded vs historical dengan policy tetap menguji efek protokol selector, termasuk pencarian hingga keputusan migrasi; policy comparison memakai bounded yang sama. Timer mulai sebelum pembuatan order/inspeksi sampai commit dan counter, tanpa pembuatan trace. Dua proses dijalankan berulang, tanpa inferensi statistik signifikansi.

## 12. HASIL UNIT TEST

39 test lulus (26 AURA termasuk baseline/selector,13 HyperMemory). Seluruh34 test lama dipertahankan. Tambahan5 test mencakup tiga journal/scratch dan dua state-aware. Differential sequence1024 event ×3policy mencakup invalid index; injection counter migrations=u64MAX menghasilkan error setelah mutasi parsial lalu seluruh state dipulihkan. Test purity,16 rejection, kandidat14 setelah banyak penolakan, overflow serta snapshot selector juga lulus. Test guard kapasitas/budget/cooldown/rate yang lama tetap lulus.

## 13. HASIL QUALITY GATES

cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo check --workspace; cargo build --workspace --release semuanya exit0 setelah perubahan source terakhir. Build seluruh contoh release lulus. Contoh research, validation dan selector_research dijalankan ke /tmp untuk smoke test, tanpa mengganti artefak historis. Log final task-006-final-quality-gates.txt.

## 14. HASIL BENCHMARK SELECTOR

| Trace | Mode | Refault | Migrasi | Stall ns | Evaluasi | Rejected | Feasible | No feasible | Peak total B | Waktu event ns |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | historical | 6.0 | 7.0 | 5150000.0 | 512.0 | 3.0 | 509.0 | 3.0 | 68224.0 | 122980.33 |
| 0 | bounded | 128.0 | 268.0 | 100000.0 | 8192.0 | 931.0 | 7261.0 | 0.0 | 66843.0 | 2120229.0 |
| 0 | round_robin | 66.0 | 144.0 | 100000.0 | 512.0 | 0.0 | 512.0 | 0.0 | 66843.0 | 122148.33 |
| 0 | journal | 128.0 | 268.0 | 100000.0 | 8192.0 | 931.0 | 7261.0 | 0.0 | 66843.0 | 2053481.0 |
| 0 | accounting_only | 128.0 | 268.0 | 100000.0 | 8192.0 | 931.0 | 7261.0 | 0.0 | 66843.0 | 2190787.33 |
| 0 | stall_only | 128.0 | 268.0 | 30000.0 | 8192.0 | 931.0 | 7261.0 | 0.0 | 66843.0 | 2598616.0 |
| 0 | combined | 128.0 | 268.0 | 30000.0 | 8192.0 | 931.0 | 7261.0 | 0.0 | 66843.0 | 2175325.33 |
| 1 | historical | 27.0 | 46.67 | 5160000.0 | 512.0 | 63.67 | 448.33 | 63.67 | 68212.0 | 161356.33 |
| 1 | bounded | 397.67 | 512.0 | 1536666.67 | 8192.0 | 3702.33 | 4489.67 | 120.33 | 66843.0 | 2344309.67 |
| 1 | round_robin | 317.0 | 512.0 | 766666.67 | 512.0 | 66.33 | 445.67 | 66.33 | 66843.0 | 159468.0 |
| 2 | historical | 132.0 | 120.0 | 5810000.0 | 512.0 | 48.0 | 464.0 | 48.0 | 68224.0 | 138173.67 |
| 2 | bounded | 210.0 | 370.0 | 420000.0 | 8192.0 | 1285.0 | 6907.0 | 0.0 | 66843.0 | 2117134.33 |
| 2 | round_robin | 120.0 | 254.0 | 100000.0 | 512.0 | 12.0 | 500.0 | 12.0 | 66843.0 | 132315.33 |
| 3 | historical | 9.33 | 18.0 | 1546666.67 | 512.0 | 26.67 | 485.33 | 26.67 | 68203.0 | 97695.33 |
| 3 | bounded | 116.33 | 200.0 | 290000.0 | 8192.0 | 687.0 | 7505.0 | 0.0 | 66855.0 | 1967866.0 |
| 3 | round_robin | 95.67 | 163.33 | 273333.33 | 512.0 | 14.67 | 497.33 | 14.67 | 66855.0 | 109538.33 |
| 4 | historical | 0.0 | 0.0 | 5120000.0 | 512.0 | 0.0 | 512.0 | 0.0 | 66816.0 | 132912.33 |
| 4 | bounded | 0.0 | 0.0 | 5120000.0 | 8192.0 | 0.0 | 8192.0 | 0.0 | 66816.0 | 2399251.0 |
| 4 | round_robin | 0.0 | 0.0 | 5120000.0 | 512.0 | 0.0 | 512.0 | 0.0 | 66816.0 | 148818.0 |
| 5 | historical | 30.0 | 53.0 | 5160000.0 | 512.0 | 73.67 | 438.33 | 73.67 | 66843.0 | 118820.33 |
| 5 | bounded | 401.67 | 512.0 | 1556666.67 | 8192.0 | 4082.67 | 4109.33 | 144.33 | 66843.0 | 1913024.0 |
| 5 | round_robin | 321.33 | 512.0 | 736666.67 | 512.0 | 86.33 | 425.67 | 86.33 | 66843.0 | 125424.0 |
| 6 | historical | 8.0 | 12.33 | 5143333.33 | 512.0 | 17.33 | 494.67 | 17.33 | 68209.0 | 125399.33 |
| 6 | bounded | 293.0 | 295.33 | 1576666.67 | 8192.0 | 1019.33 | 7172.67 | 0.0 | 66843.0 | 2130779.0 |
| 6 | round_robin | 214.33 | 240.67 | 1063333.33 | 512.0 | 22.0 | 490.0 | 22.0 | 66843.0 | 136847.67 |
| 7 | historical | 446.0 | 446.0 | 3200000.0 | 512.0 | 0.0 | 512.0 | 0.0 | 68197.0 | 163095.67 |
| 7 | bounded | 312.0 | 512.0 | 2980000.0 | 8192.0 | 5081.0 | 3111.0 | 209.0 | 66855.0 | 2170832.67 |
| 7 | round_robin | 223.0 | 447.0 | 960000.0 | 512.0 | 223.0 | 289.0 | 223.0 | 66873.0 | 135365.33 |
| 8 | historical | 30.0 | 53.0 | 5160000.0 | 512.0 | 73.67 | 438.33 | 73.67 | 70912.0 | 155387.67 |
| 8 | bounded | 395.33 | 512.0 | 1523333.33 | 8192.0 | 3850.0 | 4342.0 | 129.67 | 70912.0 | 2465830.33 |
| 8 | round_robin | 277.33 | 476.33 | 513333.33 | 512.0 | 41.67 | 470.33 | 41.67 | 70912.0 | 167505.67 |

Angka adalah rata-rata tiga seed. evaluations/rejected/feasible bounded meliputi seluruh16 kandidat per event; historical/RR hanya kandidat terpilih. No feasible historical/RR berarti kandidat tunggalnya, bukan seluruh himpunan. Kolom no_policy_migration belum diinstrumentasi pada mode historical/RR (nol tidak menyatakan seluruh event bermigrasi); jangan dibandingkan untuk mode tersebut. Migration mencakup promosi akses dan placement, sehingga bisa lebih besar dari jumlah keputusan placement. Semua CPU/IO/restore/stall adalah simulasi; event_ns adalah waktu nyata seluruh loop selector+event, bukan latency kernel.

## 15. HASIL DIFFERENTIAL TEST TRANSAKSI

Unit test membandingkan hasil Decision/Error dan seluruh Simulator PartialEq untuk3072 pasangan event dengan invalid input; injection overflow juga lulus. CSV final membandingkan seluruh kolom non-timing clone bounded vs journal pada81 pasangan konfigurasi; identik. Perubahan waktu kecil tidak cukup untuk menyatakan speedup journal karena preview masih memakai clone dan mendominasi biaya. Tidak ada uji panic unwind atau allocator OOM.

## 16. ANALISIS KOMPLEKSITAS

Keputusan3 tier O(1); clone event O(n) waktu/ruang, journal event O(1) snapshot (di luar operasi step yang bounded3target). State-aware mem-preview n event clone, sehingga O(n²) waktu dan O(n) peakspace, n≤64. Semua kandidat diperiksa, biaya tidak gratis. Counter lokal dibatasi maksimal n×2 target sehingga penjumlahan lokal tidak dapat mendekati overflow. Cursor/kelengkapan order diperiksa.

## 17. ANALISIS KEAMANAN

Userspace safe Rust, tanpa perubahan kernel/swap/device. Feasibility hanya preview state clone. Journal rollback pada Result::Err telah diuji, tidak menjamin pemulihan saat process abort. State publik yang dimutasi langsung tetap memerlukan invariant. Budget fisik simulasi tidak membatasi RSS proses.

## 18. KETERBATASAN DAN ANCAMAN VALIDITAS

History vs bounded juga berbeda jumlah kandidat yang diinspeksi, jadi perbandingan mengukur keseluruhan protokol bukan urutan murni. Bounded dapat menambah refault meski menurunkan stall, tidak mendominasi baseline. Sequentialfixedorder dan tiga seed tidak memberi confidenceinterval. Five-dataset proxy adalah rata-rata halaman, tanpa distribusi cost; mixed memakai rata-rata, bukan prediksi halaman individual. Metadata model80byte/grup adalah asumsi fixture, bukan layout storage48byte TASK003. CPU proxy dari walltime codec, bukan biaya reclaim. Ekspansi accounting_only tetap dibatasi tiercaps; tidak meniru semua detail TASK004. Preview sangat mahal untuk skala besar; tidak direkomendasikan langsung untuk kernel.

## 19. KESIMPULAN

Kriteria TASK006 terselesaikan dengan selector berbasis feasibility commit, validasi state/error, journal eksperimental, pengukuran scratch simultan dan eksperimen terpisah. Hasil tidak membuktikan peningkatan performa sistem; clone tetap default.

## 20. REKOMENDASI TASK007

Evaluasi prevalidation access sekali dan inspection kandidat tanpa replay event penuh; pertahankan equivalence journal. Tambahkan banyak pengulangan proses, latencydistribution, workload userspace dan kalibrasi per-page sebelum menghubungkan ke HyperMemory. TASK007 belum dimulai.

## 21. DAFTAR PUSTAKA

Source primer physical.rs, selector.rs, completion.rs dan test repository; docs/benchmarks/task-003-summary.csv; laporan TASK005 dan laporan TASK006 parsial yang diarsip. Tidak mengklaim dukungan paper atau hasil kernel eksternal.

## 22. LAMPIRAN DAN KALIBRASI

Kalibrasi lima dataset: compressed bytes/page = round(payload_bytes/256); compress/decompress proxy_ns = round(4096×10^9/(codec_MiB_s×1048576)). Source baris backend lz4 TASK003; satuan walltime ns, termasuk alokasi codec. Data di bawah:

```csv
dataset,compressed_bytes_per_page,compress_proxy_ns,decompress_proxy_ns
zero,27,718,1978
pattern,39,727,4412
structured,1381,3236,2612
entropy,4114,1354,1977
mixed,1390,1567,2816
```

Halaman fixture memakai lima profil bergiliran; entropy semua profilentropy, compressible semua zero. Restore backing100000ns, pressure/bobot/threshold tetap asumsi. Tidak ada ukuran RSS yang dikalibrasi. CSV final mentah567 record/ringkasan189record, kalibrasi5record, log gates dan reproducibility pada docs/benchmarks. Perintah: cargo build -p xynoos-aura --examples --release; target/release/examples/completion > docs/benchmarks/task-006-final-raw.csv; python3 scripts/summarize_task006.py. Dua proses identik non-timing, pasangan ringkasan tidak dipakai sebagai bukti per-event: validasi memakai81 pasangan konfigurasi mentah. Storage awal162MiB, akhir171MiB; tersedia17836335104byte pada pengukuran. Tidak ada commit/push atau penghapusan hasil lama.
