#!/usr/bin/env python3
"""Read owned-experiment evidence without signalling historical PIDs or replaying actions."""
import argparse
import json
from pathlib import Path
from benchmark_owned import ARMS, summary


def inspect(path):
    manifest = terminal = None
    runs, starts = {}, set()
    truncated = False
    with path.open('rb') as f:
        while True:
            line = f.readline(8 * 1024 * 1024 + 1)
            if not line:
                break
            if len(line) > 8 * 1024 * 1024:
                raise ValueError('evidence line exceeds 8 MiB')
            try:
                row = json.loads(line)
            except (ValueError, UnicodeError):
                if line.endswith(b'\n') or f.read(1):
                    raise ValueError('corrupt complete/non-final row') from None
                truncated = True
                break
            if terminal is not None:
                raise ValueError('records after terminal marker')
            kind = row['type']
            if kind == 'manifest':
                if manifest is not None or starts or runs:
                    raise ValueError('duplicate or misplaced manifest')
                manifest = row
            elif kind in ('run_start', 'run'):
                key = row['repeat'], row['arm']
                if key[1] not in ARMS:
                    raise ValueError('unknown arm')
                if kind == 'run_start':
                    if key in starts:
                        raise ValueError('duplicate run start')
                    starts.add(key)
                else:
                    if key in runs or key not in starts:
                        raise ValueError('duplicate or unstarted run')
                    runs[key] = row
            elif kind == 'end':
                terminal = row['status']
    expected = {(i, arm) for i in range(-1, manifest['configuration']['repeats']) for arm in ARMS} if manifest else set()
    measured = [row for (i, _), row in runs.items() if i >= 0]
    valid = bool(manifest) and starts == expected and set(runs) == expected
    for run in runs.values():
        valid &= (run['status'] == 'complete' and run.get('error') is None
            and bool(run.get('children')) and all(c['reaped'] and not c.get('kill_escalation') and c.get('exit_code') == 0 for c in run['children'])
            and bool(run.get('final')) and run['final']['restored'] and run['final']['verified'] and not run['final']['cancelled']
            and len(run['responses']) == manifest['configuration']['requests'])
    report = summary(measured)
    valid &= all(p['correctness_equivalent'] for p in report['paired'])
    return {'status': 'complete' if valid and terminal == 'complete' and not truncated else 'partial',
            'read_only': True, 'environment': manifest.get('environment') if manifest else None,
            'truncated_final_row': truncated,
            'unresolved_cleanup_unknown': sorted(starts-set(runs)),
            'expected_runs_including_warmup': len(expected), 'recorded_runs_including_warmup': len(runs), **report}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('events', type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.events), indent=2))
