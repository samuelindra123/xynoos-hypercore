#!/usr/bin/env python3
"""Ringkas sampel batch TASK 003 yang dihasilkan xynoos-bench."""
import csv
import statistics
from collections import defaultdict
from pathlib import Path

root = Path(__file__).resolve().parent.parent / "docs" / "benchmarks"
groups = defaultdict(list)
with (root / "task-003-raw.csv").open(newline="") as source:
    for row in csv.DictReader(source):
        groups[(row["dataset"], row["backend"])].append(row)
fields = ["dataset", "backend", "insert_median_ns", "insert_p95_batch_ns", "get_median_ns", "get_p95_batch_ns", "compress_mib_s", "decompress_mib_s", "ratio", "payload_bytes", "capacity_bytes", "metadata_bytes", "raw_pages", "compressed_pages", "rss_before_kib", "rss_loaded_kib", "peak_kib", "cpu_ms_total"]
with (root / "task-003-summary.csv").open("w", newline="") as output:
    writer = csv.DictWriter(output, fieldnames=fields)
    writer.writeheader()
    for (dataset, backend), rows in groups.items():
        def median(key):
            return round(statistics.median(float(row[key]) for row in rows), 2)
        def p95(key):
            values = sorted(float(row[key]) for row in rows)
            return values[(95 * len(values) + 99) // 100 - 1]
        last = rows[-1]
        writer.writerow(dict(dataset=dataset, backend=backend, insert_median_ns=median("insert_batch_ns_per_page"), insert_p95_batch_ns=p95("insert_batch_ns_per_page"), get_median_ns=median("get_batch_ns_per_page"), get_p95_batch_ns=p95("get_batch_ns_per_page"), compress_mib_s=median("codec_compress_mib_s"), decompress_mib_s=median("codec_decompress_mib_s"), ratio=round(int(last["payload_bytes"]) / 1048576, 6), payload_bytes=last["payload_bytes"], capacity_bytes=last["capacity_bytes"], metadata_bytes=last["metadata_bytes"], raw_pages=last["raw_pages"], compressed_pages=last["compressed_pages"], rss_before_kib=last["rss_before_kib"], rss_loaded_kib=last["rss_loaded_kib"], peak_kib=last["peak_kib"], cpu_ms_total=round(int(last["cpu_ns"]) / 1e6, 2)))
