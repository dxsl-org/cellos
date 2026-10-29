"""S10-9.2 offline tests: go/no-go checks, new run tokens (one run per set), private I/O / same argv as S10-7.4 and S10-8, census files, S10-9
summary fields, prior-run evidence integrity. No VM."""
import contextlib
import inspect
import io
import json
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_env as ENV  # noqa: E402
import s108_env as ENV8  # noqa: E402
import s107_env as E7  # noqa: E402
import s107_machine as MACH  # noqa: E402
import s107_run as RUN7  # noqa: E402
import s108_run as RUN8  # noqa: E402
import s109_preflight as PF  # noqa: E402
import s109_run as RUN  # noqa: E402
import s109_credit as CREDIT  # noqa: E402


class RunTokens(unittest.TestCase):
    def test_new_token_names_and_second_claim_refuses(self):
        self.assertEqual(ENV.SET_NAMES, ('cand-headless', 'cand-desktop'))
        tokens = {name: ENV.token(name) for name in ENV.SET_NAMES}
        self.assertEqual({n: p.name for n, p in tokens.items()},
                         {'cand-headless': 'RUN_CONSUMED-s109-cand-headless', 'cand-desktop': 'RUN_CONSUMED-s109-cand-desktop'})
        self.assertTrue(all(path.parent == HERE for path in tokens.values()))
        self.assertFalse({str(p) for p in tokens.values()} & {str(E7.SETS[n].token) for n in E7.SETS})
        with self.assertRaises(KeyError):
            ENV.token('b2-headless')
        with tempfile.TemporaryDirectory() as tmp:
            token = Path(tmp) / 'RUN_CONSUMED-x'
            self.assertFalse(ENV.run_consumed('cand-desktop', token))
            ENV.claim_run('cand-desktop', token)
            self.assertTrue(ENV.run_consumed('cand-desktop', token))
            with self.assertRaises(FileExistsError):
                ENV.claim_run('cand-desktop', token)
            self.assertEqual(oct(token.stat().st_mode & 0o777), '0o600')
            record = json.loads(token.read_text())
            self.assertEqual(record['set'], 'cand-desktop')
            self.assertIn('D9-4', record['meaning'])

    def test_claim_uses_o_excl(self):
        self.assertIn('os.O_EXCL', inspect.getsource(ENV.claim_run))

    def test_preflight_reports_a_consumed_token_or_evidence_dir_as_no_go(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(ENV, 'HERE', Path(tmp)), mock.patch.object(ENV, 'RUN_EVIDENCE', Path(tmp) / 'runs'):
            self.assertTrue(PF.check_run_token('cand-headless')['ok'])
            ENV.claim_run('cand-headless')
            self.assertFalse(PF.check_run_token('cand-headless')['ok'])
            self.assertTrue(PF.check_run_token('cand-desktop')['ok'])
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(ENV, 'HERE', Path(tmp)), mock.patch.object(ENV, 'RUN_EVIDENCE', Path(tmp)):
            (Path(tmp) / 'cand-desktop').mkdir()
            self.assertFalse(PF.check_run_token('cand-desktop')['ok'])

    def test_main_claims_before_any_vm_and_refuses_a_second_run(self):
        source = inspect.getsource(RUN.main)
        self.assertLess(source.index('ENV.claim_run(set_name)'), source.index('mkdir'))
        self.assertLess(source.index('ENV.claim_run(set_name)'), source.index('run_desktop('))
        self.assertLess(source.index('ENV.claim_run(set_name)'), source.index('run_headless('))
        self.assertLess(source.index('PF.preflight((set_name,))'), source.index('ENV.claim_run(set_name)'))
        self.assertNotRegex(source, r'while |retry|for attempt')
        outcome = {'go_by_set': {'cand-headless': True}, 'checks': {}, 'sets': {'cand-headless': {}}}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(RUN, 'HERE', Path(tmp)), \
                mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, None, None)), \
                mock.patch.object(RUN.PF, 'census_now', return_value=({}, [])), \
                mock.patch.object(RUN.PF, 'PRIOR_CENSUS', Path(tmp) / 'census.json'), \
                mock.patch.object(RUN.ENV, 'claim_run', side_effect=FileExistsError('consumed')), \
                mock.patch.object(RUN, 'run_headless') as headless, mock.patch.object(RUN, 'run_desktop') as desktop:
            (Path(tmp) / 'census.json').write_text('{}')
            with self.assertRaises(FileExistsError):
                RUN.main('cand-headless', ['--run'])
            headless.assert_not_called()
            desktop.assert_not_called()

    def test_main_refuses_on_no_go_without_claiming(self):
        outcome = {'go_by_set': {'cand-desktop': False}, 'checks': {}, 'sets': {'cand-desktop': {}}}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(RUN, 'HERE', Path(tmp)), \
                mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, None, None)), \
                mock.patch.object(RUN.ENV, 'claim_run') as claim:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(RUN.main('cand-desktop', ['--run']), 2)
            claim.assert_not_called()

    def test_without_flags_main_prints_usage_and_does_nothing(self):
        with mock.patch.object(RUN.PF, 'preflight') as preflight:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(RUN.main('cand-headless', []), 64)
            preflight.assert_not_called()

    def test_entry_scripts_map_one_to_one_to_the_sets(self):
        for script, name in (('run_cand_desktop.py', 'cand-desktop'), ('run_cand_headless.py', 'cand-headless')):
            text = (HERE / script).read_text()
            self.assertIn(f"s109_run.main('{name}', sys.argv[1:])", text)
            self.assertIn(f'RUN_CONSUMED-s109-{name}', text)
        self.assertEqual(sorted(p.name for p in HERE.glob('run_*.py')), ['run_cand_desktop.py', 'run_cand_headless.py'])
        self.assertEqual(set(ENV.NOT_RUN_SETS), {'b2-headless', 'b2-desktop'})
        self.assertFalse(set(ENV.NOT_RUN_SETS) & set(ENV.SETS))

    def test_run_dispatch_covers_each_set_once(self):
        source = inspect.getsource(RUN.main)
        self.assertIn("spec.name == 'cand-desktop'", source)
        self.assertEqual(source.count('run_headless(spec'), 1)
        self.assertEqual(source.count('run_desktop(spec'), 1)

    def test_s10_7_and_s10_8_evidence_and_tokens_are_never_written_by_s10_9(self):
        readers = set()
        for path in sorted(HERE.glob('*.py')):
            if path.name.startswith('test_'):
                continue
            if re.search(r's10-[78]-evidence', path.read_text()):
                readers.add(path.name)
        self.assertEqual(readers, {'s109_env.py', 's109_preflight.py', 's109_rsp_replay.py', 's109_stack_baseline.py'})   # env: docstring only; others read-only
        self.assertLess([i for i, line in enumerate((HERE / 's109_env.py').read_text().splitlines()) if 's10-7-evidence' in line][0], 10)
        readers.discard('s109_env.py')
        for name in readers:
            text = (HERE / name).read_text()
            for line in text.splitlines():
                if 'write_text(' in line or 'ENV.dump(' in line or '.mkdir(' in line:
                    self.assertTrue(any(ok in line for ok in ("HERE / 'go-no-go.json'", 'destination', 'ENV.dump(destination')), (name, line))
        self.assertEqual(ENV.RUN_EVIDENCE, E7.REPORTS / 's10-9-evidence' / 'measurement-runs')
        self.assertNotEqual(ENV.RUN_EVIDENCE, E7.RUN_EVIDENCE)
        self.assertNotEqual(ENV.RUN_EVIDENCE, ENV8.RUN_EVIDENCE)
        self.assertFalse({ENV.token(n) for n in ENV.SET_NAMES} & {ENV8.token(n) for n in ENV8.SET_NAMES})
        self.assertTrue(all(ENV8.run_consumed(n) for n in ENV8.SET_NAMES))            # the S10-8 tokens exist and stay


