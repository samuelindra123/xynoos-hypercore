#!/usr/bin/env python3
"""Bounded Linux workload A/B. Stdlib only. No root, control writes or simulation arms."""
import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import selectors
import signal
import statistics
import subprocess
import sys
import time

ARMS = ('baseline', 'observe')
STOP = None


def interrupt(signum, _frame):
    global STOP
    STOP = signal.Signals(signum).name


def read(path, limit=65536):
    try:
        with open(path, 'rb') as f:
            data = f.read(limit + 1)
        return data.decode() if len(data) <= limit else None
    except (OSError, UnicodeError):
        return None


def numbers(text):
    result = {}
    for line in (text or '').splitlines():
        words = line.replace(':', '').split()
        if len(words) >= 2 and words[1].isdigit():
            result[words[0]] = int(words[1])
    return result


def psi(text):
    try:
        return {line.split()[0]: {k: float(v) if k != 'total' else int(v)
                for k, v in (field.split('=') for field in line.split()[1:])}
                for line in text.splitlines()} or None
    except (AttributeError, ValueError, IndexError):
        return None


def host():
    mem = numbers(read('/proc/meminfo'))
    vm = numbers(read('/proc/vmstat'))
    cpu = (read('/proc/stat') or '').splitlines()
    # Device discovery and every read are bounded; absence stays null.
    devices = []
    try:
        for p in sorted(Path('/sys/block').glob('zram*'))[:16]:
            devices.append({'device': p.name, 'mm_stat': read(p / 'mm_stat'),
                            'io_stat': read(p / 'io_stat'), 'stat': read(p / 'stat')})
    except OSError:
        devices = None
    return {'monotonic_ns': time.monotonic_ns(), 'mem_available_kib': mem.get('MemAvailable'),
            'swap_free_kib': mem.get('SwapFree'), 'swap_total_kib': mem.get('SwapTotal'),
            'psi': {k: psi(read('/proc/pressure/' + k)) for k in ('cpu', 'memory', 'io')},
            'swap_pages': {k: vm.get(k) for k in ('pswpin', 'pswpout')},
            'cpu_ticks': [int(x) for x in cpu[0].split()[1:9]] if cpu and cpu[0].startswith('cpu ') else None,
            'loadavg': read('/proc/loadavg'), 'zram': devices,
            'mounted_cgroup_root_context': {k: read('/sys/fs/cgroup/' + k) for k in
                ('memory.current', 'memory.max', 'memory.events', 'memory.pressure')},
            'scope': 'visible_namespace; cgroup fields describe mounted root, not assumed workload scope'}


def safe(snapshot):
    available = snapshot.get('mem_available_kib')
    if available is None:
        raise RuntimeError('MemAvailable unavailable; workload admission refused')
    if available < 256 * 1024:
        raise RuntimeError('MemAvailable below 256 MiB; stop')
    cg = snapshot.get('mounted_cgroup_root_context', {})
    current, maximum = cg.get('memory.current'), cg.get('memory.max')
    if current and maximum and current.strip().isdigit() and maximum.strip().isdigit():
        if int(maximum) - int(current) < 32 * 1024 * 1024:
            raise RuntimeError('mounted cgroup root headroom below 32 MiB; stop')


def proc_sample(pid):
    status = numbers(read(f'/proc/{pid}/status'))
    stat = read(f'/proc/{pid}/stat')
    fields = stat.rsplit(')', 1)[-1].split() if stat and ')' in stat else []
    return {'rss_kib': status.get('VmRSS'), 'hwm_kib': status.get('VmHWM'),
            'user_ticks': int(fields[11]) if len(fields) > 12 else None,
            'system_ticks': int(fields[12]) if len(fields) > 12 else None}


class Child:
    def __init__(self, command, selector, name):
        self.name = name
        self.proc = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, bufsize=0)
        self.buffers = {'stdout': b'', 'stderr': b''}
        self.bytes = 0
        self.usage = None
        try:
            for channel in self.buffers:
                stream = getattr(self.proc, channel)
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, (self, channel))
        except BaseException:
            self.stop()
            raise

    def reap(self):
        if self.proc.returncode is None:
            pid, status, usage = os.wait4(self.proc.pid, os.WNOHANG)
            if pid:
                self.proc.returncode = os.waitstatus_to_exitcode(status)
                self.usage = {'user_cpu_ns': round(usage.ru_utime * 1e9),
                              'system_cpu_ns': round(usage.ru_stime * 1e9),
                              'wait4_peak_rss_kib': usage.ru_maxrss,
                              'minor_faults': usage.ru_minflt, 'major_faults': usage.ru_majflt,
                              'voluntary_switches': usage.ru_nvcsw, 'involuntary_switches': usage.ru_nivcsw}
        return self.proc.returncode

    def stop(self):
        escalation = False
        if self.proc.stdin and not self.proc.stdin.closed:
            self.proc.stdin.close()
        if self.reap() is None:
            os.kill(self.proc.pid, signal.SIGTERM)
        deadline = time.monotonic() + 1
        while self.reap() is None and time.monotonic() < deadline:
            time.sleep(.005)
        if self.reap() is None:
            escalation = True
            os.kill(self.proc.pid, signal.SIGKILL)
            deadline = time.monotonic() + 1
            while self.reap() is None and time.monotonic() < deadline:
                time.sleep(.005)
        reaped = self.proc.returncode is not None
        for channel in ('stdout', 'stderr'):
            getattr(self.proc, channel).close()
        return {'name': self.name, 'pid': self.proc.pid, 'reaped': reaped, 'exit_code': self.proc.returncode,
                'kill_escalation': escalation, 'usage': self.usage,
                'stderr_tail': self.buffers['stderr'][-4096:].decode(errors='replace')}


