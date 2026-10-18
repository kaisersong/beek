"""Host-independent tests of the acceptance tooling, not Windows acceptance."""
import copy
import http.client
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / f'scripts/windows/{name}.py')
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


evidence = module('evidence')
trace = module('trace_check')
mock = module('mock_server')
metrics = module('metrics')


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / 'tools/windows-probe').mkdir(parents=True)
        (self.root / 'tools/windows-probe/Cargo.lock').write_text('test lock')
        (self.root / 'capture.png').write_bytes(b'public test screenshot fixture')
        (self.root / 'beek-windows-probe.exe').write_bytes(b'test binary')
        self.report = {
            'schema_version': 1, 'stage': 'G0', 'source_fingerprint': 'current',
            'catalog_sha256': evidence.digest(evidence.CASES),
            'environment': {'os': 'Windows', 'arch': 'X64', 'os_build': 26100,
                            'interactive': True, 'session_id': 1, 'rustc': '1.99.1',
                            'cargo': '1.99.1', 'msvc': 'test', 'windows_sdk': 'test', 'gpu': ['test'], 'monitors': ['test']},
            'artifact': {'path': 'beek-windows-probe.exe', 'sha256': evidence.digest(self.root / 'beek-windows-probe.exe')},
            'cases': [{'id': c['id'], 'status': 'pass', 'actual': 'public fixture checked',
                       'evidence': [{'path': 'capture.png', 'sha256': evidence.digest(self.root / 'capture.png')}]}
                      for c in evidence.load_cases() if c['stage'] == 'G0']}

    def tearDown(self):
        self.temp.cleanup()

    def check(self):
        with patch.object(evidence, 'ROOT', self.root):
            return evidence.validate(self.report, self.root, current='current')

    def test_complete_record(self):
        self.assertEqual(self.check(), [])

    def test_missing_case_cannot_pass(self):
        self.report['cases'].pop()
        self.assertTrue(any('missing required case' in e for e in self.check()))

    def test_not_run_and_blocked_cannot_pass(self):
        for status in ['not_run', 'blocked', 'fail']:
            self.report['cases'][0]['status'] = status
            self.assertTrue(self.check())

    def test_stale_source_or_evidence_cannot_pass(self):
        self.report['source_fingerprint'] = 'old'
        self.assertTrue(any('stale source' in e for e in self.check()))
        self.report['source_fingerprint'] = 'current'
        (self.root / 'capture.png').write_bytes(b'changed')
        self.assertTrue(any('changed evidence' in e for e in self.check()))

    def test_wrong_host_or_service_cannot_pass(self):
        for key, value in [('os', 'Darwin'), ('arch', 'ARM64'), ('os_build', 19045), ('interactive', False), ('session_id', 0)]:
            original = copy.deepcopy(self.report['environment'])
            self.report['environment'][key] = value
            self.assertTrue(self.check(), key)
            self.report['environment'] = original

    def test_escape_and_missing_actual_rejected(self):
        self.report['cases'][0]['evidence'][0]['path'] = '../outside.log'
        self.report['cases'][1]['actual'] = ''
        errors = self.check()
        self.assertTrue(any('escaping evidence' in e for e in errors))
        self.assertTrue(any('missing actual' in e for e in errors))

    def test_duplicate_and_missing_binary_rejected(self):
        self.report['cases'].append(copy.deepcopy(self.report['cases'][0]))
        self.report['artifact'] = None
        errors = self.check()
        self.assertTrue(any('duplicate' in e for e in errors))
        self.assertTrue(any('missing tested binary' in e for e in errors))

    def test_manual_case_needs_visual_evidence(self):
        (self.root / 'only.log').write_text('no screenshot')
        self.report['cases'][1]['evidence'] = [{'path': 'only.log', 'sha256': evidence.digest(self.root / 'only.log')}]
        self.assertTrue(any('requires screenshot' in e for e in self.check()))

    def test_product_gate_cannot_use_probe_binary(self):
        self.report['stage'] = 'G2'
        errors = self.check()
        self.assertTrue(any('wrong tested binary' in e for e in errors))
        self.assertTrue(any('selection helper' in e for e in errors))
        self.assertTrue(any('missing tested application version' in e for e in errors))

    def test_release_cannot_skip_stages_or_repeat_one(self):
        run = self.root / 'run.json'
        evidence.save(run, self.report)
        with patch.object(evidence, 'validate', return_value=[]):
            errors = evidence.release_gate([run, run])
        self.assertTrue(any('duplicate stage' in e for e in errors))
        self.assertTrue(any('missing required gate: G5' in e for e in errors))

    def test_record_pass_requires_actual_and_files(self):
        run = self.root / 'run.json'
        evidence.save(run, self.report)
        with self.assertRaises(ValueError):
            evidence.record_case(run, self.report['cases'][0]['id'], 'pass', '', [])

    def test_fingerprint_changes_on_source_not_build_output(self):
        folder = self.root / 'src'; folder.mkdir()
        path = folder / 'main.rs'; path.write_text('first')
        before = evidence.source_fingerprint(self.root)
        (self.root / 'tools/windows-probe/target').mkdir()
        (self.root / 'tools/windows-probe/target/generated.rs').write_text('ignored')
        self.assertEqual(before, evidence.source_fingerprint(self.root))
        path.write_text('second')
        self.assertNotEqual(before, evidence.source_fingerprint(self.root))

    def test_fingerprint_covers_resource_and_build_policy(self):
        for relative in ('build.rs', '.cargo/config.toml', 'resources/windows/beek.rc',
                         'resources/windows/BeeK.ico', '.github/workflows/release.yml'):
            before = evidence.source_fingerprint(self.root)
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'build input')
            self.assertNotEqual(before, evidence.source_fingerprint(self.root), relative)


