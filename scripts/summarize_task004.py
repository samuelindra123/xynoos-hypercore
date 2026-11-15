#!/usr/bin/env python3
"""Memvalidasi CSV dan merangkum metrik simulasi; waktu nyata dipisahkan."""
import csv
from collections import defaultdict
from pathlib import Path
root = Path(__file__).resolve().parent.parent / 'docs' / 'benchmarks'
rows = list(csv.DictReader((root / 'task-004-raw.csv').open()))
assert len(rows) == 81
assert len({(r['trace'], r['seed'], r['policy']) for r in rows}) == 81
keys = [k for k in rows[0] if k not in ['trace', 'seed', 'policy', 'profile_bytes']]
groups = defaultdict(list)
for row in rows:
    assert int(row['migrations']) <= 512
    assert int(row['peak_ram_bytes']) <= 65536
    assert int(row['peak_compressed_bytes']) <= 16384
    assert int(row['peak_backing_bytes']) <= 131072
    groups[(row['trace'], row['policy'])].append(row)
with (root / 'task-004-summary.csv').open('w', newline='') as out:
    writer = csv.DictWriter(out, fieldnames=['trace', 'policy'] + keys)
    writer.writeheader()
    for (trace, policy), samples in groups.items():
        writer.writerow({'trace': trace, 'policy': policy, **{k: round(sum(int(r[k]) for r in samples) / len(samples), 2) for k in keys}})