def quantiles(values):
    values = sorted(values)
    if not values:
        return None
    return {'count': len(values), **{f'p{p}': values[max(0, math.ceil(len(values)*p/100)-1)] for p in (50, 95, 99)}}


def run_arm(arm, args, emit, repeat):
    start_cpu = time.process_time_ns()
    started = time.monotonic_ns()
    deadline = started + int(args.timeout * 1e9)
    children = []
    response = []
    service = []
    observations = []
    samples = []
    lateness = []
    requests = 0
    pending = None
    ready = False
    error = None
    expected_crc = None
    workload_started = None
    workload_finished = None
    telemetry_ready = arm == 'baseline'
    next_request = 0
    next_sample = started
    next_tick = started + 10_000_000
    result = {'type': 'run', 'arm': arm, 'repeat': repeat, 'status': 'incomplete'}
    emit({'type': 'run_start', 'arm': arm, 'repeat': repeat, 'monotonic_ns': started})
    with selectors.DefaultSelector() as selector:
        try:
            initial = host()
            safe(initial)
            # Start the worker only after safety checks. Observation must be ready before measured work.
            worker = Child([str(args.worker), 'workload'], selector, 'workload')
            children.append(worker)
            if arm == 'observe':
                observer = Child([str(args.observer), 'observe', '--count', '60', '--interval-ms', '500'], selector, 'observer')
                children.append(observer)
            while True:
                if STOP:
                    raise RuntimeError('cancelled: ' + STOP)
                now = time.monotonic_ns()
                if now >= deadline:
                    raise RuntimeError('arm timeout')
                if now >= next_tick:
                    lateness.append(max(0, now - next_tick))
                    # Do not manufacture many observations after a long scheduling pause.
                    next_tick = now + 10_000_000
                if now >= next_sample:
                    snapshot = host()
                    sample = {'elapsed_ns': now - started, 'host': snapshot,
                              'processes': {**{c.name: proc_sample(c.proc.pid) for c in children}, 'harness': proc_sample(os.getpid())}}
                    samples.append(sample)
                    emit({'type': 'sample', 'arm': arm, 'repeat': repeat, **sample})
                    safe(snapshot)
                    next_sample = now + 100_000_000
                if ready and telemetry_ready and pending is None and requests < args.requests and now >= next_request:
                    if workload_started is None:
                        workload_started = time.monotonic_ns()
                    pending = time.monotonic_ns()
                    worker.proc.stdin.write(b'work\n')
                    worker.proc.stdin.flush()
                for key, _ in selector.select(.005):
                    child, channel = key.data
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
                        report = json.loads(line)
                        if child.name == 'observer':
                            if (report.get('evidence') != 'live_read_only' or report.get('kernel_mutations') != 0
                                or report.get('owned_buffer_mutations') != 0 or report['plan']['kernel_write_available']):
                                raise RuntimeError('observer contract/safety failure')
                            observations.append(report)
                            telemetry_ready = True
                            # A declined policy may mean optional/required decision telemetry is absent.
                            # Keep that report explicit; independent MemAvailable admission still applies.
                            emit({'type': 'observation', 'arm': arm, 'repeat': repeat, 'report': report})
                        elif not ready:
                            if report.get('ready') is not True or report.get('workload') != 'copy_crc_1mib_x16_v1' or report.get('primary_buffer_bytes') != 2097152:
                                raise RuntimeError('worker readiness mismatch')
                            ready = True
                            expected_crc = report['crc32']
                        else:
                            if pending is None or report.get('verified') is not True or report.get('crc32') != expected_crc:
                                raise RuntimeError('workload integrity/protocol failure')
                            completed = time.monotonic_ns()
                            latency = completed - pending
                            response.append(latency)
                            service.append(report['elapsed_ns'])
                            requests += 1
                            emit({'type': 'response', 'arm': arm, 'repeat': repeat, 'request': requests,
                                  'response_ns': latency, 'service_ns': report['elapsed_ns'], 'crc32': expected_crc})
                            pending = None
                            next_request = completed + int(args.interval_ms * 1e6)
                if requests == args.requests:
                    workload_finished = time.monotonic_ns()
                    if any(c.reap() is not None for c in children):
                        raise RuntimeError('child exited before owned cleanup')
                    result['status'] = 'complete'
                    break
                for child in children:
                    if child.reap() is not None:
                        raise RuntimeError(child.name + ' terminated during workload')
        except (Exception, KeyboardInterrupt) as exc:
            error = f'{type(exc).__name__}: {exc}'
        finally:
            cleanup = []
            for child in reversed(children):
                try:
                    cleanup.append(child.stop())
                except Exception as exc:
                    cleanup.append({'name': child.name, 'pid': child.proc.pid, 'reaped': False,
                                    'kill_escalation': False, 'usage': None, 'error': str(exc)})
    if not all(c['reaped'] and not c['kill_escalation'] for c in cleanup):
        result['status'] = 'incomplete'
        error = (error or '') + '; cleanup ambiguous or kill escalation required'
    result.update(error=error, response_ns=response, service_ns=service,
                  crc32=expected_crc, observations=len(observations), samples=samples,
                  timer_lateness_ns=lateness, children=cleanup,
                  workload_completion_ns=(workload_finished-workload_started) if workload_finished else None,
                  arm_wall_ns=time.monotonic_ns()-started, harness_cpu_ns=time.process_time_ns()-start_cpu)
    emit(result)
    return result


