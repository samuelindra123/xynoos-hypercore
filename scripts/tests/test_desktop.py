#!/usr/bin/env python3
"""Adversarial admission, identity, lifecycle and independent real-task oracles."""
import argparse
import collections
import contextlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import zlib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import benchmark_desktop as b
import laptop_diagnostic as d
from inspect_desktop import inspect


class DesktopTests(unittest.TestCase):
    def setUp(self):
        b.STOP = None
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name)
        self.corpus = self.path / 'source.txt'
        self.corpus.write_bytes((b'memory cgroup pressure Result Some memory\n' * 2000) + b'end')
        self.fd, self.data = b.sealed_corpus(self.corpus)
        self.args = argparse.Namespace(worker=ROOT / 'target/release/xynoos-desktop',
            observer=ROOT / 'target/release/xynoos-mvp', requests=16, interval_ms=5,
            index_passes=2, timeout=5)
        self.rows = []

    def tearDown(self):
        os.close(self.fd)
        self.temp.cleanup()
        b.STOP = None

    def run_arm(self, arm='active'):
        row = b.run_arm(arm, self.args, self.fd, self.data, self.rows.append, 0)
        self.assertTrue(all(c['reaped'] for c in row['children']), row)
        return row

    @contextlib.contextmanager
    def quiet_policy_fixture(self):
        # These two tests verify real owned workers, protocol and result oracles.
        # Host-wide PSI is nondeterministic after CI builds/package installation.
        # Only the policy pressure averages are fixtures; identities, counters,
        # memory headroom, subprocesses and output oracles stay real. Dedicated
        # admission tests below still assert PSI refusal and pressure deferral.
        collect = d.snapshot
        def sample():
            value = collect()
            for reading in value['psi'].values():
                if reading:
                    for level in reading.values():
                        for key in ('avg10', 'avg60', 'avg300'):
                            level[key] = 0.0
            return value
        with patch.object(d, 'snapshot', side_effect=sample):
            yield

    def test_real_tasks_match_independent_whole_corpus_index(self):
        with self.quiet_policy_fixture():
            row = self.run_arm()
        self.assertEqual(row['status'], 'complete', row['error'])
        words = collections.Counter(w.lower() for w in re.findall(rb'[a-zA-Z0-9]+', self.data))
        crc = 0
        for word, count in sorted(words.items()):
            crc = zlib.crc32(word + b'\0' + count.to_bytes(8, 'little'), crc)
        self.assertEqual(row['index_results'][0]['index_crc32'], crc)
        self.assertEqual(row['search_latency_ns']['count'], 16)
        self.assertTrue(any(r['type'] == 'admission' and r['outcome'] == 'deferred_foreground' for r in self.rows))
        self.assertTrue(all(c['final']['released'] for c in row['children'] if c['name'] != 'observer'))

    def test_matched_three_arms_equal_work_and_observer_contract(self):
        with self.quiet_policy_fixture():
            rows = [self.run_arm(arm) for arm in b.ARMS]
        self.assertTrue(all(r['status'] == 'complete' for r in rows),
                        [{k: r.get(k) for k in ('arm', 'status', 'error')} for r in rows])
        self.assertEqual(len({json.dumps((r['search_results'], r['index_results'])) for r in rows}), 1)
        self.assertEqual(rows[0]['observations'], 0)
        self.assertGreater(rows[1]['observations'], 0)
        self.assertFalse(b.summarize(rows)['desktop_improvement_established'])

    def test_sealed_corpus_cannot_be_modified(self):
        with self.assertRaises(PermissionError):
            os.write(self.fd, b'x')
        self.assertEqual(os.pread(self.fd, len(self.data), 0), self.data)

    def test_missing_memory_refuses_before_children(self):
        sample = d.snapshot()
        sample['mem_available_kib'] = None
        with patch.object(d, 'snapshot', return_value=sample):
            row = self.run_arm()
        self.assertEqual(row['status'], 'incomplete')
        self.assertEqual(row['children'], [])

    def test_low_memory_after_launch_stops_all_children(self):
        sample = d.snapshot()
        low = dict(sample, mem_available_kib=0)
        with patch.object(d, 'snapshot', side_effect=[sample, sample, low]):
            row = self.run_arm()
        self.assertEqual(row['status'], 'incomplete')
        self.assertEqual(len(row['children']), 3)

    def test_admission_missing_reset_stale_pressure_and_oom(self):
        for kind in ('cpu', 'memory', 'io'):
            sample = d.snapshot()
            sample['psi'][kind] = None
            self.assertEqual(d.admission(sample), 'refused_psi_unavailable')
        sample = d.snapshot()
        self.assertEqual(d.admission(sample, now=sample['read_start_ns'] + 300_000_001), 'refused_stale_telemetry')
        before = d.snapshot()
        after = d.snapshot()
        after['psi']['cpu']['some']['total'] = before['psi']['cpu']['some']['total'] - 1
        self.assertEqual(d.admission(after, before), 'refused_counter_reset')
        after = d.snapshot()
        after['psi']['memory']['some']['avg10'] = 3
        self.assertEqual(d.admission(after), 'deferred_pressure')
        after = d.snapshot()
        after['vm']['oom_kill'] = before['vm']['oom_kill'] + 1
        self.assertEqual(d.admission(after, before), 'refused_oom_observed')
        after = d.snapshot()
        after['effective_cgroup']['ancestors'][0]['values']['memory.max'] = '1'
        self.assertEqual(d.admission(after), 'refused_cgroup_headroom')

    def test_nan_psi_and_cgroup_change_refuse(self):
        sample = d.snapshot()
        sample['psi']['cpu']['some']['avg10'] = float('nan')
        self.assertEqual(d.admission(sample), 'refused_psi_unavailable')
        before, after = d.snapshot(), d.snapshot()
        after['effective_cgroup']['device_inode'] = [0, 0]
        self.assertEqual(d.admission(after, before), 'refused_cgroup_changed')

    def test_identity_mismatch_stops_admission_and_reaps_exact_child(self):
        with selectors.DefaultSelector() as selector:
            child = b.OwnedChild(self.args.worker, 'search', self.fd, selector)
            try:
                child.initial_identity = dict(child.initial_identity, start_ticks=0)
                with self.assertRaisesRegex(RuntimeError, 'identity'):
                    child.send('work')
            finally:
                result = child.stop()
            self.assertTrue(result['reaped'])

    def test_stale_telemetry_and_partial_pipe_write_fail_closed(self):
        with selectors.DefaultSelector() as selector:
            child = b.OwnedChild(self.args.worker, 'search', self.fd, selector)
            try:
                sample = d.snapshot()
                sample['read_start_ns'] -= 1_000_000_000
                with self.assertRaisesRegex(RuntimeError, 'fresh telemetry'):
                    child.send('work', sample)
                from unittest.mock import Mock
                with patch.object(child.proc, 'stdin', Mock(write=Mock(return_value=1))):
                    with self.assertRaisesRegex(RuntimeError, 'ambiguous partial'):
                        child.send('work', d.snapshot())
            finally:
                self.assertTrue(child.stop()['reaped'])

    def test_group_writable_binary_rejected(self):
        binary = self.path / 'worker'
        binary.write_bytes(self.args.worker.read_bytes())
        binary.chmod(0o775)
        with selectors.DefaultSelector() as selector:
            with self.assertRaisesRegex(RuntimeError, 'caller-owned'):
                b.OwnedChild(binary, 'search', self.fd, selector)

    def test_cancel_before_launch_and_ambiguous_protocol_stop(self):
        b.STOP = 'SIGINT'
        row = self.run_arm()
        self.assertEqual(row['status'], 'incomplete')
        self.assertTrue(all(c['reaped'] for c in row['children']))
        b.STOP = None
        self.args.worker = ROOT / 'target/release/xynoos-perf'
        row = self.run_arm()
        self.assertEqual(row['status'], 'incomplete')

    def test_worker_input_bound_and_parent_eof(self):
        for payload in (b'not-an-operation\n', b'x' * 1000):
            result = subprocess.run([self.args.worker, 'index', '--corpus-fd', str(self.fd)],
                pass_fds=(self.fd,), input=payload, capture_output=True, timeout=2)
            self.assertNotEqual(result.returncode, 0)
        result = subprocess.run([self.args.worker, 'index', '--corpus-fd', str(self.fd)],
            pass_fds=(self.fd,), input=b'', capture_output=True, timeout=2)
        self.assertEqual(result.returncode, 0)
        self.assertTrue(json.loads(result.stdout.splitlines()[-1])['released'])

    def test_truncated_logs_do_not_certify_cleanup(self):
        path = self.path / 'partial.jsonl'
        content = (json.dumps({'type': 'manifest', 'configuration': {'repeats': 6}, 'environment': 'physical'}) + '\n' +
            json.dumps({'type': 'run_start', 'repeat': 0, 'arm': 'active'}) + '\n{"type":')
        path.write_text(content)
        row = inspect(path)
        self.assertEqual(row['status'], 'partial')
        self.assertTrue(row['truncated_final_row'])
        self.assertEqual(row['unresolved_cleanup_unknown'], [(0, 'active')])
        self.assertEqual(path.read_text(), content)
        path.write_text('{broken}\n')
        with self.assertRaises(ValueError):
            inspect(path)

    def test_signal_preserves_incomplete_evidence_and_reaps(self):
        command = [sys.executable, str(ROOT / 'scripts/benchmark_desktop.py'), '--worker', str(self.args.worker),
            '--observer', str(self.args.observer), '--corpus', str(self.corpus), '--environment', 'Codespaces',
            '--label', 'signal fixture', '--output', str(self.path / 'run'), '--requests', '1024',
            '--interval-ms', '20', '--authorize-disposable-index-admission']
        with subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE) as proc:
            events = self.path / 'run/events.jsonl'
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if events.exists() and '"type":"task"' in events.read_text():
                    break
                if proc.poll() is not None:
                    self.fail(proc.stderr.read().decode())
                time.sleep(.01)
            else:
                proc.kill()
                self.fail('no tasks before signal deadline')
            proc.send_signal(signal.SIGTERM)
            proc.communicate(timeout=5)
            self.assertEqual(proc.returncode, 1)
        report = inspect(events)
        self.assertEqual(report['status'], 'partial')
        rows = [json.loads(line) for line in events.read_text().splitlines()]
        self.assertTrue(all(c['reaped'] for r in rows if r['type'] == 'run' for c in r['children']))

    def test_parent_sigkill_leaves_unknown_log_but_no_live_owned_workers(self):
        command = [sys.executable, str(ROOT / 'scripts/benchmark_desktop.py'), '--worker', str(self.args.worker),
            '--observer', str(self.args.observer), '--corpus', str(self.corpus), '--environment', 'Codespaces',
            '--label', 'parent crash fixture', '--output', str(self.path / 'crash'), '--requests', '1024',
            '--interval-ms', '20', '--authorize-disposable-index-admission']
        children = []
        with subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE) as proc:
            events = self.path / 'crash/events.jsonl'
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if events.exists():
                    raw = events.read_text()
                    if '"type":"task"' in raw:
                        children = [json.loads(line)['identity'] for line in raw.splitlines()
                                    if '"type":"child_started"' in line]
                        break
                if proc.poll() is not None:
                    self.fail(proc.stderr.read().decode())
                time.sleep(.01)
            proc.kill()
            proc.communicate(timeout=3)
        self.assertEqual(len(children), 2)
        # First arm is baseline. Unreaped or reparented zombies are terminated, but the
        # crashed harness cannot certify wait4/reclamation; inspector must stay partial.
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            live = []
            for child in children:
                raw = b.common.read('/proc/' + str(child['pid']) + '/stat')
                if raw:
                    fields = raw.rsplit(')', 1)[-1].split()
                    if int(fields[19]) == child['start_ticks'] and fields[0] != 'Z':
                        live.append(child['pid'])
            if not live:
                break
            time.sleep(.01)
        self.assertEqual(live, [])
        self.assertEqual(inspect(events)['status'], 'partial')
        self.assertTrue(inspect(events)['unresolved_cleanup_unknown'])

    def test_active_without_authorization_refused(self):
        result = subprocess.run([sys.executable, str(ROOT / 'scripts/benchmark_desktop.py'),
            '--corpus', str(self.corpus), '--environment', 'Codespaces', '--label', 'refusal',
            '--output', str(self.path / 'unauthorized'), '--arms', 'all'], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.path / 'unauthorized').exists())


