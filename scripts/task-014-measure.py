#!/usr/bin/env python3
"""Eksperimen berbatas; CSV hanya berisi metrik agregat sistem."""
import csv, os, statistics, subprocess, tempfile, time, threading
from pathlib import Path
root = Path(__file__).resolve().parent.parent
binary = root / 'target/release/xynoos-cli'
out = root / 'docs/benchmarks'
cmd = [str(binary), 'system', 'sample', '--count', '5', '--interval-ms', '200']
for _ in range(2):
    subprocess.run([str(binary), 'system', 'snapshot'], stdout=subprocess.DEVNULL, check=True, timeout=10)
rows = []
runs = []
for run in range(5):
    start = time.monotonic_ns()
    with tempfile.TemporaryFile(mode='w+') as output, tempfile.TemporaryFile(mode='w+') as errors:
        proc = subprocess.Popen(cmd, stdout=output, stderr=errors)
        watchdog = threading.Timer(10, proc.kill)
        watchdog.start()
        if run == 4:
            time.sleep(.15)
            # Workload 150 ms CPU, 4 MiB RAM, 1 MiB I/O; file selalu dibersihkan.
            worker = '''import time,tempfile,os\nfrom pathlib import Path\np=Path('.')\nt=time.monotonic()+.15\nwhile time.monotonic()<t: pass\nb=bytearray(4*1024*1024)\nwith tempfile.NamedTemporaryFile(dir=p) as f:\n f.write(b[:1024*1024]);f.flush();os.fsync(f.fileno())\n'''
            subprocess.run(['python3', '-c', worker], cwd=root, check=True, timeout=5)
        _, status, usage = os.wait4(proc.pid, 0)
        watchdog.cancel()
        proc.returncode = os.waitstatus_to_exitcode(status)
        if proc.returncode:
            errors.seek(0)
            raise RuntimeError(errors.read())
        output.seek(0)
        data = list(csv.DictReader(output))
    elapsed = time.monotonic_ns() - start
    for sample in data:
        sample['run'] = str(run)
        sample['workload'] = 'bounded' if run == 4 else 'idle'
        rows.append(sample)
    runs.append((run,elapsed, usage.ru_utime + usage.ru_stime, usage.ru_maxrss))
with (out/'task-014-raw.csv').open('w', newline='') as f:
    w=csv.DictWriter(f,fieldnames=['run','workload']+list(rows[0].keys())[:-2],lineterminator='\n');w.writeheader();w.writerows(rows)
with (out/'task-014-summary.csv').open('w',newline='') as f:
    w=csv.writer(f,lineterminator="\n");w.writerow(['run','workload','wall_ns','collector_cpu_seconds','collector_peak_rss_kib','median_snapshot_ns','max_snapshot_ns','samples'])
    for run,wall,cpu,rss in runs:
        values=[int(x['snapshot_ns']) for x in rows if int(x['run'])==run]
        w.writerow([run,'bounded' if run==4 else 'idle',wall,f'{cpu:.6f}',rss,int(statistics.median(values)),max(values),len(values)])
print('rows',len(rows),'median_snapshot_ns',statistics.median(int(x['snapshot_ns']) for x in rows),'idle_collector_cpu_s',sum(x[2] for x in runs[:4]),'collector_peak_rss_kib',max(x[3] for x in runs),'csv_bytes',(out/'task-014-raw.csv').stat().st_size)
