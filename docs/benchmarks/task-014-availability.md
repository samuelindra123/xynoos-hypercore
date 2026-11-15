# Ketersediaan aktual, 2026-11-15

- PASS: /proc/stat, /proc/pressure/{memory,cpu,io}, /proc/meminfo, /proc/vmstat dan /proc/diskstats dapat dibaca dan diparse.
- PASS: cgroup v2 membership dan cpu.stat, memory.current, io.stat terbaca; cakupan cgroup bukan seluruh host.
- PASS: DRM card0 ditemukan dengan vendor 0x1414; keberadaan device bukan bukti GPU fisik, utilisasi, decode atau akselerasi.
- NOT VERIFIED: GPU utilization, video decode dan acceleration: tidak ada sumber andal diimplementasikan.
- NOT VERIFIED: thermal zones dan power supplies tidak ditemukan; pembacaan sensor fisik dan unit pada hardware nyata belum dapat diuji.
- NOT VERIFIED: cpufreq CPU0 tidak tersedia; mesin virtual dan host lain dapat berbeda.
- PARTIAL: hasil directory scan dibatasi 256 entri dan output CSV hanya memuat device diskstats pertama, sehingga tidak mewakili seluruh aktivitas block device.
