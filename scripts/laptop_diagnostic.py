#!/usr/bin/env python3
"""Read-only laptop bottleneck evidence. No workload, service calls or control writes."""
import argparse
import enum
import json
import os
from pathlib import Path
import platform
import signal
import time

from benchmark_physical import read, numbers, host, delta, quantiles, proc_sample

VM_KEYS = ('pgpgin', 'pgpgout', 'pgmajfault', 'pgfault', 'pswpin', 'pswpout',
           'pgscan_direct', 'pgscan_kswapd', 'pgsteal_direct', 'pgsteal_kswapd',
           'allocstall_dma', 'allocstall_dma32', 'allocstall_normal', 'oom_kill')
CG_KEYS = ('memory.current', 'memory.max', 'memory.high', 'memory.low', 'memory.events',
           'memory.stat', 'memory.pressure', 'cpu.stat', 'cpu.max', 'cpu.pressure',
           'io.stat', 'io.pressure', 'cgroup.events', 'cgroup.procs')
STOP = False


class Admission(str, enum.Enum):
    ADMITTED = 'admitted'
    CANCELLED = 'refused_cancelled'
    PRESSURE = 'deferred_pressure'
    FOREGROUND = 'deferred_foreground'
    ARRIVAL = 'deferred_arrival_guard'
    STALE = 'refused_stale_telemetry'
    MEMORY = 'refused_memory_floor'
    CGROUP = 'refused_cgroup_unresolved'
    CGROUP_TELEMETRY = 'refused_cgroup_telemetry'
    HEADROOM = 'refused_cgroup_headroom'
    PSI = 'refused_psi_unavailable'
    RESET = 'refused_counter_reset'
    VM = 'refused_vm_unavailable'
    OOM = 'refused_oom_observed'
    IDENTITY = 'refused_cgroup_changed'
    CLOCK = 'refused_clock'


def cgroup_context():
    membership = read('/proc/self/cgroup')
    mountinfo = read('/proc/self/mountinfo', 1024 * 1024)
    result = {'membership': membership, 'status': 'unresolved', 'ancestors': []}
    try:
        group = next(line[3:] for line in membership.splitlines() if line.startswith('0::'))
        for line in mountinfo.splitlines():
            left, right = line.split(' - ', 1)
            if right.split()[0] != 'cgroup2':
                continue
            fields = left.split()
            root, mount = fields[3:5]
            # Do not guess escaped mount names or paths outside the visible mount.
            if '\\' in root + mount or '..' in Path(group).parts:
                continue
            relative = Path(group).relative_to(root)
            current = Path(mount) / relative
            if not current.is_dir():
                continue
            result.update(status='resolved', path=str(current), mount=mount,
                          device_inode=list((current.stat().st_dev, current.stat().st_ino)))
            while True:
                result['ancestors'].append({'path': str(current),
                    'values': {key: read(current / key) for key in CG_KEYS}})
                if current == Path(mount):
                    return result
                current = current.parent
    except (OSError, ValueError, AttributeError, StopIteration):
        pass
    return result


def snapshot():
    started = time.monotonic_ns()
    sample = host()
    vm = numbers(read('/proc/vmstat'))
    sample.update(vm={k: vm.get(k) for k in VM_KEYS},
                  meminfo=numbers(read('/proc/meminfo')), swaps=read('/proc/swaps'),
                  diskstats=read('/proc/diskstats'),
                  effective_cgroup=cgroup_context(), self_process=proc_sample(os.getpid()))
    sample['read_start_ns'] = started
    sample['monotonic_ns'] = time.monotonic_ns()
    return sample


def background():
    # No cmdlines, open files or user content. At most 256 visible processes.
    rows = []
    try:
        paths = sorted((p for p in Path('/proc').iterdir() if p.name.isdigit()), key=lambda p: int(p.name))
        for p in paths[:256]:
            status = numbers(read(p / 'status'))
            rows.append({'pid': int(p.name), 'comm': read(p / 'comm', 256),
                         'uid': status.get('Uid'), **proc_sample(int(p.name))})
        return {'processes': rows, 'visible_count': len(paths), 'truncated': len(paths) > 256}
    except OSError:
        return None