class SameAsS107(unittest.TestCase):
    """Schedule, classes, measurement kernels and QEMU argv are the S10-7.4 ones."""

    def test_classification_function_is_the_s10_7_one(self):
        self.assertIs(RUN.classify_run, RUN7.classify_run)
        self.assertIs(RUN.classify_run, RUN8.classify_run)
        self.assertIs(RUN.summarize_memory, RUN7.summarize_memory)
        self.assertIs(RUN.facts_from_table, RUN7.facts_from_table)
        self.assertEqual(RUN.CANDIDATE_SNAPSHOTS, RUN7.CANDIDATE_SNAPSHOTS)
        done = dict(status='COMPLETE', expected_snapshots=RUN.CANDIDATE_SNAPSHOTS, taken=list(RUN.CANDIDATE_SNAPSHOTS), entries_ok=True, ready=True,
                    kind='candidate', snapshot_ok=True, snapshot_exact=True, allocator_measured=True)
        self.assertEqual(RUN.classify_run(**done)['class'], 'A')
        self.assertEqual(RUN.classify_run(**{**done, 'snapshot_exact': False})['class'], 'B')
        self.assertEqual(RUN.classify_run(**{**done, 'status': 'STOP', 'taken': list(RUN.CANDIDATE_SNAPSHOTS)[:3]})['class'], 'C')
        self.assertEqual(RUN.classify_run(**{**done, 'status': 'STOP', 'taken': []})['class'], 'D')
        self.assertEqual(RUN.classify_run(**{**done, 'status': 'STOP', 'taken': [], 'ready': False})['class'], 'E')

    def test_machine_is_the_s10_7_measurement_machine(self):
        self.assertIs(RUN.MACH, MACH)
        self.assertIs(RUN.MACH.MeasurementMachine, MACH.MeasurementMachine)
        self.assertEqual(RUN.ENV.SETS['cand-headless'], E7.SETS['cand-headless'])
        self.assertEqual(RUN.ENV.SETS['cand-desktop'], E7.SETS['cand-desktop'])
        self.assertEqual(RUN.memory_context.__module__, 's107_run')

    def test_run_argv_equals_the_recorded_s10_7_4_and_s10_8_argv_of_each_set(self):
        for name in ENV.SET_NAMES:
            for label, root in (('S10-7.4', E7.RUN_EVIDENCE), ('S10-8', ENV8.RUN_EVIDENCE)):
                with self.subTest(name=name, recorded=label):
                    recorded = json.loads((root / name / 'runtime-result.json').read_text())['argv']
                    with tempfile.TemporaryDirectory() as scratch:
                        argv = MACH.dry_argv(E7.SETS[name], scratch, observed=True)
                        argv += ['-D', f'{scratch}/evidence/qemu-trace.log', '-msg', 'timestamp=on']
                        for event in RUN.X.BASE_EVENTS:
                            argv += ['-trace', f'enable={event}']
                        mine = [a.replace(scratch, '$WORK') for a in argv]
                    work = re.search(r'(/tmp/cellbox-s10-7/measure/[^ ]+?/run-[0-9a-z-]+/vm)/', ' '.join(recorded)).group(1)
                    theirs = [a.replace(work, '$WORK') for a in recorded]
                    self.assertEqual(mine, theirs)

    def test_private_io_of_both_sets(self):
        for name in ENV.SET_NAMES:
            result = PF.check_private_io(name)
            self.assertTrue(result['ok'], (name, result['detail']['violations']))
            self.assertEqual(result['detail']['sock_dir_mode'], '0o700')

    def test_set_files_provenance_and_inputs(self):
        for name in ENV.SET_NAMES:
            self.assertTrue(PF.check_set_files(name)['ok'], name)
            self.assertTrue(PF.check_hooks_provenance(name)['ok'], name)
            self.assertTrue(PF.check_inputs(name)['ok'], (name, PF.check_inputs(name)['detail']))


