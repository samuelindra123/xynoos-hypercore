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