def context(label, environment):
    return {'schema': 'xynoos-laptop-v1', 'label': label, 'environment': environment,
            'utc_unix_ns': time.time_ns(), 'monotonic_ns': time.monotonic_ns(), 'platform': platform.platform(),
            'os_release': read('/etc/os-release'), 'cpuinfo': read('/proc/cpuinfo', 1024 * 1024),
            'page_size': os.sysconf('SC_PAGE_SIZE'), 'clock_ticks': os.sysconf('SC_CLK_TCK'),
            'cpu_affinity': sorted(os.sched_getaffinity(0)), 'boot_id': read('/proc/sys/kernel/random/boot_id'),
            'session_type': os.environ.get('XDG_SESSION_TYPE'),
            'invocation_id': os.environ.get('INVOCATION_ID'), 'background': background(),
            'block_devices': {p.name: {k: read(p / k) for k in
                ('device/model', 'queue/rotational', 'size', 'stat')} for p in sorted(Path('/sys/block').iterdir())[:32]},
            'power': {p.name: {k: read(p / k) for k in ('type', 'online', 'status', 'capacity')}
                      for p in sorted(Path('/sys/class/power_supply').glob('*'))[:16]},
            'thermal_millidegrees': {p.name: read(p / 'temp') for p in sorted(Path('/sys/class/thermal').glob('thermal_zone*'))[:32]},
            'scope': 'namespace-visible, sequential reads; process names/RSS may be sensitive; no desktop application automation',
            'kernel_mutation_available': False}


def window(first, last):
    elapsed = last['monotonic_ns'] - first['monotonic_ns']
    psi = {kind: {level: delta((first['psi'][kind] or {}).get(level, {}).get('total'),
                               (last['psi'][kind] or {}).get(level, {}).get('total'))
                  for level in ('some', 'full')} for kind in ('cpu', 'memory', 'io')}
    vm = {k: delta(first['vm'].get(k), last['vm'].get(k)) for k in VM_KEYS}
    ticks = None
    a, b = first['cpu_ticks'], last['cpu_ticks']
    if a and b and all(y >= x for x, y in zip(a, b)) and sum(b) > sum(a):
        total = sum(b) - sum(a)
        ticks = {'busy_fraction': 1 - ((b[3] - a[3]) + (b[4] - a[4])) / total,
                 'iowait_fraction': (b[4] - a[4]) / total}
    evidence = []
    for kind, name in (('cpu', 'cpu_contention'), ('memory', 'memory_stalls'), ('io', 'io_stalls')):
        value = psi[kind]['some']['delta']
        if value is not None and elapsed > 0 and value * 1000 / elapsed >= .05:
            evidence.append(name)
    if any(vm[k]['delta'] for k in ('pgscan_direct', 'allocstall_normal')):
        evidence.append('direct_reclaim_activity')
    if any(vm[k]['delta'] for k in ('pswpin', 'pswpout')):
        evidence.append('swap_activity; correlate zram/disk and CPU before attribution')
    if not evidence:
        evidence.append('no_large_PSI_signal_in_this_window; cause_unresolved')
    def disks(text):
        result = {}
        for line in (text or '').splitlines():
            fields = line.split()
            if len(fields) >= 14:
                result[fields[2]] = {key: int(fields[i]) for key, i in
                    (('read_sectors', 5), ('write_sectors', 9), ('read_ms', 6), ('write_ms', 10),
                     ('io_ms', 12), ('weighted_io_ms', 13))}
        return result
    disk_first, disk_last = disks(first['diskstats']), disks(last['diskstats'])
    devices = {name: {key: delta(disk_first[name][key], values[key]) for key in values}
               for name, values in disk_last.items() if name in disk_first}
    before_groups = {a['path']: a['values'] for a in first['effective_cgroup']['ancestors']}
    cgroups = {}
    for ancestor in last['effective_cgroup']['ancestors']:
        name, values = ancestor['path'], ancestor['values']
        old = before_groups.get(name, {})
        cgroups[name] = {kind: {key: delta(numbers(old.get(kind)).get(key), numbers(values.get(kind)).get(key))
            for key in keys} for kind, keys in
            (('cpu.stat', ('usage_usec', 'nr_throttled', 'throttled_usec')),
             ('memory.events', ('high', 'max', 'oom', 'oom_kill')))}
    return {'elapsed_ns': elapsed, 'psi_total_us': psi, 'disk_delta': devices, 'cgroup_delta': cgroups, 'vm_delta': vm, 'cpu': ticks,
            'evidence': evidence,
            'attribution': 'coexisting signals, not exclusive causes; pgpgin/out KiB, pswpin/out pages; '
                           'pgmajfault is a count, not time; PSI full memory supplies stall time; '
                           'CPU full is undefined system-wide; diskstats retain per-device counters; '
                           'zram activity plus CPU PSI suggests overhead but cannot prove compression CPU cost'}