class Preservation(unittest.TestCase):
    def test_frozen_s10_8_sources_are_unchanged(self):
        result = PF.check_frozen_s108()
        self.assertTrue(result['ok'], result['detail'])
        frozen = json.loads(PF.S108_FROZEN.read_text())
        self.assertGreaterEqual(result['detail']['files'], 60)
        for name in ('s108_run.py', 's108_credit.py', 's108_memory.py', 's108_env.py', 's108_preflight.py', 's108_census.py', 's108_gate.py',
                     'test_s108_credit.py', 'test_s108_gonogo.py', 'test_s108_wiring.py', 'test_s108_census.py'):
            self.assertIn(str(E7.HERE.parent / 'observer-s10-8' / name), frozen)
        for name in ('s107_memory.py', 's107_run.py'):                                  # the S10-8 freeze also covers S10-7 and the retained stack
            self.assertIn(str(E7.HERE / name), frozen)
        self.assertTrue(any(key.endswith('observer-rsp-atomic/atomic_rsp.py') for key in frozen))

    def test_frozen_s10_7_sources_are_unchanged(self):
        result = PF.check_frozen_s107()
        self.assertTrue(result['ok'], result['detail'])
        self.assertGreaterEqual(result['detail']['files'], 40)
        frozen = json.loads(PF.S107_FROZEN.read_text())
        for name in ('s107_memory.py', 's107_run.py', 's107_machine.py', 's107_env.py', 's107_preflight.py'):
            self.assertIn(str(E7.HERE / name), frozen)

    def test_retained_frozen_sources_prior_archives_and_census(self):
        self.assertTrue(PF.check_frozen_sources()['ok'])
        self.assertEqual(PF.check_frozen_sources()['detail']['files'], 43)
        self.assertTrue(PF.check_prior_archives()['ok'])
        census = PF.check_census()
        self.assertTrue(census['ok'])
        self.assertEqual(census['detail']['paths'], 546)

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
                for key, value in (('status', 'FAIL'), ('single_invocation', False), ('failures', 1), ('errors', 1), ('skipped', 1)):
                    summary.write_text(json.dumps({**clean, key: value}))
                    self.assertFalse(PF.check_gate()['ok'], key)
                summary.write_text(json.dumps(clean))
                source.write_text('b')
                self.assertFalse(PF.check_gate()['ok'])
            with mock.patch.object(PF, 'GATE_SUMMARY', Path(tmp) / 'missing.json'):
                self.assertFalse(PF.check_gate()['ok'])

    def test_go_requires_every_check(self):
        names = ('check_frozen_sources', 'check_frozen_s107', 'check_frozen_s108', 'check_prior_archives', 'check_prior_run_evidence', 'check_census',
                 'check_gate', 'check_collision_census', 'check_environment')
        ok = {'ok': True, 'detail': {}}
        patches = {name: mock.Mock(return_value=dict(ok)) for name in names}
        group = {'ok': True, 'detail': {}}
        with mock.patch.multiple(PF, **patches), mock.patch.object(PF, 'check_qemu_identity', return_value=(dict(ok), 'h', 'b')), \
                mock.patch.object(PF, 'own_qemu_note', return_value={}), \
                mock.patch.multiple(PF, check_set_files=lambda n: dict(group), check_hooks_provenance=lambda n: dict(group),
                                    check_run_token=lambda n: dict(group), check_private_io=lambda n: dict(group), check_inputs=lambda n: dict(group)):
            result, health, binding = PF.preflight()
            self.assertTrue(result['go'])
            self.assertEqual(result['schema'], 'cellos.s10-9.go-no-go/1')
            self.assertEqual(set(result['go_by_set']), set(ENV.SET_NAMES))
            for name in names:
                patches[name].return_value = {'ok': False, 'detail': {'why': name}}
                self.assertFalse(PF.preflight()[0]['go'], name)
                patches[name].return_value = dict(ok)

    def test_source_hygiene(self):
        for path in sorted(HERE.glob('s109_*.py')) + sorted(HERE.glob('run_*.py')):
            text = path.read_text()
            self.assertNotIn('kernel/src/main.rs', text, path.name)
            self.assertNotIn('run-desktop.sh', text, path.name)
            for forbidden in ('write_memory', "request('M", 'request("M', 'Z0,', 'add_watchpoint', 'write_register', "request('P"):
                self.assertNotIn(forbidden, text, f'{path.name}: {forbidden}')
            for line in text.splitlines():
                if 'rmtree(' in line:
                    self.assertEqual(path.name, 's109_gate.py')
                    self.assertEqual(line.strip(), 'shutil.rmtree(top)')
                if '.unlink(' in line:
                    self.assertEqual(path.name, 's109_gate.py')
                    self.assertIn(line.strip(), ('link.unlink()', 'top.unlink()'))


