"""Jalankan dua subprocess release dan validasi ekuivalensi non-timing TASK007."""
import csv,io,subprocess,statistics,resource
from pathlib import Path
root=Path(__file__).resolve().parent.parent
rows=[]
for process in range(2):
    result=subprocess.run([str(root/'target/release/examples/fastpath')],check=True,text=True,capture_output=True)
    current=list(csv.DictReader(io.StringIO(result.stdout)))
    assert len(current)==972
    rows.extend(dict(process=process,**r) for r in current)
keys=list(rows[0])
with (root/'docs/benchmarks/task-007-raw.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=keys,lineterminator="\n");w.writeheader();w.writerows(rows)
by={(r['process'],r['trace'],r['seed'],r['policy'],r['selector'],r['sample']):r for r in rows}
for r in rows:
    oracle=by[(r['process'],r['trace'],r['seed'],r['policy'],'bounded',r['sample'])]
    if r['selector'] in ['fast','journal']:
        assert all(r[k]==oracle[k] for k in r if k not in ['selector','event_ns','inspection_ns'])
    other=by[(1-r['process'],r['trace'],r['seed'],r['policy'],r['selector'],r['sample'])]
    assert all(r[k]==other[k] for k in r if k not in ['process','event_ns','inspection_ns'])
groups={}
for r in rows:groups.setdefault((r['trace'],r['policy'],r['selector']),[]).append(r)
fields=['trace','policy','mode','median_event_ns','p95_event_ns','median_ns_per_access','refaults_mean','migrations_mean','rss_note']
with (root/'docs/benchmarks/task-007-summary.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=fields,lineterminator="\n");w.writeheader()
    for (trace,policy,mode),v in groups.items():
        times=sorted(int(r['event_ns']) for r in v)
        w.writerow(dict(trace=trace,policy=policy,mode=mode,median_event_ns=statistics.median(times),p95_event_ns=times[(95*len(times)+99)//100-1],median_ns_per_access=round(statistics.median(times)/512,2),refaults_mean=round(statistics.mean(int(r['refaults']) for r in v),2),migrations_mean=round(statistics.mean(int(r['migrations']) for r in v),2),rss_note='tidak diukur per mode'))
(root/'docs/benchmarks/task-007-reproducibility.txt').write_text('1944 record valid; dua proses identik non-timing; journal/FastPath identik dengan oracle pada setiap sample dan konfigurasi. Warm-up1 dan3 sample tiap mode, seed1/42/2026.\n')

(root/"docs/benchmarks/task-007-process-memory.txt").write_text("Peak RSS maksimum subprocess benchmark (KiB): "+str(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss)+"\nPengukuran seluruh proses, seluruh mode, bukan footprint per-mode. /usr/bin/time tidak tersedia; resource.getrusage dipakai.\n")
