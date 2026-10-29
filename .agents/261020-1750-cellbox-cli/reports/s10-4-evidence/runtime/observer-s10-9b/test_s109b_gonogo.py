"""S10-9b offline tests: go/no-go composition, single run token, run order, finish/summary fields, census files, gate composition, retained
trees untouched, source hygiene. No VM, no QEMU, the real run token is never claimed."""
import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s109b_env as ENV  # noqa: E402
import s109b_preflight as PF  # noqa: E402
import s109b_run as RUN  # noqa: E402
import s109b_census as CENSUS  # noqa: E402
import s109b_gate as GATE  # noqa: E402
import s109_gate as G9  # noqa: E402
import s109_env as ENV9  # noqa: E402
import s107_env as E7  # noqa: E402


class GoNoGo(unittest.TestCase):
    GLOBAL = ('check_frozen_sources', 'check_frozen_s107', 'check_frozen_s108', 'check_frozen_s109', 'check_prior_archives', 'check_prior_run_evidence',
              'check_prior_run_evidence_s109', 'check_census', 'check_gate', 'check_collision_census', 'check_environment', 'check_no_other_qemu',
              'check_schedule')
    PER_SET = ('check_set_files', 'check_hooks_provenance', 'check_run_token', 'check_private_io', 'check_inputs')

    def patched(self, global_ok=None, set_ok=None):
        ok = {'ok': True, 'detail': {}}
        global_patches = {name: mock.Mock(return_value=dict(ok)) for name in self.GLOBAL}
        set_patches = {name: mock.Mock(return_value=dict(ok)) for name in self.PER_SET}
        return global_patches, set_patches

    def test_go_requires_every_global_and_every_set_check(self):
        ok = {'ok': True, 'detail': {}}
        global_patches, set_patches = self.patched()
        with mock.patch.multiple(PF, **global_patches, **set_patches), mock.patch.object(PF, 'check_qemu_identity', return_value=(dict(ok), 'h', 'b')), \
                mock.patch.object(PF, 'own_qemu_note', return_value={}):
            result, health, binding = PF.preflight()
            self.assertTrue(result['go'])
            self.assertEqual(result['schema'], 'cellos.s10-9b.go-no-go/1')
            self.assertEqual(set(result['go_by_set']), {'b2-desktop'})
            self.assertEqual((health, binding), ('h', 'b'))
            for name in self.GLOBAL + self.PER_SET:
                patch_ = {**global_patches, **set_patches}[name]
                patch_.return_value = {'ok': False, 'detail': {'why': name}}
                self.assertFalse(PF.preflight()[0]['go'], name)
                patch_.return_value = dict(ok)
        with mock.patch.multiple(PF, **global_patches, **set_patches), mock.patch.object(PF, 'check_qemu_identity', return_value=({'ok': False, 'detail': {}}, None, None)), \
                mock.patch.object(PF, 'own_qemu_note', return_value={}):
            self.assertFalse(PF.preflight()[0]['go'])

    def test_checks_the_real_state_would_report_for_the_inputs(self):
        for check in (PF.check_set_files, PF.check_hooks_provenance, PF.check_private_io, PF.check_inputs, PF.check_schedule):
            result = check()
            self.assertTrue(result['ok'], (check.__name__, result['detail']))

    def test_retained_frozen_sources_prior_archives_evidence_and_census(self):
        self.assertTrue(PF.check_frozen_sources()['ok'])
        self.assertEqual(PF.check_frozen_sources()['detail']['files'], 43)
        for check in (PF.check_frozen_s107, PF.check_frozen_s108, PF.check_frozen_s109, PF.check_prior_archives, PF.check_prior_run_evidence,
                      PF.check_prior_run_evidence_s109):
            self.assertTrue(check()['ok'], check.__name__)
        census = PF.check_census()
        self.assertTrue(census['ok'])
        self.assertEqual(census['detail']['paths'], 546)
        self.assertEqual(PF.check_frozen_s109()['detail']['files'], 97)

    def test_no_other_vm_may_run(self):
        with mock.patch.object(PF, 'qemu_pids', return_value=[4242]):
            result = PF.check_no_other_qemu()
        self.assertFalse(result['ok'])
        self.assertEqual(result['detail']['qemu_pids'], [4242])
        with mock.patch.object(PF, 'qemu_pids', return_value=[]):
            self.assertTrue(PF.check_no_other_qemu()['ok'])

    def test_a_consumed_token_or_existing_run_dir_is_a_no_go(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            token = root / 'RUN_CONSUMED-s109-b2-desktop'
            evidence = root / 'measurement-run'
            with mock.patch.object(ENV, 'token', lambda name=ENV.SET_NAME, root=None: token), mock.patch.object(ENV, 'RUN_EVIDENCE', evidence):
                self.assertTrue(PF.check_run_token()['ok'])
                evidence.mkdir()
                self.assertFalse(PF.check_run_token()['ok'])
                evidence.rmdir()
                token.write_text('x')
                self.assertFalse(PF.check_run_token()['ok'])

    def test_gate_check_requires_a_clean_single_invocation_and_unchanged_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            summary, frozen, source = Path(tmp) / 'summary.json', Path(tmp) / 'frozen.json', Path(tmp) / 'src.py'
            source.write_text('a')
            frozen.write_text(json.dumps({str(source): E7.sha256_file(source)}))
            clean = {'status': 'PASS', 'single_invocation': True, 'failures': 0, 'errors': 0, 'skipped': 0, 'expected_failures': 0,
                     'unexpected_successes': 0, 'tests_run': 1, 'components': {}}
            with mock.patch.object(PF, 'GATE_SUMMARY', summary), mock.patch.object(PF, 'NEW_FROZEN', frozen):
                summary.write_text(json.dumps(clean))
                self.assertTrue(PF.check_gate()['ok'])
                for key, value in (('status', 'FAIL'), ('single_invocation', False), ('failures', 1), ('errors', 1), ('skipped', 1),
                                   ('expected_failures', 1), ('unexpected_successes', 1)):
                    summary.write_text(json.dumps({**clean, key: value}))
                    self.assertFalse(PF.check_gate()['ok'], key)
                summary.write_text(json.dumps(clean))
                source.write_text('b')
                self.assertFalse(PF.check_gate()['ok'])
            with mock.patch.object(PF, 'GATE_SUMMARY', Path(tmp) / 'missing.json'):
                self.assertFalse(PF.check_gate()['ok'])

    def test_collision_census_is_empty_and_a_collision_is_caught(self):
        census = CENSUS.build_census()
        self.assertTrue(census['empty'], census['unintended_new'])
        self.assertEqual(sorted(census['new_classes']), ['s109b_b2d.B2DesktopMachine', 's109b_b2d.B2DesktopObserver', 's109b_b2d.B2DesktopSurface',
                                                         's109b_b2d.B2DesktopTargets'])
        import s109b_b2d as B2D

        class Colliding(B2D.B2DesktopObserver):
            def _stop(self, message):                       # the S10-7 defect: overrides threading.Thread._stop with another signature
                return message
        registry = {label: CENSUS.C8.mro_names(cls) for label, cls in CENSUS.base_classes().items()}
        item = CENSUS.census_class(Colliding, registry, 'Colliding')
        self.assertIn('_stop', {entry['name'] for entry in item['unintended']})


class RunOrder(unittest.TestCase):
    def fake_environment(self, tmp):
        root = Path(tmp)
        return mock.patch.multiple(ENV, WORK_ROOT=root / 'work', RUN_EVIDENCE=root / 'ev' / 'measurement-run', EVIDENCE=root / 'ev')

    def test_preflight_flag_never_claims_or_runs(self):
        outcome = {'go_by_set': {'b2-desktop': True}, 'checks': {}, 'sets': {'b2-desktop': {}}}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, None, None)), \
                mock.patch.object(ENV, 'claim_run', side_effect=AssertionError('claimed')), mock.patch.object(RUN, 'run_b2_desktop', side_effect=AssertionError('ran')), \
                mock.patch.object(RUN, 'HERE', Path(tmp)), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(RUN.main(['--preflight']), 0)
            self.assertTrue((Path(tmp) / 'go-no-go-preflight-b2-desktop.json').is_file())

    def test_no_go_stops_before_the_token(self):
        outcome = {'go_by_set': {'b2-desktop': False}, 'checks': {}, 'sets': {'b2-desktop': {}}}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, None, None)), \
                mock.patch.object(ENV, 'claim_run', side_effect=AssertionError('claimed')), mock.patch.object(RUN, 'run_b2_desktop', side_effect=AssertionError('ran')), \
                mock.patch.object(RUN, 'HERE', Path(tmp)), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(RUN.main(['--run']), 2)
            self.assertTrue((Path(tmp) / 'go-no-go-run-b2-desktop.json').is_file())

    def test_run_flag_claims_the_token_exactly_once_before_the_single_run(self):
        outcome = {'go_by_set': {'b2-desktop': True}, 'checks': {}, 'sets': {'b2-desktop': {}}}
        order = []
        summary = {'status': 'COMPLETE', 'classification': {'class': 'B'}, 'error': None, 'preservation': {'census_unchanged': True}}
        with tempfile.TemporaryDirectory() as tmp, self.fake_environment(tmp), mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, 'h', {'verified': True})), \
                mock.patch.object(RUN.PF, 'census_now', return_value=({}, [])), mock.patch.object(RUN.PF, 'PRIOR_CENSUS', Path(tmp) / 'census.json'), \
                mock.patch.object(ENV, 'claim_run', side_effect=lambda *a, **k: order.append('claim')), \
                mock.patch.object(RUN, 'run_b2_desktop', side_effect=lambda *a, **k: order.append('run') or {'status': 'COMPLETE'}), \
                mock.patch.object(RUN, 'finish', side_effect=lambda *a, **k: order.append('finish') or summary), \
                mock.patch.object(RUN, 'write_top_level', side_effect=lambda *a, **k: order.append('top')), \
                mock.patch.object(RUN, 'HERE', Path(tmp)), contextlib.redirect_stdout(io.StringIO()):
            (Path(tmp) / 'census.json').write_text(json.dumps(['/nonexistent/a']))
            self.assertEqual(RUN.main(['--run']), 0)
            self.assertEqual(order, ['claim', 'run', 'finish', 'top'])
            self.assertTrue((ENV.RUN_EVIDENCE / 'census-before.json').is_file())
            self.assertTrue((ENV.RUN_EVIDENCE / 'hashes-before.json').is_file())
            self.assertTrue(json.loads((ENV.RUN_EVIDENCE / 'hashes-before.json').read_text())['all_match'])

    def test_a_second_run_cannot_start_the_evidence_dir_exists(self):
        outcome = {'go_by_set': {'b2-desktop': True}, 'checks': {}, 'sets': {'b2-desktop': {}}}
        with tempfile.TemporaryDirectory() as tmp, self.fake_environment(tmp), mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, 'h', {'verified': True})), \
                mock.patch.object(RUN.PF, 'census_now', return_value=({}, [])), mock.patch.object(RUN.PF, 'PRIOR_CENSUS', Path(tmp) / 'census.json'), \
                mock.patch.object(ENV, 'claim_run'), mock.patch.object(RUN, 'run_b2_desktop', side_effect=AssertionError('ran')), \
                mock.patch.object(RUN, 'HERE', Path(tmp)), contextlib.redirect_stdout(io.StringIO()):
            (Path(tmp) / 'census.json').write_text(json.dumps([]))
            ENV.RUN_EVIDENCE.mkdir(parents=True)
            with self.assertRaises(FileExistsError):
                RUN.main(['--run'])

    def test_the_real_claim_is_exclusive(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'RUN_CONSUMED-s109-b2-desktop'
            ENV.claim_run(path=path)
            with self.assertRaises(FileExistsError):
                ENV.claim_run(path=path)

    def test_the_real_token_is_not_claimed_by_the_test_suite(self):
        import re
        pattern = re.compile(r'^\s*(?:[\w.]+\s*=\s*)?(?:ENV\.)?claim_run\(\)', re.M)      # a statement calling it without a target path would claim the REAL token
        source = (HERE / 'test_s109b_gonogo.py').read_text() + (HERE / 'test_s109b_wiring.py').read_text()
        self.assertEqual(pattern.findall(source), [])

    def test_summary_fields_classification_and_preservation(self):
        import test_s109b_wiring as TW9
        schedule = ENV.schedule()
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp) / 'measurement-run'
            destination.mkdir()
            TW9.Classification.table_dir(None, str(destination), ENV.schedule_snapshots(schedule))
            (destination / 'evidence' / 'memory-entry-01-terminal.json').write_text('{}')
            observed = Path(tmp) / 'observed.bin'
            observed.write_bytes(b'abc')
            paths = [str(observed)]
            before = {'census_paths': paths, 'census': RUN.PF.census_now(paths)}
            result = {'status': 'COMPLETE', 'timestamps': {'exact_prompt_ns': 1, 'desktop_ready_ns': 1}, 'argv': ['qemu'], 'start_ns': 0, 'end_ns': 3_000_000_000,
                      'ordinary_idle': {'samples': [{'changed_pixels': 0}] * 100, 'qemu_cpu_ticks_delta': 5, 'serial_bytes': 0},
                      'surface_events': [{'phase': p['name']} for p in schedule['phases']], 'command_results': [], 'observer_pause_ns_total': 7}
            with mock.patch.object(RUN.PF, 'NEW_FROZEN', Path(tmp) / 'missing-frozen.json'):
                summary = RUN.finish(ENV.SPEC, result, destination, before)
            self.assertEqual(summary['classification']['class'], 'B')
            self.assertEqual(summary['status'], 'COMPLETE')
            self.assertEqual(summary['focus_task'], 'terminal')
            self.assertEqual(summary['idle']['samples'], 100)
            self.assertEqual(summary['phases']['phases_not_entered'], [])
            self.assertTrue(summary['preservation']['census_unchanged'])
            self.assertTrue(summary['preservation']['kernel_iso_qcow2_hashes']['all_match'])
            self.assertFalse(summary['rerun_authorized'])
            self.assertFalse(summary['cbx27_met'])
            self.assertFalse(summary['phase10_criterion4_met'])
            self.assertIn('not measured', summary['comparator_notes']['allocator_peak'])
            for name in ('summary.json', 'census-after.json', 'hashes-after.json', 'evidence.sha256'):
                self.assertTrue((destination / name).is_file(), name)
            listed = {line.split('  ', 1)[1] for line in (destination / 'evidence.sha256').read_text().splitlines()}
            self.assertIn('summary.json', listed)
            self.assertNotIn('evidence.sha256', listed)
            self.assertEqual(RUN.PF.verify_manifest_normalized(destination)['ok'], True)

    def test_a_changed_protected_file_is_reported_as_census_difference(self):
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp) / 'measurement-run'
            (destination / 'evidence').mkdir(parents=True)
            observed = Path(tmp) / 'observed.bin'
            observed.write_bytes(b'abc')
            before = {'census_paths': [str(observed)], 'census': RUN.PF.census_now([str(observed)])}
            observed.write_bytes(b'abd')
            with mock.patch.object(RUN.PF, 'NEW_FROZEN', Path(tmp) / 'missing-frozen.json'):
                summary = RUN.finish(ENV.SPEC, {'status': 'STOP', 'error': 'x'}, destination, before)
            self.assertFalse(summary['preservation']['census_unchanged'])
            self.assertEqual(summary['classification']['class'], 'E')
            self.assertFalse(summary['preservation']['frozen_s109b_sources']['ok'])

    def test_top_level_copy_lists_every_file_below_the_evidence_root(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'b2-desktop-run'
            (root / 'harness').mkdir(parents=True)
            (root / 'harness' / 'a.txt').write_text('a')
            with mock.patch.object(ENV, 'EVIDENCE', root):
                RUN.write_top_level({'k': 1}, root / 'measurement-run')
            lines = (root / 'evidence.sha256').read_text().splitlines()
            self.assertEqual(sorted(line.split('  ', 1)[1] for line in lines), ['harness/a.txt', 'summary.json'])


class GateComposition(unittest.TestCase):
    def test_gate_composes_the_s10_9_gate_plus_the_new_modules(self):
        self.assertEqual(GATE.EXCLUDED_RETAINED, G9.EXCLUDED_RETAINED)
        self.assertEqual(GATE.S109B_MODULES, ['test_s109b_wiring', 'test_s109b_oracle', 'test_s109b_gonogo'])
        for name in GATE.S109B_MODULES:
            self.assertTrue((HERE / f'{name}.py').is_file(), name)
        self.assertEqual(GATE.EXPECT, {'retained_original': 144, 'retained_prior_current': 47, 'feed_hook_new': 78, 'memory': 35, 's10_7': 91, 's10_8': 85,
                                       's10_9': 142})
        self.assertEqual(G9.S109_MODULES, ['test_s109_credit', 'test_s109_wiring', 'test_s109_rsp_replay', 'test_s109_baseline', 'test_s109_census',
                                            'test_s109_gonogo'])

    def test_frozen_sources_cover_s10_9_and_the_new_executing_files(self):
        sources = GATE.frozen_sources()
        s109 = {str(p) for p in ENV9.HERE.glob('*.py')}
        own = {str(p) for p in HERE.glob('*.py')}
        self.assertTrue(s109 <= set(sources))
        self.assertTrue(own <= set(sources))
        self.assertIn(str(ENV.SCHEDULE), sources)
        self.assertTrue(any(path.endswith('schedule-b2.json') for path in sources))
        self.assertGreater(len(sources), 97)

    def test_gate_refuses_without_dont_write_bytecode(self):
        with mock.patch.dict('os.environ', {'PYTHONDONTWRITEBYTECODE': ''}), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(GATE.main(), 64)


class SourceHygiene(unittest.TestCase):
    def test_new_sources_never_touch_protected_paths_or_the_desktop_script(self):
        for path in sorted(HERE.glob('s109b_*.py')) + sorted(HERE.glob('run_*.py')):
            text = path.read_text()
            self.assertNotIn('kernel/src/main.rs', text, path.name)
            self.assertNotIn('run-desktop.sh', text, path.name)
            self.assertNotIn('git push', text, path.name)
            self.assertNotIn('git commit', text, path.name)

    def test_no_bytecode_in_any_runtime_archive_directory(self):
        for name in ('observer-s10-9b', 'observer-s10-9', 'observer-s10-8', 'observer-s10-7', 'observer-memory', 'observer-feed-hook'):
            self.assertEqual(list((E7.RUNTIME / name).rglob('__pycache__')), [], name)

    def test_the_new_directory_holds_only_new_files_and_no_retained_archive_is_inside_it(self):
        names = {p.name for p in HERE.iterdir()}
        self.assertEqual(HERE.name, 'observer-s10-9b')
        self.assertFalse({'s107_env.py', 's109_env.py', 'schedule-b2.json'} & names)

    def test_entry_point_flags(self):
        text = (HERE / 'run_b2_desktop.py').read_text()
        self.assertIn('s109b_run.main(sys.argv[1:])', text)
        doc = RUN.__doc__
        self.assertIn('--preflight | --run', doc)


if __name__ == '__main__':
    unittest.main()