def admission(sample, previous=None, now=None):
    """Fail closed for telemetry ambiguity; pressure defers only future cooperative work."""
    now = time.monotonic_ns() if now is None else now
    if not 0 <= now - sample['read_start_ns'] <= 300_000_000:
        return Admission.STALE
    if sample['mem_available_kib'] is None or sample['mem_available_kib'] < 384 * 1024:
        return Admission.MEMORY
    cg = sample['effective_cgroup']
    if cg['status'] != 'resolved':
        return Admission.CGROUP
    for ancestor in cg['ancestors']:
        values = ancestor['values']
        current, maximum = values.get('memory.current'), values.get('memory.max')
        if current is None or maximum is None or not current.strip().isdigit():
            return Admission.CGROUP_TELEMETRY
        if maximum.strip() != 'max':
            try:
                if int(maximum) - int(current) < 256 * 1024 * 1024:
                    return Admission.HEADROOM
            except ValueError:
                return Admission.CGROUP_TELEMETRY
    for kind in ('cpu', 'memory', 'io'):
        try:
            levels = ('some',) if kind == 'cpu' else ('some', 'full')
            for level in levels:
                full = sample['psi'][kind][level]
                if (not isinstance(full['total'], int) or full['total'] < 0 or
                    any(not isinstance(full[k], (int, float)) or not 0 <= full[k] <= 100
                        for k in ('avg10', 'avg60', 'avg300'))):
                    return Admission.PSI
                if previous and full['total'] < previous['psi'][kind][level]['total']:
                    return Admission.RESET
            fields = sample['psi'][kind]['some']
            if set(fields) != {'avg10', 'avg60', 'avg300', 'total'} or any(
                    not isinstance(fields[k], (int, float)) or not 0 <= fields[k] <= 100
                    for k in ('avg10', 'avg60', 'avg300')) or not isinstance(fields['total'], int) or fields['total'] < 0:
                return Admission.PSI
            if previous:
                old = previous['psi'][kind]['some']['total']
                if fields['total'] < old:
                    return Admission.RESET
        except (TypeError, KeyError):
            return Admission.PSI
    for key in ('pswpin', 'pswpout', 'oom_kill'):
        value = sample['vm'].get(key)
        if value is None:
            return Admission.VM
        if previous and (previous['vm'].get(key) is None or value < previous['vm'][key]):
            return Admission.RESET
        if previous and key == 'oom_kill' and value > previous['vm'][key]:
            return Admission.OOM
    if previous and (sample['effective_cgroup']['membership'] != previous['effective_cgroup']['membership'] or
                     sample['effective_cgroup']['device_inode'] != previous['effective_cgroup']['device_inode']):
        return Admission.IDENTITY
    if any(sample['psi'][k]['some']['avg10'] >= threshold for k, threshold in
           (('cpu', 15), ('memory', 2), ('io', 5))):
        return Admission.PRESSURE
    if previous:
        elapsed = sample['monotonic_ns'] - previous['monotonic_ns']
        if elapsed <= 0:
            return Admission.CLOCK
        if any((sample['psi'][k]['some']['total'] - previous['psi'][k]['some']['total']) * 1000 / elapsed >= threshold
               for k, threshold in (('cpu', .15), ('memory', .02), ('io', .05))):
            return Admission.PRESSURE
    return Admission.ADMITTED


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--environment', choices=('Codespaces', 'physical'), required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--seconds', type=int, default=60)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.seconds <= 600:
        parser.error('seconds must be 1..600')
    if os.environ.get('CODESPACES') == 'true' and args.environment != 'Codespaces':
        parser.error('Codespaces must be labelled Codespaces')
    global STOP
    def stop(_sig, _frame):
        global STOP
        STOP = True
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, stop)
    cpu = time.process_time_ns()
    first = last = None
    with args.output.open('x', buffering=1) as log:
        def emit(row):
            log.write(json.dumps(row, allow_nan=False) + '\n')
        emit({'type': 'context', **context(args.label, args.environment)})
        for _ in range(args.seconds * 4 + 1):
            if STOP:
                break
            last = snapshot()
            first = first or last
            emit({'type': 'sample', 'host': last})
            time.sleep(.25)
        emit({'type': 'end', 'status': 'cancelled' if STOP else 'complete',
              'diagnostic_cpu_ns': time.process_time_ns() - cpu,
              'window': window(first, last) if first and last else None})
        log.flush()
        os.fsync(log.fileno())
    return 1 if STOP else 0


if __name__ == '__main__':
    raise SystemExit(main())
