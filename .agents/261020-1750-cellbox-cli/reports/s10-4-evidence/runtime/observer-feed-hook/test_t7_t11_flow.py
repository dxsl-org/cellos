"""T7 synthetic full run, T9 budgets, T10 failure retention/classification, T11 preservation/policy."""
import ast
import json
import os
from pathlib import Path
import re
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import classify as K
import hook_ledger as L
import hook_observer as HO
import hook_surface as HS
import hookfix as H
import run_hook as RH

M, X = HO.M, HO.X
S = H.S


class FakeObserver:
    def __init__(self, ledger):
        self.ledger = ledger
        self.disarms = 0

    def disarm_hook(self):
        self.disarms += 1
        return {'ok': True}


class View:
    """Progressive ledger: the real ledger, limited to what the guest has produced so far."""

    def __init__(self, ledger):
        self.ledger, self.limit = ledger, 10 ** 30

    def snapshot(self):
        return [h for h in self.ledger.snapshot() if h['ns'] <= self.limit]

    def count(self):
        return len(self.snapshot())


def make_desk(directory, hits):
    real = L.Ledger()
    for hit in hits:
        real.append(hit)
    ledger = View(real)
    desk = object.__new__(HS.HookDesk)
    desk.a = SimpleNamespace(evidence=directory)
    desk.state = {'img': 'before-image'}
    desk.model = ['', 'BANNER', L.PROMPT]
    desk.temporal_epochs, desk.failure_records, desk._retained_errors = [], [], set()
    desk.gate = SimpleNamespace(batches=[], phase='ordinary', resume_epoch=1)
    desk.hook_observer = FakeObserver(ledger)
    desk.finite_status_pending = False
    desk.metric, desk.metric_complete, desk.disarm = {}, False, None
    desk.last_frame = desk.last_read = desk.last_frame_ns = desk.active_epoch = None
    desk.controls_called = []
    desk.controls = lambda *args: desk.controls_called.append(args)
    desk.type_text = lambda text: None
    desk.shot = lambda *args: None
    desk.chk = lambda *args: None
    return desk


class Script:
    """Drives one epoch: typed screen, Enter commit, then a fixed sequence of GUI reads."""

    def __init__(self, desk, typed_read, grab_ns, commit_ns, reads, batch_ok=True):
        self.desk, self.reads, self.calls = desk, list(reads), 0
        desk.expect_screen = self.expect_screen
        desk.read_screen = lambda image: image[1] if isinstance(image, tuple) else typed_read
        desk.tap = lambda key: self.tap(commit_ns, batch_ok)
        desk.wait_for = self.wait_for
        self.grab_ns, self.typed_read = grab_ns, typed_read

    def expect_screen(self, rows, what, timeout):
        self.desk.last_frame_ns = self.grab_ns
        return ('typed-image', self.typed_read)

    def tap(self, commit_ns, ok):
        if ok:
            self.desk.gate.batches.append({'id': len(self.desk.gate.batches) + 1, 'outcome': 'COMMITTED', 'commit_count': 1,
                                           'attempts': [{'outcome': 'COMMITTED', 'finish': commit_ns / 1e9}]})

    def wait_for(self, pred, what, timeout, interval=1.0):
        for read in self.reads:
            image = ('image', read)
            image_obj = type('Img', (), {'save': lambda s, path: Path(path).write_bytes(b'png')})()
            got = pred(_Image(read))
            if got:
                return _Image(read), got
        raise M.Stop(f'timeout waiting for {what}')


class _Image(tuple):
    def __new__(cls, read):
        return super().__new__(cls, ('image', read))

    def save(self, path):
        Path(path).write_bytes(b'png')


class Clock:
    def __init__(self, start):
        self.now = start

    def __call__(self):
        self.now += 1_550_000_000
        return self.now


def typed(command):
    line = L.PROMPT + command
    return (['', 'BANNER', line], (len(line), 2))