def delta(before, after):
    if before is None or after is None:
        return {'status': 'unavailable', 'delta': None}
    if after < before:
        return {'status': 'counter_reset', 'delta': None}
    return {'status': 'available', 'delta': after-before}


def context_delta(samples):
    if not samples:
        return None
    first, last = samples[0]['host'], samples[-1]['host']
    a, b = first['cpu_ticks'], last['cpu_ticks']
    busy = None
    if a and b and all(y >= x for x, y in zip(a, b)):
        total = sum(b)-sum(a)
        busy = (total-(b[3]-a[3])-(b[4]-a[4]))/total if total else None
    return {'sample_window_ns': last['monotonic_ns']-first['monotonic_ns'],
            'cpu_busy_fraction': busy,
            'minimum_mem_available_kib': min((s['host']['mem_available_kib'] for s in samples
                                            if s['host']['mem_available_kib'] is not None), default=None),
            'swap_pages': {key: delta(first['swap_pages'][key], last['swap_pages'][key]) for key in ('pswpin', 'pswpout')},
            'psi_total_us': {kind: {level: delta((first['psi'][kind] or {}).get(level, {}).get('total'),
                                                   (last['psi'][kind] or {}).get(level, {}).get('total'))
                                    for level in ('some', 'full')} for kind in ('cpu', 'memory', 'io')},
            'zram_first': first['zram'], 'zram_last': last['zram']}


