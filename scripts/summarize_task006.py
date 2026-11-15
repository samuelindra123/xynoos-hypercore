"""Ringkasan hasil final TASK 006; metrik waktu tidak dipakai untuk equivalence."""
import csv
from pathlib import Path
from collections import defaultdict
root=Path(__file__).resolve().parent.parent/'docs'/'benchmarks'
rows=list(csv.DictReader((root/'task-006-final-raw.csv').open()))
assert len(rows)==567
by={(r['trace'],r['seed'],r['policy'],r['selector']):r for r in rows}
assert len(by)==567
for r in rows:
    assert int(r['physical_bytes'])==int(r['metadata'])+int(r['resident_raw'])+int(r['resident_compressed'])
    assert int(r['peak_total'])>=int(r['physical_bytes'])
    if r['selector']=='journal':
        other=by[(r['trace'],r['seed'],r['policy'],'bounded')]
        assert all(r[k]==other[k] for k in r if k not in ['selector','event_ns'])
groups=defaultdict(list)
for r in rows: groups[(r['trace'],r['policy'],r['selector'])].append(r)
keys=[k for k in rows[0] if k not in ['trace','seed','policy','selector']]
with (root/'task-006-final-summary.csv').open('w',newline='') as f:
    writer=csv.DictWriter(f,fieldnames=['trace','policy','selector']+keys);writer.writeheader()
    for (trace,policy,selector),v in groups.items():writer.writerow(dict(trace=trace,policy=policy,selector=selector,**{k:round(sum(int(x[k]) for x in v)/3,2) for k in keys}))
