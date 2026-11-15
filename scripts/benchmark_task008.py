"""Benchmark Rust allocator dan verifikasi metrik simulator antar mode."""
import csv,io,statistics,subprocess,resource
from pathlib import Path
root=Path(__file__).resolve().parent.parent
rows=[]
for process in range(2):
    result=subprocess.run([str(root/'target/release/examples/scalability')],text=True,capture_output=True,check=True)
    data=list(csv.DictReader(io.StringIO(result.stdout)))
    assert len(data)==162
    rows.extend(dict(process=process,**row) for row in data)
by={(r['process'],r['groups'],r['seed'],r['mode'],r['sample']):r for r in rows}
assert len(by)==324
excluded={'mode','event_ns','inspection_ns','alloc_count','alloc_bytes','dealloc_count','estimated_vec_clones','estimated_access_replays'}
for r in rows:
    other=by[(r['process'],r['groups'],r['seed'],'clone',r['sample'])]
    assert all(r[k]==other[k] for k in r if k not in excluded)
    repeat=by[(1-r['process'],r['groups'],r['seed'],r['mode'],r['sample'])]
    assert all(r[k]==repeat[k] for k in r if k not in excluded|{'process'})
root.joinpath('docs/benchmarks/task-008-reproducibility.txt').write_text('324 record: metrik non-timing/non-allocator identik antar mode dan dua subprocess; alokasi dicatat sebagai pengukuran, bukan diuji determinismenya.\n')
with root.joinpath('docs/benchmarks/task-008-raw.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator='\n');w.writeheader();w.writerows(rows)
fields=['groups','mode','samples','event_ns_median','event_ns_p95','inspection_ns_median','alloc_count_median','alloc_bytes_median','dealloc_count_median','peak_live_bytes','rss_scope']
with root.joinpath('docs/benchmarks/task-008-summary.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=fields,lineterminator='\n');w.writeheader()
    for n in [1,4,8,16,32,64]:
        for mode in ['clone','journal','fast']:
            group=[r for r in rows if int(r['groups'])==n and r['mode']==mode]
            def med(k):return statistics.median(int(r[k]) for r in group)
            times=sorted(int(r['event_ns']) for r in group)
            w.writerow(dict(groups=n,mode=mode,samples=len(group),event_ns_median=med('event_ns'),event_ns_p95=times[(95*len(times)+99)//100-1],inspection_ns_median=med('inspection_ns'),alloc_count_median=med('alloc_count'),alloc_bytes_median=med('alloc_bytes'),dealloc_count_median=med('dealloc_count'),peak_live_bytes='tidak_diukur',rss_scope='subprocess_keseluruhan'))
root.joinpath('docs/benchmarks/task-008-memory.txt').write_text(f'Peak RSS maksimum seluruh subprocess: {resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss} KiB; termasuk inisialisasi dan seluruh ukuran/mode.\n')
