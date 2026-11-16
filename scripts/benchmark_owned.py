#!/usr/bin/env python3
"""Three-arm owned lossless corpus experiment. No kernel control; stdlib only."""
import argparse
import datetime
import hashlib
import itertools
import json
import math
import os
from pathlib import Path
import platform
import selectors
import signal
import subprocess
import sys
import time
import benchmark_physical as base

ARMS = ('baseline', 'observe', 'active')
ORDERS = tuple(itertools.permutations(ARMS))
STOP = None


def provenance():
    directory = Path(__file__).resolve().parent
    packaged = base.read(directory/'SOURCE_MANIFEST.json')
    if packaged:
        return json.loads(packaged)
    root = directory.parent
    try:
        commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True, timeout=2).strip()
    except (OSError, subprocess.SubprocessError):
        commit = None
    paths = [root/'Cargo.toml', root/'Cargo.lock', *sorted((root/'crates').rglob('*.rs')),
             *sorted((root/'crates').glob('*/Cargo.toml')),
             directory/'benchmark_owned.py', directory/'benchmark_physical.py', directory/'inspect_owned.py']
    return {'base_commit': commit, 'source_sha256': {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
                                                  for p in paths if p.is_file()}}


def interrupt(signum, _frame):
    global STOP
    STOP = signal.Signals(signum).name


def run_arm(arm, args, emit, repeat):
    started = time.monotonic_ns()
    cpu = time.process_time_ns()
    deadline = time.monotonic() + args.timeout
    responses, samples, children = [], [], []
    result = {'type': 'run', 'arm': arm, 'repeat': repeat, 'status': 'incomplete'}
    ready = final = None
    pending = None
    next_request = 0
    next_sample = 0
    finishing = False
    finish_deadline = None
    work_start = work_end = None
    error = None
    emit({'type': 'run_start', 'arm': arm, 'repeat': repeat})
    with selectors.DefaultSelector() as selector:
        try:
            base.safe(base.host())
            command = [str(args.worker), arm, args.corpus]
            if arm == 'active':
                if not args.authorize_owned:
                    raise RuntimeError('active requires --authorize-owned')
                command.append('--authorize-owned')
            child = base.Child(command, selector, 'workload')
            children.append(child)
            while True:
                now = time.monotonic()
                if now >= deadline:
                    raise RuntimeError('arm timeout')
                if STOP and not finishing:
                    child.proc.stdin.write(b'cancel\n')
                    child.proc.stdin.flush()
                    finishing = True
                    finish_deadline = now + 1
                if finish_deadline and now >= finish_deadline:
                    raise RuntimeError('owned final/restore timeout')
                if now >= next_sample:
                    h = base.host()
                    samples.append({'host': h, 'processes': {'workload': base.proc_sample(child.proc.pid),
                                    'harness': base.proc_sample(os.getpid())}})
                    emit({'type': 'sample', 'arm': arm, 'repeat': repeat, **samples[-1]})
                    base.safe(h)
                    next_sample = now + .1
                if ready and not finishing and pending is None and now >= next_request:
                    pending = time.monotonic_ns()
                    if work_start is None:
                        work_start = pending
                    child.proc.stdin.write(b'work\n')
                    child.proc.stdin.flush()
                for key, _ in selector.select(.005):
                    _, channel = key.data
                    data = os.read(key.fileobj.fileno(), 65536)
                    if not data:
                        selector.unregister(key.fileobj)
                        continue
                    child.bytes += len(data)
                    if child.bytes > 2 * 1024 * 1024:
                        raise RuntimeError('child output exceeds 2 MiB')
                    child.buffers[channel] += data
                    if len(child.buffers[channel]) > 65536:
                        raise RuntimeError('child line exceeds 64 KiB')
                    if channel == 'stderr':
                        continue
                    while b'\n' in child.buffers[channel]:
                        line, child.buffers[channel] = child.buffers[channel].split(b'\n', 1)
                        row = json.loads(line)
                        if row['type'] == 'ready' and ready is None:
                            if (row.get('arm') != arm or row.get('corpus') != args.corpus
                                or row.get('workload') != 'owned_lossless_16mib_v1'
                                or row.get('retained_bytes') != 17 * 1024 * 1024
                                or not 0 < row['limits']['address_space_bytes'] <= 128 * 1024 * 1024
                                or not 0 < row['limits']['cpu_seconds'] <= 8
                                or row['limits']['wall_seconds'] != 25
                                or row.get('kernel_mutations') != 0):
                                raise RuntimeError('worker readiness/limits mismatch')
                            ready = row
                            emit({'type': 'ready', 'arm': arm, 'repeat': repeat, 'report': row})
                        elif row['type'] == 'response' and pending is not None:
                            if row.get('verified') is not True or row.get('kernel_mutations') != 0:
                                raise RuntimeError('worker integrity/safety failure')
                            row.update(response_ns=time.monotonic_ns()-pending,
                                       process=base.proc_sample(child.proc.pid))
                            responses.append(row)
                            emit({'type': 'response', 'arm': arm, 'repeat': repeat,
                                  'request': len(responses), **row})
                            pending = None
                            next_request = time.monotonic() + args.interval_ms / 1000
                            if len(responses) == args.requests and not finishing:
                                work_end = time.monotonic_ns()
                                child.proc.stdin.write(b'finish\n')
                                child.proc.stdin.flush()
                                finishing = True
                                finish_deadline = time.monotonic() + 1
                        elif row['type'] == 'final' and finishing and final is None:
                            if row.get('restored') is not True or row.get('verified') is not True:
                                raise RuntimeError('restoration failure')
                            if row['retained_after_restore'] != 17 * 1024 * 1024:
                                raise RuntimeError('restored allocation mismatch')
                            final = row
                            # Capture restoration HWM before the worker may disappear.
                            final['process'] = base.proc_sample(child.proc.pid)
                            emit({'type': 'restoration', 'arm': arm, 'repeat': repeat, 'report': row})
                        else:
                            raise RuntimeError('unexpected protocol row')
                code = child.reap()
                if code is not None:
                    # Drain pipe EOF before deciding that a final report is missing.
                    if selector.get_map():
                        continue
                    if code != 0 or final is None or STOP or len(responses) != args.requests or final['cancelled']:
                        raise RuntimeError('cancelled: '+STOP if STOP else 'worker exit/protocol failure')
                    result['status'] = 'complete'
                    break
        except (Exception, KeyboardInterrupt) as exc:
            error = f'{type(exc).__name__}: {exc}'
        finally:
            cleanup = []
            for child in reversed(children):
                try:
                    cleanup.append(child.stop())
                except Exception as exc:
                    cleanup.append({'reaped': False, 'error': str(exc)})
    if not all(c['reaped'] and not c.get('kill_escalation') and c.get('exit_code') == 0 for c in cleanup):
        result['status'] = 'incomplete'
    result.update(error=error, responses=responses, samples=samples, children=cleanup, ready=ready, final=final,
                  completion_ns=work_end-work_start if work_end else None,
                  arm_wall_ns=time.monotonic_ns()-started, harness_cpu_ns=time.process_time_ns()-cpu)
    emit(result)
    return result


