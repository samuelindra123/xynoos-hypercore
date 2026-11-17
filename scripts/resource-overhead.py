"""Per-child wait4 measurements; no daemon or induced pressure."""
import json, os, platform, statistics, subprocess, tempfile, time
results = {}
for command in (['resources'], ['memory', '--interval-ms', '100']):
    rows = []
    for _ in range(20):
        with tempfile.TemporaryFile() as out:
            start = time.perf_counter()
            child = subprocess.Popen(['target/release/xynoos', *command], stdout=out)
            _, status, usage = os.wait4(child.pid, 0)
            child.returncode = os.waitstatus_to_exitcode(status)
            wall = time.perf_counter() - start
            if child.returncode: raise RuntimeError(child.returncode)
            out.seek(0)
            value = json.load(out)
            rows.append({'wall_ms': wall * 1000, 'cpu_ms': (usage.ru_utime + usage.ru_stime) * 1000,
                         'peak_rss_kib': usage.ru_maxrss,
                         'hardware_collection_ms': value.get('hardware', {}).get('duration_ns', 0) / 1e6})
    results[' '.join(command)] = {'samples': rows, 'summary': {
        k: {'min': min(r[k] for r in rows), 'median': statistics.median(r[k] for r in rows),
            'max': max(r[k] for r in rows), 'stdev': statistics.stdev(r[k] for r in rows)} for k in rows[0]}}
print(json.dumps({'environment': platform.platform(), 'count': 20,
    'limitations': ['Process peak includes startup; no idle daemon.', 'No induced pressure; not N5000 evidence.'],
    'results': results}, indent=2))
