#!/usr/bin/env python3
"""Lifecycle/adversarial tests exercise only owned subprocesses and read-only telemetry."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'scripts'))
import benchmark_physical as bench
from inspect_physical import inspect

WORKER = ROOT/'target/release/xynoos-perf'
OBSERVER = ROOT/'target/release/xynoos-mvp'


class BenchmarkTests(unittest.TestCase):
    def setUp(self):
        bench.STOP = None
        self.temp = tempfile.TemporaryDirectory(prefix='xynoos-bench-test-')
        self.directory = Path(self.temp.name)
        self.args = argparse.Namespace(worker=WORKER, observer=OBSERVER, requests=10, interval_ms=0, timeout=1)
        self.events = []

    def tearDown(self):
        self.temp.cleanup()
        bench.STOP = None

    def fixture(self, body):
        p = self.directory/'fixture'
        p.write_text('#!'+sys.executable+'\n'+body)
        p.chmod(0o755)
        return p

    def arm(self, arm='baseline'):
        result = bench.run_arm(arm, self.args, self.events.append, 0)
        self.assertTrue(all(c['reaped'] for c in result['children']), result)
        return result

    def test_real_workload_integrity_and_proc_resources(self):
        result = self.arm()
        self.assertEqual(result['status'], 'complete', result['error'])
        self.assertEqual(len(result['response_ns']), 10)
        self.assertTrue(result['children'][0]['usage']['user_cpu_ns'] > 0)
        self.assertIsNotNone(bench.summarize([result])['arms']['baseline']['response_ns'])

    def test_real_observation_overlaps_identical_work(self):
        result = self.arm('observe')
        self.assertEqual(result['status'], 'complete', result['error'])
        self.assertGreater(result['observations'], 0)
        self.assertEqual({c['name'] for c in result['children']}, {'observer', 'workload'})

    def test_high_pressure_stops_and_reaps_launched_worker(self):
        initial = bench.host()
        low = dict(initial, mem_available_kib=1)
        with patch.object(bench, 'host', side_effect=[initial, low]):
            result = self.arm()
        self.assertEqual(result['status'], 'incomplete')
        self.assertIn('256 MiB', result['error'])
        self.assertEqual(len(result['children']), 1)

    def test_lost_memavailable_stops_owned_children(self):
        initial = bench.host()
        with patch.object(bench, 'host', side_effect=[initial, dict(initial, mem_available_kib=None)]):
            result = self.arm()
        self.assertIn('unavailable', result['error'])

    def test_missing_psi_stays_unknown(self):
        self.assertIsNone(bench.psi(None))
        self.assertEqual(bench.delta(None, 10)['status'], 'unavailable')
        self.assertEqual(bench.delta(10, 1)['status'], 'counter_reset')
        self.assertEqual(bench.quantiles([1, 2, 3])['p99'], 3)

    def test_mounted_cgroup_low_headroom_refuses(self):
        with self.assertRaisesRegex(RuntimeError, 'headroom'):
            bench.safe({'mem_available_kib': 1024*1024,
                        'mounted_cgroup_root_context': {'memory.current': '100', 'memory.max': '200'}})

    def test_workload_termination_is_not_a_complete_run(self):
        self.args.worker = self.fixture('import sys\nsys.exit(7)\n')
        result = self.arm()
        self.assertEqual(result['status'], 'incomplete')
        self.assertIn('terminated', result['error'])
        self.assertEqual(result['children'][0]['exit_code'], 7)

    def test_observer_termination_is_not_zero_overhead(self):
        self.args.observer = self.fixture('raise SystemExit(4)\n')
        result = self.arm('observe')
        self.assertEqual(result['status'], 'incomplete')
        self.assertIn('observer', result['error'])

    def test_stalled_worker_times_out_and_is_reaped(self):
        self.args.worker = self.fixture('import time\ntime.sleep(30)\n')
        result = self.arm()
        self.assertIn('timeout', result['error'])

    def test_kill_escalation_is_explicit_and_incomplete(self):
        self.args.worker = self.fixture('import signal,time\nsignal.signal(signal.SIGTERM, signal.SIG_IGN)\ntime.sleep(30)\n')
        result = self.arm()
        self.assertEqual(result['status'], 'incomplete')
        self.assertTrue(result['children'][0]['kill_escalation'])
        self.assertEqual(result['children'][0]['exit_code'], -signal.SIGKILL)

    def test_output_bound_stops_child(self):
        self.args.worker = self.fixture('import sys,time\nsys.stdout.write("x"*70000)\nsys.stdout.flush()\ntime.sleep(30)\n')
        result = self.arm()
        self.assertIn('64 KiB', result['error'])

    def test_refusal_and_missing_decision_telemetry_are_explicit(self):
        result = self.arm('observe')
        self.assertEqual(result['status'], 'complete', result['error'])
        reports = [e['report'] for e in self.events if e['type'] == 'observation']
        self.assertTrue(all(r['kernel_mutations'] == 0 for r in reports))
        self.assertTrue(all('reason' in r['plan'] for r in reports))

    def test_worker_rejects_long_input_and_unsupported_modes(self):
        result = subprocess.run([WORKER, 'workload'], input=b'x'*100000, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertLess(len(result.stdout), 65536)
        self.assertNotEqual(subprocess.run([WORKER, 'optimize'], capture_output=True).returncode, 0)

    def test_signals_preserve_partial_evidence_and_cleanup(self):
        for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            output = self.directory/str(int(sig))
            command = [sys.executable, str(ROOT/'scripts/benchmark_physical.py'), '--environment', 'Codespaces',
                       '--label', 'lifecycle fixture', '--requests', '1000', '--repeats', '2',
                       '--output', str(output)]
            with subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE) as proc:
                deadline = time.monotonic()+5
                events = output/'events.jsonl'
                while time.monotonic() < deadline:
                    if events.exists() and '"type":"response"' in events.read_text():
                        break
                    if proc.poll() is not None:
                        self.fail(proc.stderr.read().decode())
                    time.sleep(.01)
                else:
                    proc.kill()
                    self.fail('no response before signal test deadline')
                proc.send_signal(sig)
                proc.communicate(timeout=5)
                self.assertEqual(proc.returncode, 1)
            rows = [json.loads(line) for line in events.read_text().splitlines()]
            run = next(e for e in rows if e['type'] == 'run')
            self.assertEqual(run['status'], 'incomplete')
            self.assertTrue(all(c['reaped'] for c in run['children']))
            self.assertEqual(json.loads((output/'summary.json').read_text())['status'], 'incomplete')
            self.assertEqual(inspect(events)['status'], 'partial')

    def test_crash_tail_is_read_only_and_cleanup_unknown(self):
        path = self.directory/'partial.jsonl'
        content = (json.dumps({'type':'manifest','environment':'physical','configuration':{'repeats':2}})+'\n'+
                   json.dumps({'type':'run_start','repeat':0,'arm':'observe'})+'\n'+ '{"type":')
        path.write_text(content)
        before = path.read_bytes()
        report = inspect(path)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['unresolved_runs_cleanup_unknown'], [(0,'observe')])
        self.assertTrue(report['truncated_final_event'])
        self.assertEqual(before, path.read_bytes())

    def test_end_marker_cannot_promote_failed_runs(self):
        path = self.directory/'contradictory.jsonl'
        rows = [{'type':'manifest','environment':'physical','configuration':{'repeats':2}}]
        for repeat in range(2):
            for arm in bench.ARMS:
                rows.append({'type':'run', 'repeat':repeat, 'arm':arm, 'status':'incomplete',
                             'children':[], 'response_ns':[], 'service_ns':[]})
        rows.append({'type':'end','status':'complete'})
        path.write_text(''.join(json.dumps(r)+'\n' for r in rows))
        self.assertEqual(inspect(path)['status'], 'partial')

    def test_corrupt_complete_event_is_rejected(self):
        path = self.directory/'corrupt.jsonl'
        path.write_text('{broken}\n')
        with self.assertRaises(ValueError):
            inspect(path)

    def test_existing_output_and_incorrect_environment_are_refused(self):
        command = [sys.executable, str(ROOT/'scripts/benchmark_physical.py'), '--environment', 'Codespaces',
                   '--label', 'fixture', '--output', str(self.directory)]
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
        env = dict(os.environ, CODESPACES='true')
        command[command.index('Codespaces')] = 'physical'
        self.assertIn(b'Codespaces must be labelled', subprocess.run(command, env=env, capture_output=True).stderr)


if __name__ == '__main__':
    unittest.main()
