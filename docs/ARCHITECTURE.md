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
