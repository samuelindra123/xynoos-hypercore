"""Validasi dan ringkasan eksperimen shared RAM TASK 005."""
import csv
from pathlib import Path
from collections import defaultdict
root=Path(__file__).resolve().parent.parent/'docs'/'benchmarks'
rows=list(csv.DictReader((root/'task-005-raw.csv').open()))
assert len(rows)==270
assert len({(r['trace'],r['seed'],r['policy']) for r in rows})==270
for r in rows:
 assert int(r['physical_bytes'])<=66304
 assert int(r['peak_physical_bytes'])<=66304
 assert int(r['resident_compressed'])+768<=int(r['physical_bytes'])
 assert int(r['capacity_violations'])==0
keys=[k for k in rows[0] if k not in ['trace','seed','policy','profile_bytes']]
groups=defaultdict(list)
for r in rows:groups[(r['trace'],r['policy'])].append(r)
with (root/'task-005-summary.csv').open('w',newline='') as f:
 w=csv.DictWriter(f,fieldnames=['trace','policy']+keys);w.writeheader()
 for (t,p),rs in groups.items():w.writerow(dict(trace=t,policy=p,**{k:round(sum(int(r[k]) for r in rs)/3,2) for k in keys}))
