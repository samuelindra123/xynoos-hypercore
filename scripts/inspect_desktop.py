#!/usr/bin/env python3
"""Inspect partial desktop evidence without signalling, replay or cleanup."""
import argparse
import json
from pathlib import Path
from benchmark_desktop import ARMS, summarize


def inspect(path):
    manifest = None
    starts, runs = {}, {}
    end = None
    truncated = False
    terminal = False
    with path.open('rb') as stream:
        while True:
            line = stream.readline(8 * 1024 * 1024 + 1)
            if not line:
                break
            if len(line) > 8 * 1024 * 1024:
                raise ValueError('evidence line bound')
            try:
                row = json.loads(line)
            except (ValueError, UnicodeError):
                if line.endswith(b'\n') or stream.read(1):
                    raise ValueError('corrupt complete/nonfinal row') from None
                truncated = True
                break
            if terminal:
                raise ValueError('evidence after end')
            kind = row['type']
            if kind == 'manifest':
                if manifest is not None or starts:
                    raise ValueError('duplicate/late manifest')
                manifest = row
            elif kind == 'run_start':
                key = row['repeat'], row['arm']
                if manifest is None or key in starts:
                    raise ValueError('missing manifest or duplicate run start')
                starts[key] = row
            elif kind == 'run':
                key = row['repeat'], row['arm']
                if key not in starts or key in runs:
                    raise ValueError('unstarted/duplicate run')
                runs[key] = row
            elif kind == 'end':
                terminal = True
                end = row['status']
    unresolved = sorted(set(starts) - set(runs))
    config = manifest.get('configuration', {}) if manifest else {}
    arms = (config['only_arm'],) if config.get('only_arm') else (ARMS if config.get('arms', 'all') == 'all' else ARMS[:2])
    expected = {(i, arm) for i in range(config.get('repeats', 0)) for arm in arms}
    expected |= {(-1, arm) for arm in arms}
    complete = bool(manifest and end == 'complete' and not truncated and not unresolved and set(runs) == expected)
    for row in runs.values():
        children = {c['name']: c for c in row['children']}
        wanted = {'search', 'index'} | ({'observer'} if row['arm'] != 'baseline' else set())
        complete &= (row['status'] == 'complete' and set(children) == wanted and all(
            c['reaped'] and not c.get('kill_escalation') and not c.get('cleanup_error') for c in children.values()))
        for role in ('search', 'index'):
            child = children.get(role, {})
            final = child.get('final') or {}
            complete &= (child.get('exit_code') == 0 and final.get('released') is True and final.get('cancelled') is False
                         and final.get('searches') == (config.get('requests') if role == 'search' else 0)
                         and final.get('passes') == (config.get('index_passes') if role == 'index' else 0))
        complete &= (len(row.get('search_results', [])) == config.get('requests') and
                     len(row.get('index_results', [])) == config.get('index_passes'))
    good = [r for r in runs.values() if r['status'] == 'complete']
    if good:
        complete &= len({json.dumps((r['search_results'], r['index_results']), sort_keys=True) for r in good}) == 1
    return {'status': 'complete' if complete else 'partial', 'read_only': True,
            'environment': manifest.get('environment') if manifest else None,
            'truncated_final_row': truncated, 'unresolved_cleanup_unknown': unresolved,
            'recorded_runs': len(runs), **summarize([r for r in runs.values() if r['repeat'] >= 0])}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('events', type=Path)
    print(json.dumps(inspect(parser.parse_args().events), indent=2))
