#!/usr/bin/env python3
"""Matched source-search/indexing experiment. No cgroup, scheduling or application writes."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import selectors
import signal
import stat
import subprocess
import sys
import time

import benchmark_physical as common
import laptop_diagnostic as diag

ARMS = ('baseline', 'observe', 'active')
STOP = None


def stop(sig, _frame):
    global STOP
    STOP = signal.Signals(sig).name


def identity(pid):
    status = common.numbers(common.read(f'/proc/{pid}/status'))
    raw = common.read(f'/proc/{pid}/stat')
    fields = raw.rsplit(')', 1)[-1].split() if raw else []
    exe = os.stat(f'/proc/{pid}/exe')
    return {'pid': pid, 'uid': status.get('Uid'), 'ppid': status.get('PPid'),
            'start_ticks': int(fields[19]), 'exe_device_inode': [exe.st_dev, exe.st_ino],
            'cgroup': common.read(f'/proc/{pid}/cgroup'),
            'children': common.read(f'/proc/{pid}/task/{pid}/children')}


class OwnedChild(common.Child):
    """Single reaper, pinned executable, pidfd signals; never signal a saved PID."""
    def __init__(self, binary, role, corpus_fd, selector):
        self.name = role
        self.usage = None
        self.pidfd = None
        self.buffers = {'stdout': b'', 'stderr': b''}
        self.bytes = 0
        binary_fd = os.open(binary, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            info = os.fstat(binary_fd)
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o6022:
                raise RuntimeError('worker must be a caller-owned regular file without group/other write access')
            self.expected_exe = [info.st_dev, info.st_ino]
            command = ([str(binary), 'observe', '--count', '60', '--interval-ms', '1000'] if role == 'observer'
                       else [str(binary), role, '--corpus-fd', str(corpus_fd)])
            self.proc = subprocess.Popen(command, executable=f'/proc/self/fd/{binary_fd}',
                pass_fds=(binary_fd,) if role == 'observer' else (binary_fd, corpus_fd),
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
        finally:
            os.close(binary_fd)
        try:
            self.pidfd = os.pidfd_open(self.proc.pid)
            self.initial_identity = identity(self.proc.pid)
            self.check_identity()
            for channel in self.buffers:
                stream = getattr(self.proc, channel)
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, (self, channel))
        except BaseException:
            self.stop()
            raise
        self.ready = role == 'observer'
        self.pending = None
        self.count = 0
        self.final = None
        self.finish_sent = False

    def check_identity(self):
        if self.reap() is not None:
            raise RuntimeError(self.name + ' exited unexpectedly')
        actual = identity(self.proc.pid)
        if (actual != self.initial_identity or actual['uid'] != os.geteuid() or
            actual['ppid'] != os.getpid() or actual['exe_device_inode'] != self.expected_exe or
            not actual['cgroup'] or actual['cgroup'] != common.read('/proc/self/cgroup') or
            actual['children'] is None or actual['children'].strip()):
            raise RuntimeError(self.name + ' identity/ownership/cgroup/descendant mismatch')

    def send(self, command, snapshot=None, previous=None):
        self.check_identity()
        if command == 'work' and (snapshot is None or diag.admission(snapshot, previous).startswith('refused')):
            raise RuntimeError('fresh telemetry required immediately before work')
        if self.finish_sent:
            raise RuntimeError('command after terminal command')
        payload = command.encode() + b'\n'
        if self.proc.stdin.write(payload) != len(payload):
            raise RuntimeError('ambiguous partial pipe write; command will not be retried')
        self.proc.stdin.flush()
        if command in ('finish', 'cancel'):
            self.finish_sent = True

    def stop(self):
        escalation = False
        if self.proc.stdin and not self.proc.stdin.closed:
            self.proc.stdin.close()
        if self.reap() is None:
            # pidfd is authoritative; unreaped direct-child PID is startup fallback only.
            if self.pidfd is not None:
                signal.pidfd_send_signal(self.pidfd, signal.SIGTERM)
            else:
                os.kill(self.proc.pid, signal.SIGTERM)
        deadline = time.monotonic() + 1
        while self.reap() is None and time.monotonic() < deadline:
            time.sleep(.005)
        if self.reap() is None:
            escalation = True
            signal.pidfd_send_signal(self.pidfd, signal.SIGKILL) if self.pidfd is not None else os.kill(self.proc.pid, signal.SIGKILL)
            deadline = time.monotonic() + 1
            while self.reap() is None and time.monotonic() < deadline:
                time.sleep(.005)
        for channel in ('stdout', 'stderr'):
            getattr(self.proc, channel).close()
        if self.pidfd is not None:
            os.close(self.pidfd)
            self.pidfd = None
        return {'name': self.name, 'identity': getattr(self, 'initial_identity', None),
                'reaped': self.proc.returncode is not None, 'exit_code': self.proc.returncode,
                'kill_escalation': escalation, 'usage': self.usage,
                'final': getattr(self, 'final', None),
                'stderr_tail': self.buffers['stderr'][-4096:].decode(errors='replace')}


def make_corpus(root, destination):
    """Curated tracked source, no user projects, symlinks or archives; immutable memfd per run."""
    paths = subprocess.check_output(['git', 'ls-files', '-z', 'crates', 'scripts'], cwd=root).split(b'\0')
    files, chunks = {}, []
    for raw in sorted(p for p in paths if p):
        path = Path(os.fsdecode(raw))
        if path.suffix not in ('.rs', '.py'):
            continue
        source = root / path
        source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        with os.fdopen(source_fd, 'rb') as stream:
            if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                raise RuntimeError('source corpus symlink/nonregular file')
            data = stream.read(512 * 1024 + 1)
        if len(data) > 512 * 1024:
            raise RuntimeError('source file too large')
        files[str(path)] = hashlib.sha256(data).hexdigest()
        # Explicit byte-preserving ASCII escape of non-ASCII source bytes.
        chunks.append(b'\nFILE ' + raw + b'\n' + data.decode('utf-8').encode('ascii', 'backslashreplace') + b'\n')
    corpus = b''.join(chunks)
    if not 1 <= len(corpus) <= 4 * 1024 * 1024:
        raise RuntimeError('corpus outside 1 byte..4 MiB')
    destination.write_bytes(corpus)
    destination.with_suffix('.manifest.json').write_text(json.dumps({
        'files_sha256': files, 'corpus_sha256': hashlib.sha256(corpus).hexdigest(),
        'bytes': len(corpus), 'encoding': 'UTF-8 source to ASCII backslashreplace; file markers included'}, indent=2)+'\n')


def sealed_corpus(path):
    import fcntl
    with path.open('rb') as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise RuntimeError('corpus must be regular')
        data = source.read(4 * 1024 * 1024 + 1)
    if not 1 <= len(data) <= 4 * 1024 * 1024 or not data.isascii():
        raise RuntimeError('invalid bounded ASCII corpus')
    fd = os.memfd_create('xynoos-disposable-source', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    try:
        with os.fdopen(os.dup(fd), 'wb') as stream:
            stream.write(data)
        os.lseek(fd, 0, os.SEEK_SET)
        fcntl.fcntl(fd, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        return fd, data
    except BaseException:
        os.close(fd)
        raise


def run_arm(arm, args, fd, corpus, emit, repeat):
    started, cpu = time.monotonic_ns(), time.process_time_ns()
    children, samples, searches, indexes = [], [], [], []
    error = None
    observations = 0
    foreground_with_index = 0
    background_with_search = 0
    result = {'type': 'run', 'arm': arm, 'repeat': repeat, 'status': 'incomplete'}
    emit({'type': 'run_start', 'arm': arm, 'repeat': repeat, 'monotonic_ns': started})
    previous = current = None
    observed = arm == 'baseline'
    next_request = started
    next_sample = started
    last_decision = None
    foreground_done = background_done = workload_started = None
    finish_deadline = None
    preflight = diag.Admission.CANCELLED if STOP else diag.admission(diag.snapshot())
    if preflight.startswith('refused'):
        result.update(error=preflight, children=[], search_results=[], index_results=[])
        emit(result)
        return result
    import zlib
    expected_crc = zlib.crc32(corpus)
    oracle = [sum(term in line for line in corpus.splitlines()) for term in (b'memory', b'pressure', b'Result', b'cgroup')]
    import collections
    import re
    words = collections.Counter(word.lower() for word in re.findall(rb'[a-zA-Z0-9]+', corpus))
    oracle_index_crc = 0
    for word, count in sorted(words.items()):
        oracle_index_crc = zlib.crc32(word + b'\0' + count.to_bytes(8, 'little'), oracle_index_crc)
    oracle_words = len(words)
    del words
    deadline_started = time.monotonic_ns()
    with selectors.DefaultSelector() as selector:
        try:
            current = diag.snapshot()
            decision = diag.admission(current)
            if decision.startswith('refused'):
                raise RuntimeError(decision)
            for role in ('search', 'index'):
                if STOP or diag.admission(current).startswith('refused'):
                    raise RuntimeError('cancelled or telemetry expired before launch')
                child = OwnedChild(args.worker, role, fd, selector)
                children.append(child)
                emit({'type': 'child_started', 'arm': arm, 'repeat': repeat, 'role': role, 'identity': child.initial_identity})
            search, index = children
            if arm != 'baseline':
                if STOP or diag.admission(current).startswith('refused'):
                    raise RuntimeError('cancelled or telemetry expired before observer launch')
                child = OwnedChild(args.observer, 'observer', fd, selector)
                children.append(child)
                emit({'type': 'child_started', 'arm': arm, 'repeat': repeat, 'role': 'observer', 'identity': child.initial_identity})
            while True:
                now = time.monotonic_ns()
                if STOP:
                    raise RuntimeError('cancelled: ' + STOP)
                if now - deadline_started > args.timeout * 1e9:
                    raise RuntimeError('arm timeout; no further work admitted')
                if now >= next_sample:
                    previous, current = current, diag.snapshot()
                    decision = diag.admission(current, previous)
                    if decision.startswith('refused'):
                        raise RuntimeError(decision)
                    sample = {'host': current, 'processes': {c.name: common.proc_sample(c.proc.pid)
                              for c in children if c.proc.returncode is None}, 'harness': common.proc_sample(os.getpid())}
                    samples.append(sample)
                    emit({'type': 'sample', 'arm': arm, 'repeat': repeat, **sample})
                    for child in children:
                        if not child.finish_sent:
                            child.check_identity()
                    next_sample = now + 100_000_000
                both_ready = all(c.ready for c in children)
                if both_ready and observed:
                    if workload_started is None:
                        workload_started = next_request = now
                    # Foreground arrivals use a fixed absolute schedule, so a stall cannot disappear as think time.
                    if search.pending is None and search.count < args.requests and now >= next_request:
                        due = next_request
                        foreground_with_index += int(index.pending is not None)
                        search.send('work', current, previous)
                        search.pending = (time.monotonic_ns(), due)
                    decision = diag.admission(current, previous)
                    if decision.startswith('refused'):
                        raise RuntimeError(decision)
                    allow = arm != 'active' or (search.pending is None and search.count < args.requests
                            and next_request - now >= 5_000_000 and decision == 'admitted') or (
                            search.count == args.requests and decision == 'admitted')
                    if index.pending is None and len(indexes) < args.index_passes:
                        policy = diag.Admission.ADMITTED if allow else (diag.Admission.FOREGROUND if search.pending else
                                 (diag.Admission.ARRIVAL if decision == 'admitted' else decision))
                        if policy != last_decision:
                            emit({'type': 'admission', 'arm': arm, 'repeat': repeat, 'outcome': policy,
                                  'telemetry_ns': current['monotonic_ns'], 'index_chunks_completed': index.count})
                            last_decision = policy
                        if allow:
                            background_with_search += int(search.pending is not None)
                            index.send('work', current, previous)
                            index.pending = (time.monotonic_ns(), now)
                for key, _ in selector.select(.001):
                    child, channel = key.data
                    data = os.read(key.fileobj.fileno(), 65536)
                    if not data:
                        selector.unregister(key.fileobj)
                        continue
                    child.bytes += len(data)
                    if child.bytes > 4 * 1024 * 1024:
                        raise RuntimeError('child output bound')
                    child.buffers[channel] += data
                    if len(child.buffers[channel]) > 65536:
                        raise RuntimeError('child line bound')
                    if channel == 'stderr':
                        continue
                    while b'\n' in child.buffers[channel]:
                        line, child.buffers[channel] = child.buffers[channel].split(b'\n', 1)
                        report = json.loads(line)
                        if child.name == 'observer':
                            if (report.get('evidence') != 'live_read_only' or report.get('kernel_mutations') != 0 or
                                report.get('owned_buffer_mutations') != 0 or report['plan']['kernel_write_available']):
                                raise RuntimeError('observer safety contract')
                            observed = True
                            observations += 1
                            emit({'type': 'observation', 'arm': arm, 'repeat': repeat, 'report': report})
                        elif report.get('type') == 'ready' and not child.ready:
                            if (report.get('pid') != child.proc.pid or report.get('role') != child.name or
                                report.get('corpus_crc32') != expected_crc or report.get('bytes') != len(corpus) or
                                report.get('kernel_mutations') != 0 or report['limits']['address_space_bytes'] > 192 * 1024 * 1024 or
                                report['limits']['cpu_seconds'] > 20 or report['limits']['wall_seconds'] != 60):
                                raise RuntimeError('worker readiness/limits/corpus mismatch')
                            child.ready = True
                        elif report.get('type') == 'result' and child.pending is not None:
                            if report.get('sequence') != child.count or report.get('role') != child.name:
                                raise RuntimeError('worker sequence mismatch')
                            sent, due = child.pending
                            completed = time.monotonic_ns()
                            row = {'type': 'task', 'arm': arm, 'repeat': repeat, 'role': child.name,
                                   'response_ns': completed - due, 'roundtrip_ns': completed - sent, 'completed_monotonic_ns': completed, **report}
                            row['type'] = 'task'
                            value = report['result']
                            if child.name == 'search':
                                if value != {'term': ('memory', 'pressure', 'Result', 'cgroup')[child.count % 4],
                                             'matching_lines': oracle[child.count % 4], 'searches': child.count + 1}:
                                    raise RuntimeError('source search oracle mismatch')
                                searches.append(row)
                                next_request += args.interval_ms * 1_000_000
                            elif value['pass_complete']:
                                if (value['passes'] != len(indexes) + 1 or value['offset'] != 0 or
                                    value['index_crc32'] != oracle_index_crc or value['unique_words'] != oracle_words):
                                    raise RuntimeError('index completion mismatch')
                                indexes.append(value)
                                if len(indexes) > 1 and value['index_crc32'] != indexes[0]['index_crc32']:
                                    raise RuntimeError('index mismatch between refreshes')
                            emit(row)
                            child.count += 1
                            child.pending = None
                        elif report.get('type') == 'final' and child.finish_sent and child.final is None:
                            if report.get('released') is not True or report.get('cancelled') is not False:
                                raise RuntimeError('worker final/release mismatch')
                            if report.get('searches') != (args.requests if child.name == 'search' else 0) or report.get('passes') != (args.index_passes if child.name == 'index' else 0) or report.get('partial_index_bytes') != 0:
                                raise RuntimeError('incomplete work at release')
                            child.final = report
                        else:
                            raise RuntimeError('unexpected child report; ambiguous effect')
                now = time.monotonic_ns()
                if len(searches) == args.requests and not search.finish_sent:
                    foreground_done = now - workload_started
                    search.send('finish')
                if len(indexes) == args.index_passes and not index.finish_sent:
                    background_done = now - workload_started
                    index.send('finish')
                if search.finish_sent and index.finish_sent:
                    finish_deadline = finish_deadline or now + 1_000_000_000
                    if now > finish_deadline:
                        raise RuntimeError('release acknowledgement timeout')
                    if search.final and index.final and search.reap() == 0 and index.reap() == 0:
                        final_sample = diag.snapshot()
                        final_admission = diag.admission(final_sample, current)
                        sample = {'host': final_sample, 'processes': {
                            c.name: common.proc_sample(c.proc.pid) for c in children if c.proc.returncode is None},
                            'harness': common.proc_sample(os.getpid())}
                        samples.append(sample)
                        emit({'type': 'sample', 'arm': arm, 'repeat': repeat, **sample})
                        if final_admission.startswith('refused'):
                            raise RuntimeError(final_admission)
                        result['final_admission'] = final_admission
                        result['status'] = 'complete'
                        break
                for child in children:
                    if child.reap() is not None and (not child.finish_sent or child.proc.returncode != 0):
                        raise RuntimeError(child.name + ' premature exit')
        except (Exception, KeyboardInterrupt) as exc:
            error = f'{type(exc).__name__}: {exc}'
        finally:
            cleanup = []
            for child in reversed(children):
                try:
                    if child.name != 'observer' and child.proc.returncode is None and not child.finish_sent:
                        # Cancel has no external rollback: private index is discarded on exit.
                        child.send('cancel')
                        end = time.monotonic() + .2
                        while child.reap() is None and time.monotonic() < end:
                            time.sleep(.005)
                    cleanup.append(child.stop())
                except Exception as exc:
                    try:
                        outcome = child.stop()
                        outcome['cleanup_error'] = str(exc)
                        cleanup.append(outcome)
                    except Exception as final_exc:
                        cleanup.append({'name': child.name, 'reaped': False, 'error': str(final_exc)})
    if not all(c['reaped'] and not c.get('kill_escalation') and not c.get('cleanup_error') for c in cleanup):
        result['status'] = 'incomplete'
        error = (error or '') + '; termination uncertain or escalated'
    result.update(error=error, observations=observations,
                  foreground_requests_with_index_inflight=foreground_with_index,
                  background_chunks_dispatched_during_search=background_with_search, search_latency_ns=common.quantiles([r['response_ns'] for r in searches]),
                  latency_above_1s=sum(r['response_ns'] >= 1_000_000_000 for r in searches),
                  roundtrip_ns=common.quantiles([r['roundtrip_ns'] for r in searches]),
                  search_results=[r['result'] for r in searches], index_results=indexes,
                  foreground_completion_ns=foreground_done, background_completion_ns=background_done,
                  arm_wall_ns=time.monotonic_ns() - started, harness_cpu_ns=time.process_time_ns() - cpu,
                  samples=samples, children=cleanup,
                  host_window=diag.window(samples[0]['host'], samples[-1]['host']) if len(samples) > 1 else None)
    emit(result)
    return result


def summarize(runs):
    pairs = []
    for repeat in sorted({r['repeat'] for r in runs}):
        group = {r['arm']: r for r in runs if r['repeat'] == repeat and r['status'] == 'complete'}
        if {'baseline', 'observe'} <= set(group):
            pairs.append({'repeat': repeat, **{arm: {
                'p95_over_baseline': group[arm]['search_latency_ns']['p95'] / group['baseline']['search_latency_ns']['p95'],
                'foreground_completion_over_baseline': group[arm]['foreground_completion_ns'] / group['baseline']['foreground_completion_ns'],
                'background_completion_over_baseline': group[arm]['background_completion_ns'] / group['baseline']['background_completion_ns']}
                for arm in ('observe', 'active') if arm in group}})
    return {'complete_runs': {arm: sum(r['arm'] == arm and r['status'] == 'complete' for r in runs) for arm in ARMS},
            'paired': pairs, 'failed_runs': sum(r['status'] != 'complete' for r in runs),
            'desktop_improvement_established': False,
            'interpretation': 'source-search task latency only; GUI/browser evidence is separate; '
                              'report background completion cost and session variability; no pooled significance claim'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    root = Path(__file__).resolve().parents[1]
    parser.add_argument('--worker', type=Path, default=root / 'target/release/xynoos-desktop')
    parser.add_argument('--observer', type=Path, default=root / 'target/release/xynoos-mvp')
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--environment', choices=('Codespaces', 'physical'), required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--authorize-disposable-index-admission', action='store_true')
    parser.add_argument('--repeats', type=int, default=6)
    parser.add_argument('--requests', type=int, default=120)
    parser.add_argument('--interval-ms', type=int, default=20)
    parser.add_argument('--index-passes', type=int, default=16)
    parser.add_argument('--timeout', type=int, default=45)
    parser.add_argument('--arms', choices=('baseline-observe', 'all'), default='baseline-observe',
                        help='all adds separately authorized cooperative active arm')
    parser.add_argument('--only-arm', choices=ARMS, help='optional long manual browser-matched sessions; balance order externally')
    parser.add_argument('--prepare-corpus', action='store_true', help='prepare curated repository source and exit')
    args = parser.parse_args()
    if args.prepare_corpus:
        if args.corpus.exists() or args.corpus.with_suffix('.manifest.json').exists():
            parser.error('corpus destination exists')
        make_corpus(root, args.corpus)
        return 0
    if not (args.repeats in (6, 12) and 16 <= args.requests <= 1024 and 1 <= args.index_passes <= 16 and
            5 <= args.interval_ms <= 100 and 5 <= args.timeout <= 45):
        parser.error('repeats 6|12, requests 16..1024, passes 1..16, interval 5..100, timeout 5..45')
    if (args.only_arm == 'active' or (args.only_arm is None and args.arms == 'all')) and not args.authorize_disposable_index_admission:
        parser.error('active requires --authorize-disposable-index-admission; only owned indexing admission is authorized')
    if os.geteuid() == 0:
        parser.error('run as a normal desktop user, without sudo')
    if os.environ.get('CODESPACES') == 'true' and args.environment != 'Codespaces':
        parser.error('Codespaces must be labelled Codespaces')
    args.worker, args.observer = args.worker.resolve(), args.observer.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, stop)
    fd, corpus = sealed_corpus(args.corpus)
    runs = []
    warmups = []
    status = 'incomplete'
    try:
        with (args.output / 'events.jsonl').open('x', buffering=1) as log:
            def emit(row):
                log.write(json.dumps(row, separators=(',', ':'), allow_nan=False) + '\n')
                if row['type'] in ('manifest', 'run_start', 'child_started', 'run', 'end'):
                    log.flush()
                    os.fsync(log.fileno())
            emit({'type': 'manifest', **diag.context(args.label, args.environment), 'schema': 'xynoos-desktop-v1',
                  'configuration': {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
                  'corpus_sha256': hashlib.sha256(corpus).hexdigest(), 'binary_sha256': {
                      str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (args.worker, args.observer)},
                  'source_commit': common.read(Path(__file__).parent / 'SOURCE_COMMIT'),
                  'harness_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  'diagnostic_sha256': hashlib.sha256(Path(diag.__file__).read_bytes()).hexdigest(),
                  'order': 'one selected-arm warmup; measured AB/BA or six ABC permutations balanced; single-arm mode requires external balancing',
                  'authorization_scope': 'only defer new disposable indexing chunks; physical kernel mutation disabled'})
            try:
                selected = ARMS if args.arms == 'all' else ARMS[:2]
                orders = list(itertools.permutations(selected))
                for repeat in range(-1, args.repeats):
                    for arm in ((args.only_arm,) if args.only_arm else (selected if repeat == -1 else orders[repeat % len(orders)])):
                        result = run_arm(arm, args, fd, corpus, emit, repeat)
                        if repeat >= 0:
                            runs.append(result)
                        else:
                            warmups.append({'arm': arm, 'status': result['status'], 'error': result['error']})
                        if result['status'] != 'complete':
                            raise RuntimeError(result['error'])
                        print(f'{repeat} {arm}: complete', flush=True)
                if len({json.dumps((r['search_results'], r['index_results']), sort_keys=True) for r in runs}) != 1:
                    raise RuntimeError('cross-arm output mismatch')
                status = 'complete'
            except (Exception, KeyboardInterrupt) as exc:
                emit({'type': 'failure', 'error': f'{type(exc).__name__}: {exc}'})
            emit({'type': 'end', 'status': status})
            summary = {'status': status, 'environment': args.environment, 'label': args.label,
                       'expected_runs': args.repeats * (1 if args.only_arm else len(selected)), 'warmup': warmups, **summarize(runs)}
            (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
        print(json.dumps(summary, indent=2))
    finally:
        os.close(fd)
    return 0 if status == 'complete' else 1


if __name__ == '__main__':
    raise SystemExit(main())
