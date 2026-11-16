#!/usr/bin/env python3
"""Owned lossless experiment: correctness, hard bounds, cancellation and failure evidence."""
import argparse
import copy
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
import benchmark_owned as bench
from inspect_owned import inspect
WORKER = ROOT/'target/release/xynoos-owned'


class OwnedTests(unittest.TestCase):
    def setUp(self):
        bench.STOP = None
        self.temp = tempfile.TemporaryDirectory(prefix='xynoos-owned-test-')
        self.directory = Path(self.temp.name)
        self.args = argparse.Namespace(worker=WORKER, corpus='mixed', authorize_owned=True,
                                       requests=16, interval_ms=1, timeout=4)
        self.events = []

    def tearDown(self):
        self.temp.cleanup()
        bench.STOP = None

    def arm(self, arm='active'):
        result = bench.run_arm(arm, self.args, self.events.append, 0)
        self.assertTrue(all(c['reaped'] for c in result['children']), result)
        return result

    def fixture(self, body):
        p = self.directory/'fixture'
        p.write_text('#!'+sys.executable+'\n'+body)
        p.chmod(0o755)
        return p

    def test_matched_real_intervention_output_and_restoration(self):
        runs = [self.arm(arm) for arm in bench.ARMS]
        self.assertTrue(all(r['status'] == 'complete' for r in runs), [(r['arm'], r['error']) for r in runs])
        report = bench.summary(runs)
        self.assertTrue(report['paired'][0]['correctness_equivalent'])
        active = report['arms']['active']
        # Host CPU/I/O backoff is an admissible outcome on a busy test host.
        if active['actions']:
            self.assertEqual(active['blocks_replaced'], 16)
            self.assertLess(active['steady_payload_bytes']['p50'], 8*1024*1024)
        else:
            self.assertTrue(any(row['policy']['effect'] in ('back_off', 'refused_telemetry', 'refused_memory') for row in runs[-1]['responses']))
        self.assertTrue(all(r['final']['restored'] and not r['final']['cancelled'] for r in runs))
        evidence = [{'type':'manifest', 'configuration':{'repeats':6, 'requests':16}}]
        for repeat in range(-1, 6):
            for r in runs:
                evidence.append({'type':'run_start', 'repeat':repeat, 'arm':r['arm']})
                clone = copy.deepcopy(r)
                clone['repeat'] = repeat
                evidence.append(clone)
        evidence.append({'type':'end', 'status':'complete'})
        path = self.directory/'complete.jsonl'
        def write():
            path.write_text(''.join(json.dumps(row)+'\n' for row in evidence))
        write()
        self.assertEqual(inspect(path)['status'], 'complete')
        evidence[2]['status'] = 'incomplete'
        write()
        self.assertEqual(inspect(path)['status'], 'partial')
        evidence[2]['status'] = 'complete'
        evidence[2]['children'][0]['reaped'] = False
        write()
        self.assertEqual(inspect(path)['status'], 'partial')
        evidence[2]['children'][0]['reaped'] = True
        evidence[-2]['responses'][0]['checksum'] ^= 1
        write()
        self.assertEqual(inspect(path)['status'], 'partial')

    def test_entropy_never_claims_savings(self):
        self.args.corpus = 'entropy'
        run = self.arm()
        self.assertEqual(run['status'], 'complete', run['error'])
        self.assertTrue(all(row['retained_bytes'] == 17*1024*1024 for row in run['responses']))
        self.assertEqual(bench.summary([run])['arms']['active']['actions'], 0)

    def test_no_authorization_no_child(self):
        self.args.authorize_owned = False
        run = self.arm()
        self.assertEqual(run['children'], [])
        self.assertIn('authorize', run['error'])
        p = subprocess.run([WORKER, 'active', 'mixed'], capture_output=True)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(p.stdout, b'')

    def test_hard_resource_limits_and_eof_reverse(self):
        with subprocess.Popen([WORKER, 'active', 'mixed', '--authorize-owned'], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as p:
            ready = json.loads(p.stdout.readline())
            limits = (Path('/proc')/str(p.pid)/'limits').read_text()
            self.assertIn('134217728', limits)
            self.assertIn('Max cpu time              8', limits)
            self.assertEqual(ready['limits']['wall_seconds'], 25)
            p.stdin.write(b'work\nwork\n')
            p.stdin.flush()
            self.assertEqual(json.loads(p.stdout.readline())['type'], 'response')
            self.assertEqual(json.loads(p.stdout.readline())['type'], 'response')
            p.stdin.close()
            final = json.loads(p.stdout.readline())
            self.assertTrue(final['restored'] and final['cancelled'])
            self.assertEqual(p.wait(timeout=2), 0)

    def test_oversized_input_and_request_bound(self):
        p = subprocess.run([WORKER, 'baseline', 'mixed'], input=b'x'*100000, capture_output=True, timeout=4)
        self.assertNotEqual(p.returncode, 0)
        p = subprocess.run([WORKER, 'baseline', 'mixed'], input=b'work\n'*257, capture_output=True, timeout=4)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn(b'request limit', p.stderr)

    def test_worker_death_timeout_and_output_bound(self):
        for body, error in [('raise SystemExit(7)\n', 'worker exit'),
                            ('import time\ntime.sleep(5)\n', 'timeout'),
                            ('import sys,time\nsys.stdout.write("x"*70000)\nsys.stdout.flush()\ntime.sleep(5)\n', '64 KiB')]:
            self.args.worker = self.fixture(body)
            self.args.timeout = .15
            run = self.arm()
            self.assertEqual(run['status'], 'incomplete')
            self.assertIn(error, run['error'])

    def test_missing_memavailable_and_low_headroom_refuse(self):
        for h in [dict(bench.base.host(), mem_available_kib=None),
                  dict(bench.base.host(), mounted_cgroup_root_context={'memory.current':'1','memory.max':'2'})]:
            with patch.object(bench.base, 'host', return_value=h):
                run = self.arm()
            self.assertEqual(run['children'], [])
            self.assertEqual(run['status'], 'incomplete')

    def test_signals_restore_and_record_incomplete(self):
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            bench.STOP = None
            self.events = []
            def emit(row):
                self.events.append(row)
                if row['type'] == 'response' and row.get('request') == 3:
                    bench.interrupt(sig, None)
            run = bench.run_arm('active', self.args, emit, 0)
            self.assertEqual(run['status'], 'incomplete')
            self.assertTrue(run['final']['restored'] and run['final']['cancelled'])
            self.assertTrue(all(c['reaped'] and c['exit_code'] == 0 for c in run['children']))

    def test_crash_tail_inspection_is_read_only(self):
        p = self.directory/'partial.jsonl'
        p.write_text(json.dumps({'type':'manifest', 'configuration':{'repeats':6,'requests':16}})+'\n'+
                     json.dumps({'type':'run_start','arm':'active','repeat':0})+'\n'+ '{"type":')
        before = p.read_bytes()
        report = inspect(p)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['unresolved_cleanup_unknown'], [(0, 'active')])
        self.assertEqual(p.read_bytes(), before)

    def test_independent_watchdog_with_open_idle_stdin(self):
        # Independent worker deadline; harness makes no cancellation call.
        started = time.monotonic()
        with subprocess.Popen([WORKER, 'baseline', 'mixed'], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as p:
            self.assertEqual(json.loads(p.stdout.readline())['type'], 'ready')
            self.assertEqual(p.wait(timeout=28), 124)
        self.assertLess(time.monotonic()-started, 28)


if __name__ == '__main__':
    unittest.main()
