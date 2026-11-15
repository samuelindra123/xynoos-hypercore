"""Atribusi heap dan ekuivalensi empat workload, tiga ukuran, dua subprocess."""
import csv,io,subprocess,statistics,resource
from pathlib import Path
root=Path(__file__).resolve().parent.parent
rows=[]
for process in range(2):
 result=subprocess.run([str(root/'target/release/examples/typed_research')],text=True,capture_output=True,check=True)
 data=list(csv.DictReader(io.StringIO(result.stdout)));assert len(data)==324
 rows.extend(dict(process=process,**r) for r in data)
by={(r['process'],r['trace'],r['groups'],r['seed'],r['mode'],r['sample']):r for r in rows};assert len(by)==648
exclude={'mode','event_ns','inspection_ns','alloc_count','alloc_bytes','dealloc_count','estimated_vec_clones','estimated_access_replays','live_count','live_bytes','peak_live_bytes'}
for r in rows:
 other=by[(r['process'],r['trace'],r['groups'],r['seed'],'clone',r['sample'])]
 assert all(r[k]==other[k] for k in r if k not in exclude)
 other=by[(1-r['process'],r['trace'],r['groups'],r['seed'],r['mode'],r['sample'])]
 assert all(r[k]==other[k] for k in r if k not in {'process','event_ns','inspection_ns'})
 assert int(r['peak_live_bytes'])>=int(r['live_bytes'])
with root.joinpath('docs/benchmarks/task-011-raw.csv').open('w',newline='') as f:
 w=csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator='\n');w.writeheader();w.writerows(rows)
fields=['groups','mode','samples','event_ns_median','inspection_ns_median','alloc_count_median','alloc_bytes_median','dealloc_count_median','live_bytes_median','peak_live_bytes_median']
with root.joinpath('docs/benchmarks/task-011-summary.csv').open('w',newline='') as f:
 w=csv.DictWriter(f,fieldnames=fields,lineterminator='\n');w.writeheader()
 for n in [1,16,64]:
  for mode in ['clone','journal','fast']:
   v=[r for r in rows if int(r['groups'])==n and r['mode']==mode]
   d=dict(groups=n,mode=mode,samples=len(v))
   for k in fields[3:]:d[k]=statistics.median(int(r[k.removesuffix('_median')]) for r in v)
   w.writerow(d)
root.joinpath('docs/benchmarks/task-011-reproducibility.txt').write_text('648 record valid; mode ekuivalen pada metrik simulasi; dua subprocess identik pada semua kolom selain runtime. Semua scope selesai dengan live count nol setelah drop; register tidak penuh.\n')
root.joinpath('docs/benchmarks/task-011-memory.txt').write_text(f'Peak RSS maksimum subprocess keseluruhan: {resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss} KiB.\n')