class TraceTests(unittest.TestCase):
    def events(self):
        def event(name, data=None, ms=0):
            return {'event': name, 'data': data or {}, 'ms': ms}
        return [event('started', {'os': 'windows', 'arch': 'x86_64'}), event('tray_ready'),
                event('overlay_native', {'hwnd': 10, 'extended_style': 0x08000080}),
                event('overlay_shown', {'foreground_before': 20, 'foreground_after': 20})] + [
                event('overlay_clicked', {'foreground': 20}) for _ in range(10)] + [
                event('lifetime_host', {'visible': False}),
                event('heartbeat', {'visible_windows': 0, 'gpui_windows': 1, 'lifetime_hosts': 1}, 1000),
                event('heartbeat', {'visible_windows': 0, 'gpui_windows': 1, 'lifetime_hosts': 1}, 31000), event('quit')]

    def test_observable_invariants(self):
        self.assertEqual(trace.check(self.events()), [])

    def test_missing_focus_theft_and_no_tray_lifetime_rejected(self):
        self.assertTrue(trace.check([]))
        events = self.events()
        events[3]['data']['foreground_after'] = 10
        events[4]['data']['foreground'] = 10
        events[-2]['data']['visible_windows'] = 1
        errors = trace.check(events)
        for text in ['changed foreground', 'activated the popup', '30 seconds']:
            self.assertTrue(any(text in e for e in errors), text)


class MetricsTests(unittest.TestCase):
    def test_failures_remain_in_denominator_and_p95(self):
        rows = [{'app': 'edge', 'scenario': 'regular', 'outcome': 'success', 'latency_ms': '100', 'loading_ms': '20'} for _ in range(90)]
        rows += [{'app': 'edge', 'scenario': 'regular', 'outcome': 'timeout', 'latency_ms': '1500', 'loading_ms': ''} for _ in range(10)]
        result = metrics.summarize(rows)
        self.assertEqual(result['edge/regular']['attempts'], 100)
        self.assertEqual(result['edge/regular']['success_rate'], .9)
        self.assertEqual(result['edge/regular']['p95_all_attempts_ms'], 1500)
        self.assertTrue(metrics.acceptance_errors(result))

    def test_regular_gate_and_missing_measurements(self):
        rows = [{'app': app, 'scenario': 'regular', 'outcome': 'success', 'latency_ms': '200', 'loading_ms': '30'} for app in metrics.APPS for _ in range(100)]
        self.assertEqual(metrics.acceptance_errors(metrics.summarize(rows)), [])
        for row in rows:
            row['loading_ms'] = ''
        self.assertTrue(metrics.acceptance_errors(metrics.summarize(rows)))

    def test_invalid_or_unmeasured_latencies_rejected(self):
        for latency in ['NaN', '-1', 'inf', '']:
            with self.assertRaises(ValueError):
                metrics.summarize([{'app': 'edge', 'scenario': 'fault', 'outcome': 'timeout', 'latency_ms': latency}])


class MockServerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = mock.Server()
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown(); cls.server.server_close(); cls.thread.join(timeout=2)

    def request(self, path, method='POST', body=b'{}'):
        conn = http.client.HTTPConnection('127.0.0.1', self.server.server_port, timeout=5)
        conn.request(method, path, body=body if method == 'POST' else None)
        response = conn.getresponse()
        status, data = response.status, response.read()
        conn.close()
        return status, data

    def test_normal_and_fragmented_utf8(self):
        for path in ['/sse/normal', '/sse/fragmented']:
            code, data = self.request(path)
            self.assertEqual(code, 200)
            self.assertIn('😀', data.decode('utf-8'))
            self.assertIn(b'[DONE]', data)

    def test_abrupt_eof_and_limits(self):
        self.assertNotIn(b'[DONE]', self.request('/sse/disconnect')[1])
        self.assertGreater(len(self.request('/sse/oversized-frame')[1]), 65536)
        self.assertGreater(len(self.request('/sse/oversized-output')[1]), 256 * 1024)

    def test_http_statuses_stats_and_invalid_input(self):
        before = json.loads(self.request('/stats', 'GET')[1])['requests']
        for status in [401, 403, 429, 500]:
            self.assertEqual(self.request(f'/http/{status}')[0], status)
        self.assertEqual(json.loads(self.request('/stats', 'GET')[1])['requests'], before + 4)
        self.assertEqual(self.request('/sse/normal', body=b'invalid')[0], 400)
        self.assertEqual(self.request('/sse/normal', body=b'x' * (128 * 1024 + 1))[0], 413)


if __name__ == '__main__':
    unittest.main()
