# Ketersediaan telemetry TASK 013

Pengamatan Codespaces Linux 6.8.0-1070-azure x86_64: `/proc/pressure/memory`, `/proc/meminfo`, dan `/proc/vmstat` tersedia dan dapat dibaca tanpa root. Lima snapshot final berstatus `ok` dan tidak parsial; baris `some` dan `full` tersedia. Nama VMStat yang diamati mencakup `pgfault`, `pgmajfault`, `pswpin`, `pswpout`, `pgscan_kswapd`, `pgscan_direct`, `pgsteal_kswapd`, `pgsteal_direct`, serta `workingset_refault_anon` dan `workingset_refault_file`.

Jika path tidak tersedia collector menghasilkan `Unsupported`; permission menghasilkan `Permission`; error parsing dipertahankan per sumber. Pengujian fixture path yang tidak ada memvalidasi snapshot parsial. CPU dan IO PSI tidak dikumpulkan dalam milestone ini. Observasi di Codespaces dapat mencerminkan cakupan procfs host atau namespace, sehingga tidak mewakili laptop tertentu atau batas cgroup secara otomatis.
