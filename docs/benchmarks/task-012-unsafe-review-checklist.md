# Checklist kesiapan review unsafe

Ini adalah **self-audit oleh agent yang sama**, bukan review atau persetujuan independen. Tidak ada reviewer eksternal yang dilibatkan. Source ditinjau: `examples/support/tracker010.rs`; tabel operasi unsafe TASK011 tetap berlaku pada `task-011-safety-audit.md`.

| Aspek | Bukti dan batas | Status review |
|---|---|---|
| GlobalAlloc pointer/Layout/alignment | System menerima pointer dan Layout asli; alamat hanya disimpan sebagai integer, tidak didereferensi. Null realloc tidak menghapus record lama. | Diperiksa secara source |
| Sentinel, reuse, tombstone | 0/usizeMAX diasumsikan bukan pointer System valid pada target Linux; metadata synthetic mencakup same-address, reuse, tombstone dan table-full. | Terbatas pada target dan test |
| Scope, epoch dan deallocation | try_begin terkunci, menolak LIVE != 0; deallocation tracked sesudah disable tetap direkam. ENABLED global, bukan thread-local. Callback yang melintasi boundary dapat masuk epoch baru. | Keterbatasan diketahui |
| Spinlock, ordering dan reentransi | Acquire/Release melindungi register; System.realloc berjalan di dalam lock dan diasumsikan tidak masuk ulang ke wrapper. Reentransi dapat deadlock. | Perlu review eksternal |
| Concurrency dan counter | Uji 4 thread × 100 operasi dengan barrier TASK010; bukan bukti seluruh lifecycle. Overflow counter tidak diuji untuk workload tak terbatas. | Terbatas |
| Perubahan TASK012 | Tidak ada unsafe baru dalam library atau profiler. | Diverifikasi pencarian source |

Tidak ada high severity yang dapat direproduksi dalam workload terkontrol ini. Ini bukan pernyataan bahwa allocator bebas bug atau aman dipakai sebagai profiler produksi. Prioritaskan audit independen, backend interposition, reentransi dan epoch lifecycle sebelum perluasan scope.