class CensusFiles(unittest.TestCase):
    def test_before_and_after_census_files_are_saved_by_main_and_finish(self):
        main = inspect.getsource(RUN.main)
        self.assertIn("census_file(destination / 'census-before.json'", main)
        self.assertLess(main.index("census_file(destination / 'census-before.json'"), main.index('run_headless('))
        finish = inspect.getsource(RUN.finish)
        self.assertIn("census_file(destination / 'census-after.json'", finish)

    def test_census_file_round_trip_and_finish_summary(self):
        state = ({'/a': {'size': 1, 'sha256': 'x'}}, [])
        spec = ENV.SETS['cand-headless']
        credits = [{'label': 'new interrupt prompt', 'original_timeout_s': 10, 'pause_credited_ns': 5, 'effective_deadline_ns': 7, 'outcome': 'satisfied'}]
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp) / 'dest'
            (destination / 'evidence').mkdir(parents=True)
            RUN.census_file(destination / 'census-before.json', ['/a'], state)
            before = {'census_paths': ['/a'], 'census': state}
            ok = {'ok': True, 'detail': {}}
            result = {'status': 'STOP', 'error': 'Stop: timeout: new interrupt prompt', 'timestamps': {'exact_prompt_ns': 1}, 'recovery_waits': credits,
                      'observer_pause_ns_total': 99, 'rsp_client': 'AtomicHeadlessClient (AtomicReceiveMixin over RecurringClient)'}
            with mock.patch.object(RUN.PF, 'census_now', return_value=state), mock.patch.object(RUN.PF, 'check_frozen_sources', return_value=ok), \
                    mock.patch.object(RUN.PF, 'check_frozen_s107', return_value=ok), mock.patch.object(RUN.PF, 'check_frozen_s108', return_value=ok), \
                    mock.patch.object(RUN.PF, 'check_prior_archives', return_value=ok), \
                    mock.patch.object(RUN.E7, 'verify_set_files', return_value={'all_match': True}):
                summary = RUN.finish(spec, result, destination, before)
            self.assertEqual(summary['recovery_waits'], credits)
            self.assertEqual(summary['classification']['class'], 'D')
            self.assertTrue(summary['preservation']['census_unchanged'])
            self.assertEqual(json.loads((destination / 'census-after.json').read_text()), json.loads((destination / 'census-before.json').read_text()))
            self.assertEqual(summary['preservation']['kernel_set_hashes'], {'all_match': True})
            manifest = (destination / 'evidence.sha256').read_text()
            for name in ('summary.json', 'census-before.json', 'census-after.json'):
                self.assertIn(f'  {name}\n', manifest)
            self.assertFalse(summary['cbx27_met'] or summary['rerun_authorized'])

    def test_census_change_is_reported(self):
        spec = ENV.SETS['cand-desktop']
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp)
            (destination / 'evidence').mkdir()
            ok = {'ok': True, 'detail': {}}
            before = {'census_paths': ['/a'], 'census': ({'/a': {'size': 1, 'sha256': 'x'}}, [])}
            changed = ({'/a': {'size': 2, 'sha256': 'y'}}, [])
            with mock.patch.object(RUN.PF, 'census_now', return_value=changed), mock.patch.object(RUN.PF, 'check_frozen_sources', return_value=ok), \
                    mock.patch.object(RUN.PF, 'check_frozen_s107', return_value=ok), mock.patch.object(RUN.PF, 'check_frozen_s108', return_value=ok), \
                    mock.patch.object(RUN.PF, 'check_prior_archives', return_value=ok), \
                    mock.patch.object(RUN.E7, 'verify_set_files', return_value={'all_match': True}):
                summary = RUN.finish(spec, {'status': 'STOP', 'timestamps': {}}, destination, before)
            self.assertFalse(summary['preservation']['census_unchanged'])
            self.assertEqual(summary['classification']['class'], 'E')