class DeterministicAdmissionTests(unittest.TestCase):
    def sample(self, stamp=2000):
        fields = {'avg10': 0.0, 'avg60': 0.0, 'avg300': 0.0, 'total': 10}
        return {'read_start_ns': stamp - 100, 'monotonic_ns': stamp,
            'mem_available_kib': 1024 * 1024,
            'psi': {kind: {level: dict(fields) for level in ('some', 'full')}
                    for kind in ('cpu', 'memory', 'io')},
            'vm': {'pswpin': 0, 'pswpout': 0, 'oom_kill': 0},
            'effective_cgroup': {'status': 'resolved', 'membership': '0::/test',
                'device_inode': [1, 2], 'ancestors': [{'path': '/test',
                    'values': {'memory.current': '1024', 'memory.max': 'max'}}]}}

    def test_low_pressure_admits_and_missing_full_refuses(self):
        sample = self.sample()
        self.assertEqual(d.admission(sample, now=2000), d.Admission.ADMITTED)
        del sample['psi']['memory']['full']
        self.assertEqual(d.admission(sample, now=2000), d.Admission.PSI)

    def test_short_window_and_full_counter_reset_refuse(self):
        before, after = self.sample(), self.sample(100002000)
        after['psi']['cpu']['some']['total'] += 20000
        self.assertEqual(d.admission(after, before, now=after['monotonic_ns']), d.Admission.PRESSURE)
        after = self.sample(100002000)
        after['psi']['memory']['full']['total'] = 0
        self.assertEqual(d.admission(after, before, now=after['monotonic_ns']), d.Admission.RESET)

    def test_telemetry_scope_and_limits_fail_closed(self):
        sample = self.sample()
        sample['effective_cgroup']['status'] = 'unresolved'
        self.assertEqual(d.admission(sample, now=2000), d.Admission.CGROUP)
        sample = self.sample()
        sample['effective_cgroup']['ancestors'][0]['values']['memory.max'] = 'invalid'
        self.assertEqual(d.admission(sample, now=2000), d.Admission.CGROUP_TELEMETRY)
        self.assertEqual(d.admission(self.sample(), now=1), d.Admission.STALE)


if __name__ == '__main__':
    unittest.main()