def peak_hwm(run):
    values = ([s['processes']['workload']['hwm_kib'] for s in run['samples']]
              + [row['process']['hwm_kib'] for row in run['responses']]
              + [run['final']['self_memory_after_restore']['hwm_kib']])
    return max((v for v in values if v is not None), default=None)


def summary(runs):
    result = {}
    for arm in ARMS:
        group = [r for r in runs if r['arm'] == arm and r['status'] == 'complete']
        rows = [row for r in group for row in r['responses']]
        steady = [row for r in group for row in r['responses'][8:]]
        effects = {}
        for row in rows:
            if row['policy']:
                name = row['policy']['effect']
                effects[name] = effects.get(name, 0) + 1
        cpu = [c['usage']['user_cpu_ns']+c['usage']['system_cpu_ns'] for r in group for c in r['children'] if c.get('usage')]
        peaks = [peak_hwm(r) for r in group]
        result[arm] = {'complete_runs': len(group),
            **{key: base.quantiles([row[key] for row in rows]) for key in ('response_ns', 'service_ns', 'elapsed_ns')},
            'completion_ns': base.quantiles([r['completion_ns'] for r in group]),
            'arm_wall_ns': base.quantiles([r['arm_wall_ns'] for r in group]),
            'child_cpu_ns': base.quantiles(cpu),
            'harness_cpu_ns': base.quantiles([r['harness_cpu_ns'] for r in group]),
            'steady_payload_bytes': base.quantiles([row['retained_bytes'] for row in steady]),
            'steady_rss_kib': base.quantiles([row['process']['rss_kib'] for row in steady if row['process']['rss_kib'] is not None]),
            'sampled_peak_hwm_kib': base.quantiles([v for v in peaks if v is not None]),
            'harness_rss_kib': base.quantiles([s['processes']['harness']['rss_kib'] for r in group for s in r['samples']
                                            if s['processes']['harness']['rss_kib'] is not None]),
            'policy_effects': effects,
            'actions': sum(row['policy']['changed_blocks'] > 0 for row in rows if row['policy']),
            'blocks_replaced': sum(row['policy']['changed_blocks'] for row in rows if row['policy']),
            'action_ns': base.quantiles([row['policy']['action_ns'] for row in rows if row['policy'] and row['policy']['action_ns']]),
            'restoration_ns': base.quantiles([r['final']['restore_ns'] for r in group]),
            'context_per_run': [base.context_delta(r['samples']) for r in group]}
    paired = []
    for i in sorted({r['repeat'] for r in runs}):
        triple = {r['arm']: r for r in runs if r['repeat'] == i and r['status'] == 'complete'}
        if len(triple) == 3:
            reference = triple['baseline']
            equivalent = all(r['ready']['identity'] == reference['ready']['identity'] and
                [x['checksum'] for x in r['responses']] == [x['checksum'] for x in reference['responses']] for r in triple.values())
            paired.append({'repeat': i, 'correctness_equivalent': equivalent,
                **{arm+'_completion_ratio': triple[arm]['completion_ns']/reference['completion_ns'] for arm in ('observe', 'active')},
                **{arm+'_wall_ratio': triple[arm]['arm_wall_ns']/reference['arm_wall_ns'] for arm in ('observe', 'active')}})
    return {'arms': result, 'paired': paired, 'failed_runs': sum(r['status'] != 'complete' for r in runs),
            'refused_policy_samples': sum(row['policy']['effect'].startswith('refused') for r in runs for row in r['responses'] if row['policy']),
            'interpretation': 'Private cooperative corpus only. Steady memory excludes first 8 requests; CPU/wall include generation, policy, verification, restoration. Host counters are shared context. No desktop benefit claim.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, default=Path(__file__).resolve().parents[1]/'target/release/xynoos-owned')
    parser.add_argument('--environment', required=True, choices=('Codespaces', 'physical'))
    parser.add_argument('--label', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--authorize-owned', action='store_true')
    parser.add_argument('--corpus', choices=('mixed', 'entropy'), default='mixed')
    parser.add_argument('--repeats', type=int, default=6)
    parser.add_argument('--requests', type=int, default=128)
    parser.add_argument('--interval-ms', type=int, default=10)
    parser.add_argument('--timeout', type=float, default=24)
    args = parser.parse_args()
    if not args.authorize_owned:
        parser.error('explicit --authorize-owned required for the active arm')
    if not (args.repeats in (6, 12, 18, 24) and 16 <= args.requests <= 256 and 0 <= args.interval_ms <= 20
            and math.isfinite(args.timeout) and 1 <= args.timeout <= 24):
        parser.error('repeats 6/12/18/24, requests 16..256, interval 0..20, timeout 1..24')
    if os.environ.get('CODESPACES') == 'true' and args.environment != 'Codespaces':
        parser.error('Codespaces must be labelled Codespaces')
    args.worker = args.worker.resolve()
    if not args.worker.is_file() or not os.access(args.worker, os.X_OK):
        parser.error('missing worker executable')
    args.output.mkdir(parents=True, exist_ok=False)
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, interrupt)
    runs = []
    status = 'incomplete'
    with (args.output/'events.jsonl').open('x', buffering=1) as log:
        def emit(row):
            log.write(json.dumps(row, separators=(',', ':'), allow_nan=False)+'\n')
            if row['type'] in ('manifest', 'run_start', 'run', 'end', 'failure'):
                log.flush()
                os.fsync(log.fileno())
        emit({'type': 'manifest', 'schema_version': 1, 'experiment': 'owned_lossless_16mib_v1',
            'environment': args.environment, 'label': args.label,
            'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'platform': platform.platform(),
            'cpuinfo': base.read('/proc/cpuinfo', 1048576), 'os_release': base.read('/etc/os-release'),
            'source_commit': base.read(Path(__file__).parent/'SOURCE_COMMIT'),
            'source_manifest': provenance(),
            'sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in
                       (args.worker, Path(__file__), Path(base.__file__))},
            'configuration': {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
            'order': 'one excluded ABC warmup; all six permutations in each measured block of six',
            'initial_host': base.host(), 'physical_mutation_available': False})
        try:
            for repeat in range(-1, args.repeats):
                order = ARMS if repeat == -1 else ORDERS[repeat % 6]
                for arm in order:
                    run = run_arm(arm, args, emit, repeat)
                    if repeat >= 0:
                        runs.append(run)
                    if run['status'] != 'complete':
                        raise RuntimeError(run['error'] or 'cleanup incomplete')
                print(f'triple {repeat} complete', flush=True)
            if not all(p['correctness_equivalent'] for p in summary(runs)['paired']):
                raise RuntimeError('cross-arm correctness mismatch')
            status = 'complete'
        except (Exception, KeyboardInterrupt) as exc:
            emit({'type': 'failure', 'error': str(exc)})
        report = {'status': status, 'environment': args.environment, 'expected_runs': args.repeats*3,
                  'recorded_runs': len(runs), **summary(runs)}
        emit({'type': 'end', 'status': status})
        with (args.output/'summary.json').open('x') as f:
            json.dump(report, f, indent=2, allow_nan=False)
            f.write('\n')
            f.flush()
            os.fsync(f.fileno())
    print(json.dumps(report, indent=2))
    return 0 if status == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