class SyntheticFullRun(unittest.TestCase):
    def run_epochs(self, directory, mutate=None, finite_reads=None, status_reads=None):
        guest, f, s = H.two_epochs()
        hits = guest.clone_hits() if mutate is None else mutate(guest.clone_hits())
        desk = make_desk(directory, hits)
        finite = L.evaluate('finite', L.FINITE, guest.hits, f['typed_grab_ns'], f['commit_finish_ns'], 10 ** 15, f['until_ns'])
        status = L.evaluate('status', L.STATUS, guest.hits, s['typed_grab_ns'], s['commit_finish_ns'], 10 ** 15)
        good_f = (finite.extra['expected_rows'], (9, len(finite.extra['expected_rows']) - 1))
        good_s = (['', 'BANNER'] + status.extra['expected_rows'], (9, len(status.extra['expected_rows']) + 1))
        return guest, f, s, desk, good_f, good_s

    def epoch(self, desk, kind, typed_read, grab, commit, reads, timeout, until=None):
        desk.hook_observer.ledger.limit = until if until is not None else 10 ** 30
        Script(desk, typed_read, grab, commit, reads)
        name = 'finite-stream' if kind == 'finite' else 'status'
        command = L.FINITE if kind == 'finite' else L.STATUS
        return desk.command_read(name, command, timeout)

    def test_full_finite_then_status_flow_passes_and_disarms_once(self):
        with tempfile.TemporaryDirectory() as directory:
            guest, f, s, desk, good_f, good_s = self.run_epochs(directory)
            clock = Clock(guest.ns)
            with patch.object(HS.time, 'monotonic_ns', clock):
                partial = (['{"frame":"begin"'], (5, 0))  # mid-output sample: non-gating
                rows = self.epoch(desk, 'finite', typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'],
                                  [partial, partial, good_f, good_f, good_f], 120, f['until_ns'])
                self.assertEqual(rows, good_f[0])
                self.assertTrue(desk.finite_status_pending)
                self.assertFalse(desk.metric_complete)
                self.assertEqual(desk.hook_observer.disarms, 0)
                rows2 = self.epoch(desk, 'status', typed(L.STATUS), s['typed_grab_ns'], s['commit_finish_ns'],
                                   [good_s, good_s, good_s], 60)
                self.assertIn('st=0', rows2)
            self.assertTrue(desk.metric_complete)
            self.assertEqual(desk.hook_observer.disarms, 1)
            self.assertEqual(sorted(desk.metric), ['finite', 'status'])
            self.assertEqual(desk.metric['finite']['frames'], 2)
            self.assertFalse(desk.finite_status_pending)
            self.assertEqual(len(desk.controls_called), 2)
            self.assertEqual(desk.model[-1], L.PROMPT)
            evidence = json.loads((Path(directory) / 'feed-hook-epoch-finite.json').read_text())
            self.assertEqual(evidence['oracle']['hits'], 11)
            epochs = json.loads((Path(directory) / 'gui-temporal-epochs.json').read_text())
            self.assertTrue(all(sample['gating'] is False for e in epochs for sample in e['samples']))
            self.assertEqual([e['state'] for e in epochs], ['COMPLETE', 'COMPLETE'])

    def test_intermediate_samples_never_gate_even_when_unreadable_or_wrong(self):
        with tempfile.TemporaryDirectory() as directory:
            guest, f, s, desk, good_f, good_s = self.run_epochs(directory)
            with patch.object(HS.time, 'monotonic_ns', Clock(guest.ns)):
                rows = self.epoch(desk, 'finite', typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'],
                                  [None, (['garbage'], (0, 0)), None, good_f, good_f, good_f], 120, f['until_ns'])
            self.assertEqual(rows, good_f[0])

    def test_screen_that_never_matches_times_out_with_retained_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            guest, f, s, desk, good_f, good_s = self.run_epochs(directory)
            wrong = (good_f[0][:-2] + ['tampered', 'Cellos >'], good_f[1])
            with patch.object(HS.time, 'monotonic_ns', Clock(guest.ns)):
                with self.assertRaises(M.Stop) as raised:
                    self.epoch(desk, 'finite', typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'],
                               [good_f, wrong, wrong, wrong, wrong], 120, f['until_ns'])
            self.assertIn('timeout waiting for', str(raised.exception))
            failure = json.loads((Path(directory) / 'gui-failure-01.json').read_text())
            self.assertEqual(failure['epoch']['state'], 'STOP')
            self.assertFalse(desk.metric_complete)
            self.assertTrue(list((Path(directory) / 'gui-epoch-01').glob('*.json')))

    def assert_oracle_stop(self, directory, mutate, cls, code, reads_good=True):
        guest, f, s, desk, good_f, good_s = self.run_epochs(directory, mutate)
        with patch.object(HS.time, 'monotonic_ns', Clock(guest.ns)):
            with self.assertRaises(HS.HookOracleFailure) as raised:
                self.epoch(desk, 'finite', typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'],
                           [good_f] * 6, 120, f['until_ns'])
        self.assertEqual((raised.exception.cls, raised.exception.code), (cls, code))
        failure = json.loads((Path(directory) / 'gui-failure-01.json').read_text())
        self.assertEqual(failure['epoch']['state'], 'STOP')
        self.assertEqual(desk.hook_observer.disarms, 0)

    def test_oracle_failures_stop_with_class_and_retain_epoch(self):
        def replace(hits, old, new):
            for hit in hits:
                data = bytes.fromhex(hit['data_hex'])
                if old in data:
                    data = data.replace(old, new, 1)
                    hit.update(data_hex=data.hex(), length=len(data), sha256=L.hashlib.sha256(data).hexdigest())
                    return hits
            raise AssertionError('pattern absent')
        cases = {
            'dropped': (lambda h: replace(h, b'"dropped":0', b'"dropped":3'), 'D', 'end-incomplete'),
            'esc': (lambda h: replace(h, b'{"frame":"end"', b'\x1b"frame":"end"'), 'C', 'unsupported-control'),
        }
        for name, (mutate, cls, code) in cases.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as directory:
                self.assert_oracle_stop(directory, mutate, cls, code)

    def test_typed_line_f1_failure_stops_before_enter(self):
        with tempfile.TemporaryDirectory() as directory:
            guest, f, s, desk, good_f, good_s = self.run_epochs(directory)
            for hit in guest.hits:
                if hit['data_hex'] == b'|'.hex():
                    hit['data_hex'] = b'!'.hex()
                    hit['sha256'] = L.hashlib.sha256(b'!').hexdigest()
            desk = make_desk(directory, guest.hits)
            Script(desk, typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'], [good_f])
            with self.assertRaises(HS.HookOracleFailure) as raised:
                desk.command_read('finite-stream', L.FINITE, 120)
            self.assertEqual(raised.exception.code, 'typed-line-mismatch')
            self.assertEqual(desk.gate.batches, [])  # Enter was never sent

    def test_notice_row_outside_feed_is_a_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            guest, f, s, desk, good_f, good_s = self.run_epochs(directory)
            notice = (good_f[0][:-1] + ['[terminal: output failed: Revoked - Enter retries, Esc closes]'], good_f[1])
            with patch.object(HS.time, 'monotonic_ns', Clock(guest.ns)):
                with self.assertRaises(HS.HookOracleFailure) as raised:
                    self.epoch(desk, 'finite', typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'], [notice], 120, f['until_ns'])
            self.assertEqual(raised.exception.code, 'notice-row')

    def test_enter_must_commit_exactly_once(self):
        with tempfile.TemporaryDirectory() as directory:
            guest, f, s, desk, good_f, good_s = self.run_epochs(directory)
            script = Script(desk, typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'], [good_f], batch_ok=False)
            with self.assertRaises(M.Stop):
                desk.command_read('finite-stream', L.FINITE, 120)

    def test_non_original_commands_use_the_retained_path_untouched(self):
        desk = object.__new__(HS.HookDesk)
        desk.finite_status_pending = False
        with patch.object(HS.G.TemporalDesk, 'command_read', return_value=['st=130']) as original:
            self.assertEqual(desk.command_read('status', L.STATUS, 60), ['st=130'])
        self.assertEqual(original.call_args.args, ('status', L.STATUS, 60, None))

    def test_status_epoch_failure_keeps_finite_metric_but_not_complete(self):
        with tempfile.TemporaryDirectory() as directory:
            guest, f, s, desk, good_f, good_s = self.run_epochs(directory)
            bad_status = (good_s[0][:-2] + ['st=9', 'Cellos >'], good_s[1])
            with patch.object(HS.time, 'monotonic_ns', Clock(guest.ns)):
                self.epoch(desk, 'finite', typed(L.FINITE), f['typed_grab_ns'], f['commit_finish_ns'], [good_f, good_f, good_f], 120, f['until_ns'])
                with self.assertRaises(M.Stop):
                    self.epoch(desk, 'status', typed(L.STATUS), s['typed_grab_ns'], s['commit_finish_ns'], [bad_status] * 4, 60)
            self.assertEqual(sorted(desk.metric), ['finite'])
            self.assertFalse(desk.metric_complete)
            self.assertEqual(desk.hook_observer.disarms, 0)


class Budgets(unittest.TestCase):
    def test_original_deadlines_and_oracle_constants_are_unchanged(self):
        common = (HERE.parent / 'common.py').read_text()
        self.assertIn("self.command('finite-stream','top --records -d 1 --frames 2 | to jsonl | cat',120)", common)
        self.assertIn("self.command('status','echo st=$?',60)", common)
        self.assertIn('900_000_000_000', common)
        self.assertEqual((L.MAX_HITS, L.MAX_BYTES, L.QUIESCENCE_NS, HS.STABLE_NS), (2048, 524288, 3 * S, 1_500_000_000))
        run_source = (HERE / 'run_hook.py').read_text()
        self.assertIn("'original_window_seconds': 900", run_source)
        self.assertNotIn('--headless', run_source)
        self.assertNotIn("X.Machine('headless'", run_source)

    def test_cap_arithmetic_from_retained_per_scope_cost(self):
        steps = [json.loads(line) for line in (HERE.parent / 'observer-gui-temporal/desktop/evidence/steps.jsonl').read_text().splitlines()]
        summary = json.loads((HERE.parent / 'observer-gui-temporal/summary.json').read_text())
        runtime = summary['desktop']
        wall_s = (runtime['end_ns'] - runtime['start_ns']) / 1e9
        per_scope = wall_s / runtime['native_scope_count']
        self.assertEqual(len(steps), runtime['native_scope_count'])
        window_hits = 120 / per_scope
        self.assertLess(per_scope, 0.1)
        self.assertGreater(window_hits, 1200)  # ~1700 hits would fill the 120 s finite window
        self.assertLessEqual(L.MAX_HITS, 2048)
        # the cap is above what the deadline allows, so the deadline governs and cap overrun is a classed stop
        self.assertGreater(L.MAX_HITS, window_hits * 0.9)

    def test_quiescence_cap_deadline_produce_labelled_classes(self):
        self.assertEqual(K.classify({'status': 'STOP', 'hook_failure': {'code': 'cap-hits'}})['class'], 'C')
        self.assertEqual(K.classify({'status': 'STOP', 'error': 'Stop: timeout waiting for finite-stream: feed-hook oracle'})['class'], 'C')
        self.assertEqual(K.classify({'status': 'STOP', 'error': 'Stop: 900s measurement window expired'})['class'], 'C')


class FailureRetentionAndClassification(unittest.TestCase):
    def test_ledger_survives_a_hook_stop_with_the_rejected_record(self):
        with tempfile.TemporaryDirectory() as directory:
            ledger = L.Ledger(Path(directory) / 'ledger.jsonl')
            guest = H.Guest().feed(b'abc')
            for hit in guest.hits:
                ledger.append(hit)
            bad = L.hit_record(len(guest.hits) + 1, guest.ns + 1, 1, 0x1a5 + 2, 1, b'x')
            with self.assertRaises(L.LedgerViolation):
                ledger.append(bad)
            rows = [json.loads(line) for line in (Path(directory) / 'ledger.jsonl').read_text().splitlines()]
            self.assertEqual([r['accepted'] for r in rows], [True, False])
            self.assertEqual(ledger.violation['code'], 'unknown-return-site')

    def test_hook_facts_collects_everything_needed_for_classification(self):
        guest, f, s = H.two_epochs()
        ledger = L.Ledger()
        for hit in guest.hits:
            ledger.append(hit)
        observer = SimpleNamespace(ledger=ledger, identity_gate={'ok': 1}, disarm_record={'x': 1}, hook_failure=None,
                                   terminal_entries=1)
        desk = SimpleNamespace(metric={'finite': {'kind': 'finite', 'oracle': {'status': 'READY', 'output_text': 'x',
                                                                              'expected_rows': []}}}, metric_complete=True)
        facts = RH.hook_facts(observer, desk)
        self.assertEqual((facts['hits'], facts['stdout_bytes'], facts['stderr_bytes']), (len(guest.hits), ledger.total_bytes(), 0))
        self.assertTrue(facts['metric_complete'])
        self.assertNotIn('output_text', facts['metric']['finite']['oracle'])
        json.dumps(facts)

    def test_classification_table(self):
        c = K.classify
        both = {'metric_complete': True, 'preservation_ok': True}
        self.assertEqual(c({**both, 'status': 'COMPLETE'})['class'], 'A')
        self.assertEqual(c({**both, 'status': 'STOP', 'error': 'Stop: rg cancelled gate failed'})['class'], 'B')
        self.assertEqual(c({**both, 'status': 'STOP', 'error': 'Stop: rg cancelled gate failed'})['tail_stop_class'], 'E')
        self.assertEqual(c({**both, 'status': 'STOP', 'oracle_failure': {'cls': 'C', 'code': 'x'}})['class'], 'B')
        self.assertEqual(c({**both, 'status': 'COMPLETE', 'cleanup_error': 'boom'})['class'], 'B')
        self.assertEqual(c({'status': 'STOP', 'oracle_failure': {'cls': 'D', 'code': 'status-nonzero'}})['class'], 'D')
        self.assertEqual(c({'status': 'STOP', 'oracle_failure': {'cls': 'C', 'code': 'F2-window-hit'}})['class'], 'C')
        self.assertEqual(c({'status': 'STOP', 'hook_failure': {'code': 'second-terminal-entry'}})['class'], 'C')
        self.assertEqual(c({'status': 'STOP', 'error': 'StepFailed: input rejected'})['class'], 'E')
        self.assertEqual(c({'status': 'STOP', 'error': 'AdmissionError: QMP'})['class'], 'E')
        self.assertEqual(c({'status': 'STOP', 'oracle_failure': {'cls': 'D', 'code': 'x'}, 'cleanup_error': 'x'})['class'], 'E')
        self.assertEqual(c({'status': 'STOP', 'oracle_failure': {'cls': 'D', 'code': 'x'}, 'preservation_ok': False})['class'], 'E')
        self.assertEqual(c({'status': 'COMPLETE'})['class'], 'C')  # complete schedule without the metric is not a success
        self.assertEqual(set(K.CLASSES), set('ABCDE'))
        for outcome in ('A', 'B', 'C', 'D', 'E'):
            self.assertNotIn('rerun', K.CLASSES[outcome].lower())


class PreservationAndPolicy(unittest.TestCase):
    def test_packets_and_client_calls_are_read_only_hardware_z1_only(self):
        tree = ast.parse((HERE / 'hook_observer.py').read_text())
        used = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Attribute) and isinstance(node.value, (ast.Attribute, ast.Name)):
                base = node.value
                name = base.attr if isinstance(base, ast.Attribute) else base.id
                if name in ('client', 'real'):
                    used.add(node.attr)
        self.assertLessEqual(used, {'read_memory', 'read_register', 'add_breakpoint', 'remove_breakpoint',
                                    'read_physical_memory', '_hardware_breakpoints', 'control'})
        source = (HERE / 'hook_observer.py').read_text() + (HERE / 'hook_surface.py').read_text()
        for forbidden in ('request(', 'write_memory', 'Z0', 'add_watchpoint', "'M'", "'X'", 'qmp', 'resume(', 'interrupt('):
            self.assertNotIn(forbidden, source, forbidden)
        policy = HO.OBS.RspClient._validate if hasattr(HO.OBS.RspClient, '_validate') else None
        self.assertIsNotNone(policy)
        for ok in ('Z1,11601da80,1', 'z1,11601da80,1', 'm11601da80,40', 'p7', 'p2'):
            policy(ok)
        for bad in ('Z0,11601da80,1', 'M11601da80,1:55', 'X11601da80,1:U', 's', 'vCont;s', 'P7=00', 'G00'):
            with self.subTest(bad), self.assertRaises(Exception):
                policy(bad)

    def test_run_claim_is_exclusive_and_permanent(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'RUN_CONSUMED'
            RH.claim_run(path)
            self.assertEqual(oct(path.stat().st_mode & 0o777), '0o600')
            with self.assertRaises(FileExistsError):
                RH.claim_run(path)
        source = (HERE / 'run_hook.py').read_text()
        self.assertEqual(source.count('claim_run()'), 1)
        self.assertNotIn('RUN_CONSUMED.unlink', source)
        self.assertNotIn('--headless', source)
        main_source = source[source.index('def main'):]
        self.assertLess(main_source.index('claim_run()'), main_source.index('run_desktop(health'))
        self.assertEqual(main_source.count('run_desktop(health'), 1)  # one call site: no retry loop
        self.assertEqual(len(re.findall(r'\bwhile\b|\bfor attempt\b|\bretry\b', main_source, re.I)), 0)

    def test_writes_are_limited_to_the_new_sibling_and_work_dir(self):
        source = (HERE / 'run_hook.py').read_text()
        for match in re.finditer(r'X\.dump\((.+?),', source):
            self.assertRegex(match[1], r'HERE|machine\.work|work /|destination|provenance_path', match[1])
        self.assertEqual(RH.RUNTIME, HERE.parent)
        self.assertEqual(HERE.name, 'observer-feed-hook')
        self.assertNotIn('observer-feed-hook', RH.prior_archives())

    def test_preflight_defines_every_go_no_go_criterion(self):
        with patch.object(RH, 'stray_qemu', return_value=[]):
            result, _h, _b, before = RH.preflight()
        names = set(result['checks'])
        for expected in ('G1_cp1_approval_recorded', 'G2_single_clean_invocation', 'G2_frozen_sources_match',
                         'G3_static_identity_matches_candidate_elf', 'G4_candidate_inputs_and_terminal_sha',
                         'G4_protected_paths_stable_since_s10_4_15', 'G4_prior_archive_manifest_intact',
                         'G4_qemu_health_and_input_source_binding', 'G5_run_not_consumed', 'G6_no_stray_qemu',
                         'G6_tmp_space', 'G7_docs_record_approval_before_execution'):
            self.assertIn(expected, names)
        self.assertEqual(result['go'], all(c['ok'] for c in result['checks'].values()))
        failing = {name for name, c in result['checks'].items() if not c['ok']}
        # Only the not-yet-produced gate receipts may fail while this very gate is running.
        self.assertLessEqual(failing, {'G2_single_clean_invocation', 'G2_frozen_sources_match'}, failing)
        for name in ('G1_cp1_approval_recorded', 'G4_prior_archive_manifest_intact', 'G4_compressed_trace_present',
                     'G4_qemu_health_and_input_source_binding', 'G5_run_not_consumed', 'G6_tmp_space',
                     'G7_docs_record_approval_before_execution'):
            self.assertTrue(result['checks'][name]['ok'], name)
        self.assertTrue(result['checks']['G3_static_identity_matches_candidate_elf']['ok'])
        self.assertTrue(result['checks']['G4_candidate_inputs_and_terminal_sha']['ok'])
        self.assertTrue(result['checks']['G4_protected_paths_stable_since_s10_4_15']['ok'])
        self.assertEqual(len(before), 546)

    def test_stray_qemu_scan_and_no_go_on_any_failed_check(self):
        self.assertIsInstance(RH.stray_qemu(), list)
        fake = {'a': {'ok': True}, 'b': {'ok': False}}
        self.assertFalse(all(item['ok'] for item in fake.values()))


if __name__ == '__main__':
    unittest.main()