class SummaryFields(unittest.TestCase):
    """The S10-9 summary: recovery waits recorded with the 10 s criterion `not_evaluated`, RSP client and wire-event counts, classes unchanged."""

    def finish(self, status, waits, evidence_rows=()):
        spec = ENV.SETS['cand-headless']
        ok = {'ok': True, 'detail': {}}
        state = ({'/a': {'size': 1, 'sha256': 'x'}}, [])
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp) / 'dest'
            (destination / 'evidence').mkdir(parents=True)
            (destination / 'evidence/rsp-traffic.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in evidence_rows))
            before = {'census_paths': ['/a'], 'census': state}
            result = {'status': status, 'error': None, 'timestamps': {'exact_prompt_ns': 1}, 'recovery_waits': waits, 'observer_pause_ns_total': 5,
                      'rsp_client': 'AtomicHeadlessClient (AtomicReceiveMixin over RecurringClient)'}
            with mock.patch.object(RUN.PF, 'census_now', return_value=state), mock.patch.object(RUN.PF, 'check_frozen_sources', return_value=ok), \
                    mock.patch.object(RUN.PF, 'check_frozen_s107', return_value=ok), mock.patch.object(RUN.PF, 'check_frozen_s108', return_value=ok), \
                    mock.patch.object(RUN.PF, 'check_prior_archives', return_value=ok), mock.patch.object(RUN.E7, 'verify_set_files', return_value={'all_match': True}):
                summary = RUN.finish(spec, result, destination, before)
            written = json.loads((destination / 'summary.json').read_text())
        return summary, written

    def test_summary_carries_the_recovery_records_policy_and_criterion(self):
        waits = [{'label': 'new interrupt prompt', 'outcome': 'satisfied', 'time_to_prompt_s': 41.2, 'pause_credited_ns': 5, 'effective_deadline_ns': 7,
                  'recovery_10s_criterion': 'not_evaluated'}]
        summary, written = self.finish('STOP', waits)
        self.assertEqual(summary, written)
        self.assertEqual(summary['recovery_waits'], waits)
        self.assertEqual(summary['recovery_10s_criterion'], 'not_evaluated')
        self.assertIn('NOT EVALUATED', summary['recovery_note'])
        self.assertEqual(summary['recovery_wait_policy']['effective_deadline'], 'min(start + 120 s + pause_in_wait, machine_start + 900 s)')
        self.assertEqual(summary['rsp_client'], 'AtomicHeadlessClient (AtomicReceiveMixin over RecurringClient)')
        self.assertFalse(summary['cbx27_met'] or summary['phase10_criterion4_met'] or summary['rerun_authorized'])
        self.assertNotIn('met', {k for k in summary['recovery_wait_policy'] if k.endswith('_met')})

    def test_summary_counts_the_atomic_wire_events(self):
        rows = [{'event': 'rx', 'raw_hex': '2b'}, {'event': 'wire_frame_pending', 'payload_hex': '00'}, {'event': 'tx', 'raw_hex': '2b'},
                {'event': 'packet_rx', 'payload_hex': '00'}, {'event': 'wire_terminal', 'error': 'x'}]
        summary, _ = self.finish('STOP', [], rows)
        wire = summary['rsp_wire_events']
        self.assertEqual((wire['wire_frame_pending'], wire['packet_rx'], wire['wire_terminal'], wire['tx_ack_plus'], wire['rows']), (1, 1, 1, 1, 5))

    def test_no_recovery_wait_is_still_not_evaluated(self):
        summary, _ = self.finish('STOP', [])
        self.assertEqual(summary['recovery_waits'], [])
        self.assertEqual(summary['recovery_10s_criterion'], 'not_evaluated')
        self.assertEqual(summary['classification']['class'], 'D')

    def test_runners_record_waits_and_the_criterion_in_runtime_result(self):
        for function in (RUN.run_headless, RUN.run_desktop):
            source = inspect.getsource(function)
            self.assertIn("recovery_waits=surface.__dict__.get('pause_credits', [])", source)
            self.assertIn("'recovery_10s_criterion': CREDIT.CRITERION", source)
            self.assertIn("'recovery_wait_policy': RECOVERY_WAIT_POLICY", source)
            self.assertIn('observer_pause_ns_total', source)


class PriorEvidenceIntegrity(unittest.TestCase):
    def test_real_s10_7_and_s10_8_manifests_verify(self):
        result = PF.check_prior_run_evidence()
        self.assertTrue(result['ok'], [r for r in result['detail']['results'] if not r['ok']])
        self.assertEqual(result['detail']['manifests'], 7)

    def test_verify_manifest_detects_modified_missing_and_unlisted_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'a.txt').write_text('a')
            (root / 'b.txt').write_text('b')
            (root / 'evidence.sha256').write_text(f"{E7.sha256_file(root / 'a.txt')}  a.txt\n{E7.sha256_file(root / 'b.txt')}  b.txt\n")
            self.assertTrue(PF.verify_manifest(root)['ok'])
            (root / 'a.txt').write_text('changed')
            self.assertEqual(PF.verify_manifest(root)['modified_or_missing'], ['a.txt'])
            (root / 'a.txt').write_text('a')
            (root / 'b.txt').unlink()
            self.assertEqual(PF.verify_manifest(root)['modified_or_missing'], ['b.txt'])
            (root / 'b.txt').write_text('b')
            (root / 'extra.txt').write_text('x')
            self.assertEqual(PF.verify_manifest(root)['unlisted'], ['extra.txt'])
            (root / 'evidence.sha256').unlink()
            self.assertFalse(PF.verify_manifest(root)['ok'])


class RunCommands(unittest.TestCase):
    def test_one_command_entries_and_docstring_commands(self):
        for script, name in (('run_cand_desktop.py', 'cand-desktop'), ('run_cand_headless.py', 'cand-headless')):
            text = (HERE / script).read_text()
            self.assertIn(f'PYTHONDONTWRITEBYTECODE=1 python3 -B {script} --preflight', text)
            self.assertIn(f'PYTHONDONTWRITEBYTECODE=1 python3 -B {script} --run', text)
            self.assertEqual(text.count('s109_run.main('), 1)
        self.assertEqual(ENV.token('cand-headless').name, 'RUN_CONSUMED-s109-cand-headless')
        self.assertEqual(ENV.token('cand-desktop').name, 'RUN_CONSUMED-s109-cand-desktop')

    def test_run_order_documented_headless_then_desktop(self):
        self.assertEqual(ENV.SET_NAMES, ('cand-headless', 'cand-desktop'))
        self.assertEqual(ENV.NOT_RUN_SETS['b2-headless'], 'class B COMPLETE in S10-7.4; not rerun (D9-2)')


if __name__ == '__main__':
    unittest.main()
