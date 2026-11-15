# Audit metodologis unsafe — TASK 011

Audit dilakukan oleh engineer/agent yang sama pada sesi terpisah dari desain transaksi; **bukan audit eksternal independen**. Tidak ada unsafe baru pada library. Tabel mencakup implementasi allocator TASK 008–010 yang relevan. Bukti merujuk source dan pengujian aktual; tingkat risiko mengukur risiko residual penggunaan di luar scope penelitian.

| File | Fungsi | Operasi unsafe | Invariant yang diperlukan | Bukti | Severity | Rekomendasi |
|---|---|---|---|---|---|---|
| scalability.rs | GlobalAlloc::alloc | System.alloc | Layout valid/nonzero, null dicek, alignment diteruskan | self-check Box, TASK008 | Rendah | Tetap executable khusus |
| scalability.rs | dealloc | System.dealloc | pointer milik System dengan Layout asli tepat sekali | delegasi langsung | Rendah | Tidak memakai input yang dimanipulasi |
| scalability.rs | realloc | System.realloc | pointer/Layout lama benar, ukuran baru nonzero, null tidak dilepas dua kali | delegasi langsung | Rendah | Tidak menganggap count sebagai live heap |
| tracker.rs | alloc | System.alloc | Layout/align diteruskan; callback tanpa rekursi | benchmark TASK009 | Sedang | Hanya eksperimen terkontrol |
| tracker.rs | dealloc | System.dealloc | ownership/Layout tetap; removal bookkeeping tidak dereference | source; test lintas scope TASK009 | Sedang | Audit eksternal sebelum penggunaan lebih luas |
| tracker.rs | realloc | System.realloc | catat hanya hasil non-null; pointer lama tetap valid saat gagal | source/test metadata | Sedang | Jangan klaim transient physical peak |
| tracker010.rs | alloc | System.alloc | Layout asli, tidak merekam null; lock tidak reentrant | source, tracker_validation | Sedang | Batasi platform dan scope |
| tracker010.rs | dealloc | System.dealloc | Layout identik, satu pemilik; tracked sebelum atau sesudah scope | tracker_validation, live=0 | Sedang | Jangan reset tanpa quiescence |
| tracker010.rs | realloc | System.realloc | original tetap tracked pada null; same-address dan pointer reuse tepat | test metadata same-address/null, realloc System | Sedang | Uji allocator backend berbeda; jangan paksa ENOMEM global |

Alignment diberikan System; address hanya disimpan sebagai integer dan tidak dereference. Sentinel 0/usizeMAX diasumsikan tidak menjadi pointer yang valid pada target Linux ini; ini bukan jaminan portable umum. Tombstone mempertahankan urutan probing, table-full mengeset INVALID. Counter atomik relaxed dilindungi lock Acquire/Release untuk register, tetapi global ENABLED bukan isolasi thread; validasi empat thread terbatas bukan bukti keselamatan untuk seluruh lifecycle. System.realloc dipanggil di dalam spin lock: desain mengasumsikan backend System tidak masuk ulang ke wrapper; interposisi allocator/reentransi dapat deadlock. Begin hanya valid pada LIVE=0 dan lingkungan dikendalikan; callback yang telah memulai System.alloc saat epoch dimulai dapat diatribusikan ke epoch baru. Counter overflow tidak dibuktikan aman untuk penggunaan tak terbatas. Tidak ditemukan bukti kerentanan exploitable atau risiko tinggi yang terkonfirmasi pada workload yang diuji; audit ini tidak membuktikan ketiadaannya. Jangan pakai profiler sebagai allocator produksi.