def summarize(runs):
    summaries = {}
    for arm in ARMS:
        group = [r for r in runs if r['arm'] == arm and r['status'] == 'complete']
        rss = {name: [s['processes'][name]['rss_kib'] for r in group for s in r['samples']
                      if name in s['processes'] and s['processes'][name]['rss_kib'] is not None]
               for name in ('workload', 'observer', 'harness')}
        completion = [r['workload_completion_ns'] for r in group]
        summaries[arm] = {'complete_runs': len(group),
            'response_ns': quantiles([v for r in group for v in r['response_ns']]),
            'service_ns': quantiles([v for r in group for v in r['service_ns']]),
            'per_run_response_ns': [quantiles(r['response_ns']) for r in group],
            'completion_ns': quantiles(completion),
            'completion_cv': statistics.stdev(completion)/statistics.mean(completion) if len(completion)>1 else None,
            'host_context_per_run': [context_delta(r['samples']) for r in group],
            'process_rss_kib': {k: quantiles(v) for k, v in rss.items()},
            'child_cpu_ns': {name: quantiles([c['usage']['user_cpu_ns']+c['usage']['system_cpu_ns'] for r in group
                              for c in r['children'] if c['usage'] and c['name'] == name])
                              for name in ('workload', 'observer')},
            'harness_cpu_ns': quantiles([r['harness_cpu_ns'] for r in group]),
            'timer_lateness_ns': quantiles([v for r in group for v in r['timer_lateness_ns']])}
    paired = []
    for repeat in sorted({r['repeat'] for r in runs}):
        pair = {r['arm']: r for r in runs if r['repeat'] == repeat and r['status'] == 'complete'}
        if len(pair) == 2:
            a, b = pair['baseline'], pair['observe']
            paired.append({'repeat': repeat,
                'completion_observe_over_baseline': b['workload_completion_ns']/a['workload_completion_ns'],
                'p50_response_observe_over_baseline': statistics.median(b['response_ns'])/statistics.median(a['response_ns'])})
    return {'arms': summaries, 'paired': paired, 'failed_runs': sum(r['status'] != 'complete' for r in runs),
            'interpretation': 'timer lateness is a harness scheduling proxy, not GUI/frame/input latency; shared telemetry is context, not attributed benefit',
            'active_optimization': {'status': 'NO-GO', 'available': False, 'reason': 'no demonstrated admissible active action; simulation is excluded'}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    root = Path(__file__).resolve().parents[1]
    parser.add_argument('--worker', type=Path, default=root/'target/release/xynoos-perf')
    parser.add_argument('--observer', type=Path, default=root/'target/release/xynoos-mvp')
    parser.add_argument('--environment', required=True, choices=('Codespaces', 'physical'))
    parser.add_argument('--label', required=True, help='machine/session description; do not claim another host')
    parser.add_argument('--output', type=Path, required=True, help='new directory; never overwritten')
    parser.add_argument('--repeats', type=int, default=8)
    parser.add_argument('--requests', type=int, default=200)
    parser.add_argument('--interval-ms', type=int, default=10)
    parser.add_argument('--timeout', type=float, default=25)
    args = parser.parse_args()
    if not (2 <= args.repeats <= 30 and args.repeats % 2 == 0 and 10 <= args.requests <= 1000
            and 0 <= args.interval_ms <= 100 and math.isfinite(args.timeout) and 1 <= args.timeout <= 25):
        parser.error('require even repeats 2..30, requests 10..1000, interval 0..100, timeout 1..25 seconds')
    args.worker, args.observer = args.worker.resolve(), args.observer.resolve()
    for binary in (args.worker, args.observer):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            parser.error('missing executable: ' + str(binary))
    if os.environ.get('CODESPACES') == 'true' and args.environment != 'Codespaces':
        parser.error('Codespaces must be labelled Codespaces')
    args.output.mkdir(parents=True, exist_ok=False)
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, interrupt)
    runs = []
    status = 'incomplete'
    with (args.output/'events.jsonl').open('x', buffering=1) as log:
        def emit(event):
            log.write(json.dumps(event, separators=(',', ':'), allow_nan=False)+'\n')
            if event['type'] in ('manifest', 'run_start', 'run', 'end'):
                log.flush()
                os.fsync(log.fileno())
        manifest = {'type': 'manifest', 'schema_version': 1, 'environment': args.environment, 'label': args.label,
                    'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'platform': platform.platform(),
                    'cpuinfo': read('/proc/cpuinfo', 1048576), 'os_release': read('/etc/os-release'),
                    'clock_ticks_per_second': os.sysconf('SC_CLK_TCK'),
                    'configuration': {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
                    'binary_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (args.worker, args.observer)},
                    'harness_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                    'source_commit': read(Path(__file__).resolve().parent/'SOURCE_COMMIT'),
                    'source_manifest': read(Path(__file__).resolve().parent/'SOURCE_MANIFEST.json'),
                    'initial_host': host(), 'warmup': 'first arm pair recorded as warmup and excluded; one work request in flight; interval after completion',
                    'order': 'AB/BA alternating; even number of measured pairs', 'physical_mutation_available': False}
        emit(manifest)
        try:
            for repeat in range(-1, args.repeats):
                order = ARMS if repeat % 2 == 0 else tuple(reversed(ARMS))
                for arm in order:
                    r = run_arm(arm, args, emit, repeat)
                    if repeat >= 0:
                        runs.append(r)
                    if r['status'] != 'complete':
                        raise RuntimeError(r['error'])
                    if STOP:
                        raise RuntimeError('cancelled: '+STOP)
                print(f'pair {repeat}: complete', flush=True)
            if len({r['crc32'] for r in runs}) != 1:
                raise RuntimeError('cross-arm corpus mismatch')
            status = 'complete'
        except (Exception, KeyboardInterrupt) as exc:
            emit({'type': 'failure', 'error': f'{type(exc).__name__}: {exc}'})
        summary = {'schema_version': 1, 'status': status, 'environment': args.environment,
                   'label': args.label, 'expected_measured_runs': args.repeats*2,
                   'recorded_measured_runs': len(runs), **summarize(runs)}
        emit({'type': 'end', 'status': status})
        with (args.output/'summary.json').open('x') as f:
            json.dump(summary, f, indent=2, allow_nan=False)
            f.write('\n')
            f.flush()
            os.fsync(f.fileno())
    print(json.dumps(summary, indent=2))
    return 0 if status == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
