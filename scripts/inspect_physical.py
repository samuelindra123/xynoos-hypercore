#!/usr/bin/env python3
"""Read partial physical benchmark JSONL without replaying, signalling or deleting anything."""
import argparse
import json
from pathlib import Path
from benchmark_physical import summarize


def inspect(path):
    runs = []
    started = []
    ended = None
    manifest = None
    truncated = False
    with path.open('rb') as f:
        while True:
            line = f.readline(8 * 1024 * 1024 + 1)
            if not line:
                break
            if len(line) > 8 * 1024 * 1024:
                raise ValueError('evidence line exceeds 8 MiB')
            try:
                event = json.loads(line)
            except (ValueError, UnicodeError):
                if f.read(1) or line.endswith(b'\n'):
                    raise ValueError('corrupt complete/non-final event') from None
                truncated = True
                break
            if event['type'] == 'manifest':
                if manifest is not None:
                    raise ValueError('duplicate manifest')
                manifest = event
            elif event['type'] == 'run_start':
                started.append((event['repeat'], event['arm']))
            elif event['type'] == 'run':
                if any((r['repeat'], r['arm']) == (event['repeat'], event['arm']) for r in runs):
                    raise ValueError('duplicate completed run evidence')
                runs.append(event)
            elif event['type'] == 'end':
                ended = event['status']
    finished = {(r['repeat'], r['arm']) for r in runs}
    unresolved = [key for key in started if key not in finished]
    cleanup = all(r['children'] and all(c['reaped'] and not c.get('kill_escalation', False) for c in r['children']) for r in runs)
    expected = manifest['configuration']['repeats'] * 2 if manifest else None
    measured = [r for r in runs if r['repeat'] >= 0]
    expected_keys = {(i, arm) for i in range(manifest['configuration']['repeats']) for arm in ('baseline', 'observe')} if manifest else set()
    complete = (ended == 'complete' and not truncated and not unresolved and cleanup
                and len(measured) == expected and all(r['status'] == 'complete' for r in runs)
                and {(r['repeat'], r['arm']) for r in measured} == expected_keys)
    return {'status': 'complete' if complete else 'partial', 'read_only': True,
            'environment': manifest.get('environment') if manifest else None,
            'truncated_final_event': truncated, 'unresolved_runs_cleanup_unknown': unresolved,
            'expected_measured_runs': expected, 'recorded_measured_runs': len(measured),
            'cleanup': 'unknown_for_unresolved_runs' if unresolved else 'see_recorded_child_results',
            **summarize(measured)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('events', type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.events), indent=2))
