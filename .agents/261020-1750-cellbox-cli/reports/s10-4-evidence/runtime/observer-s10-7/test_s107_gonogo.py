"""S10-7.4 offline tests: go/no-go checks, run tokens (one run per set), private I/O, preservation. No VM."""
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
import s107_env as ENV  # noqa: E402
import s107_machine as MACH  # noqa: E402
import s107_preflight as PF  # noqa: E402
import s107_run as RUN  # noqa: E402


class RunTokens(unittest.TestCase):
    def test_each_set_has_its_own_token_and_second_claim_refuses(self):
        tokens = {name: ENV.SETS[name].token for name in ENV.SET_NAMES}
        self.assertEqual(len(set(tokens.values())), 3)
        self.assertTrue(all(path.parent == HERE and path.name == f'RUN_CONSUMED-{name}' for name, path in tokens.items()))
        with tempfile.TemporaryDirectory() as tmp:
            token = Path(tmp) / 'RUN_CONSUMED-x'
            self.assertFalse(ENV.run_consumed('cand-desktop', token))
            ENV.claim_run('cand-desktop', token)
            self.assertTrue(ENV.run_consumed('cand-desktop', token))
            with self.assertRaises(FileExistsError):
                ENV.claim_run('cand-desktop', token)
            self.assertEqual(oct(token.stat().st_mode & 0o777), '0o600')
            self.assertEqual(json.loads(token.read_text())['set'], 'cand-desktop')

    def test_preflight_reports_a_consumed_token_as_no_go(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(ENV, 'HERE', Path(tmp)), mock.patch.object(ENV, 'RUN_EVIDENCE', Path(tmp) / 'runs'):
            self.assertTrue(PF.check_run_token('b2-headless')['ok'])
            ENV.claim_run('b2-headless')
            self.assertFalse(PF.check_run_token('b2-headless')['ok'])
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(ENV, 'RUN_EVIDENCE', Path(tmp)):
            (Path(tmp) / 'cand-headless').mkdir()
            self.assertFalse(PF.check_run_token('cand-headless')['ok'])

    def test_main_claims_before_any_vm_and_refuses_a_second_run(self):
        source = inspect.getsource(RUN.main)
        self.assertLess(source.index('ENV.claim_run(set_name)'), source.index('mkdir'))
        self.assertLess(source.index('ENV.claim_run(set_name)'), source.index('run_desktop('))
        self.assertLess(source.index('ENV.claim_run(set_name)'), source.index('run_headless('))
        self.assertLess(source.index('PF.preflight((set_name,))'), source.index('ENV.claim_run(set_name)'))
        self.assertNotRegex(source, r'while |retry|for attempt')
        outcome = {'go_by_set': {'b2-headless': True}, 'checks': {}, 'sets': {'b2-headless': {}}}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(RUN, 'HERE', Path(tmp)), \
                mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, None, None)), \
                mock.patch.object(RUN.PF, 'census_now', return_value=({}, [])), \
                mock.patch.object(RUN.PF, 'PRIOR_CENSUS', Path(tmp) / 'census.json'), \
                mock.patch.object(RUN.ENV, 'claim_run', side_effect=FileExistsError('consumed')), \
                mock.patch.object(RUN, 'run_headless') as headless, mock.patch.object(RUN, 'run_desktop') as desktop:
            (Path(tmp) / 'census.json').write_text('{}')
            with self.assertRaises(FileExistsError):
                RUN.main('b2-headless', ['--run'])
            headless.assert_not_called()
            desktop.assert_not_called()

    def test_main_refuses_on_no_go_without_claiming(self):
        outcome = {'go_by_set': {'b2-headless': False}, 'checks': {}, 'sets': {'b2-headless': {}}}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(RUN, 'HERE', Path(tmp)), \
                mock.patch.object(RUN.PF, 'preflight', return_value=(outcome, None, None)), \
                mock.patch.object(RUN.ENV, 'claim_run') as claim:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(RUN.main('b2-headless', ['--run']), 2)
            claim.assert_not_called()

    def test_without_flags_main_prints_usage_and_does_nothing(self):
        with mock.patch.object(RUN.PF, 'preflight') as preflight:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(RUN.main('b2-headless', []), 64)
            preflight.assert_not_called()

    def test_entry_scripts_map_one_to_one_to_the_sets(self):
        for script, name in (('run_cand_desktop.py', 'cand-desktop'), ('run_cand_headless.py', 'cand-headless'), ('run_b2_headless.py', 'b2-headless')):
            text = (HERE / script).read_text()
            self.assertIn(f"s107_run.main('{name}', sys.argv[1:])", text)
            self.assertIn(f'RUN_CONSUMED-{name}', text)
        self.assertEqual({'b2-desktop'}, set(ENV.UNRUNNABLE_SETS))
        self.assertNotIn('b2-desktop', ENV.SETS)

    def test_run_dispatch_covers_each_set_once(self):
        source = inspect.getsource(RUN.main)
        self.assertIn("spec.name == 'cand-desktop'", source)
        self.assertEqual(source.count('run_headless(spec'), 1)


class PrivateIO(unittest.TestCase):
    GOOD = ['qemu-system-x86_64', '-machine', 'q35', '-display', 'none', '-chardev', 'socket,id=ser0,path=/w/sock/s.sock,server=on,wait=off',
            '-serial', 'chardev:ser0', '-qmp', 'unix:/w/sock/q.sock,server=on,wait=off', '-monitor', 'none', '-netdev', 'user,id=net0,restrict=on',
            '-device', 'e1000,netdev=net0', '-chardev', 'socket,path=/w/sock/gdb.sock,server=on,wait=off,id=gdb0', '-gdb', 'chardev:gdb0', '-S']

    def test_good_argv_has_no_violation(self):
        self.assertEqual(PF.argv_private_violations(self.GOOD, '/w'), [])

    def test_violations_are_detected(self):
        cases = {
            'hostfwd': self.GOOD[:-1] + ['-netdev', 'user,id=n,hostfwd=tcp::2222-:22'],
            'vnc': self.GOOD + ['-vnc', '127.0.0.1:37'],
            'tcp gdb': self.GOOD + ['-gdb', 'tcp::1234'],
            'nic': self.GOOD + ['-nic', 'user'],
            'smp': self.GOOD + ['-smp', '2'],
            'socket elsewhere': [a.replace('/w/sock/q.sock', '/tmp/q.sock') for a in self.GOOD],
            'display': [a if a != 'none' else 'gtk' for a in self.GOOD],
            'unrestricted netdev': [a.replace(',restrict=on', '') for a in self.GOOD],
        }
        for name, argv in cases.items():
            with self.subTest(name):
                self.assertTrue(PF.argv_private_violations(argv, '/w'), name)

    def test_real_argv_of_every_set_is_private(self):
        for name in ENV.SET_NAMES:
            result = PF.check_private_io(name)
            self.assertTrue(result['ok'], (name, result['detail']['violations']))
            self.assertEqual(result['detail']['sock_dir_mode'], '0o700')

    def test_machine_dirs_are_private_and_nothing_is_started(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / 'run'
            machine = MACH.MeasurementMachine(ENV.SETS['b2-headless'], work, observed=True, with_data_disk=False)
            self.assertFalse(machine.started)
            for sub in ('evidence', 'scratch', 'sock'):
                self.assertEqual(oct((work / sub).stat().st_mode & 0o777), '0o700')
            self.assertTrue(str(machine.gdb_path).startswith(str(work / 'sock')))
            self.assertEqual(PF.argv_private_violations(machine.argv, str(work)), [])
            provenance = json.loads((work / 'evidence/input-provenance.json').read_text())
            self.assertIn('test-hooks', provenance['kernel_provenance']['kernel_features'])
            self.assertEqual(provenance['kernel_provenance']['kernel_sha256'], ENV.expected_hashes('b2-headless')['kernel'])
            self.assertTrue(provenance['set_files']['all_match'])


class Preservation(unittest.TestCase):
    def test_frozen_sources_check_detects_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / 'file.py'
            target.write_text('a')
            record = Path(tmp) / 'frozen.json'
            record.write_text(json.dumps({str(target): ENV.sha256_file(target)}))
            self.assertTrue(PF.check_frozen_sources(record)['ok'])
            target.write_text('b')
            result = PF.check_frozen_sources(record)
            self.assertFalse(result['ok'])
            self.assertEqual(result['detail']['drift'], [str(target)])
            target.unlink()
            self.assertFalse(PF.check_frozen_sources(record)['ok'])

    def test_real_frozen_runtime_sources_are_unchanged(self):
        result = PF.check_frozen_sources()
        self.assertTrue(result['ok'], result['detail']['drift'])
        self.assertEqual(result['detail']['files'], 43)
        frozen = json.loads(PF.PRIOR_FROZEN.read_text())
        for name in ('common.py', 'observer.py', 'rsp.py'):
            self.assertIn(str(ENV.RUNTIME / name), frozen)
        self.assertIn(str(ENV.REPO / 'scripts/phase10_canonical_drive.py'), frozen)

    def test_prior_archive_check_detects_modification(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(ENV, 'RUNTIME', Path(tmp)):
            archive = Path(tmp) / 'observer-x'
            archive.mkdir()
            (archive / 'f').write_text('1')
            record = Path(tmp) / 'before.json'
            record.write_text(json.dumps({'observer-x': PF.tree_hashes(archive)}))
            self.assertTrue(PF.check_prior_archives(record)['ok'])
            (archive / 'g').write_text('new')
            self.assertFalse(PF.check_prior_archives(record)['ok'])

    def test_real_prior_archives_are_unchanged(self):
        result = PF.check_prior_archives()
        self.assertTrue(result['ok'], result['detail']['changed'])
        self.assertEqual(result['detail']['archives'], 25)

    def test_census_comparison_accepts_only_declared_differences(self):
        prior = {'/r/a': {'size': 1, 'sha256': 'a'}, '/r/b': {'size': 2, 'sha256': 'b'}, f'{ENV.CAND_RETAINED_ROOT}/c': {'size': 3, 'sha256': 'c'}}
        same = PF.compare_census(prior, dict(prior), [], {'promoted': {}})
        self.assertTrue(same['exact_unchanged'] and same['explained_only'])
        now = {'/r/a': {'size': 1, 'sha256': 'a'}, '/r/b': {'size': 9, 'sha256': 'new'}}
        with tempfile.TemporaryDirectory() as tmp:
            twin = Path(tmp) / 'c'
            twin.write_bytes(b'c')
            relocated = {**prior, f'{ENV.CAND_RETAINED_ROOT}/c': {'size': 1, 'sha256': ENV.sha256_file(twin)}}
            promotion = {'promoted': {'r/b': {'bytes': 9, 'sha256': 'new'}}}
            with mock.patch.object(ENV, 'REPO', Path('/')):
                got = PF.compare_census(relocated, now, [f'{ENV.CAND_RETAINED_ROOT}/c'], promotion, cache_root=Path(tmp))
            self.assertFalse(got['exact_unchanged'])
            self.assertTrue(got['explained_only'], got)
            self.assertEqual((got['promoted_expected'], got['relocated']), (['/r/b'], [f'{ENV.CAND_RETAINED_ROOT}/c']))
            bad = PF.compare_census(prior, {'/r/a': {'size': 1, 'sha256': 'x'}, '/r/b': prior['/r/b']}, [f'{ENV.CAND_RETAINED_ROOT}/c'],
                                    {'promoted': {}}, cache_root=Path(tmp))
            self.assertFalse(bad['explained_only'])
            self.assertEqual((bad['unexplained_changed'], bad['unexplained_missing']), (['/r/a'], [f'{ENV.CAND_RETAINED_ROOT}/c']))

    def test_real_protected_census_is_unchanged_modulo_declared_s10_5_promotion(self):
        result = PF.check_census()
        detail = result['detail']
        self.assertEqual(detail['paths'], 546)
        self.assertTrue(result['ok'], {k: detail[k] for k in ('unexplained_changed', 'unexplained_missing')})
        self.assertFalse(detail['unexplained_changed'] or detail['unexplained_missing'])
        # the user's uncommitted kernel/src/main.rs VMX edit is in the census and byte-identical to the S10-4.17 record
        prior = json.loads(PF.PRIOR_CENSUS.read_text())
        main_rs = str(ENV.REPO / 'kernel/src/main.rs')
        self.assertIn(main_rs, prior)
        self.assertEqual(ENV.sha256_file(main_rs), prior[main_rs]['sha256'])

    def test_census_method_equals_the_retained_protected_paths_method(self):
        source = inspect.getsource(RUN.X.protected_paths)
        self.assertIn("{'size': Path(name).stat().st_size, 'sha256': sha(name)}", source)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'f'
            path.write_bytes(b'abc')
            present, missing = PF.census_now([str(path), str(path) + 'x'])
            self.assertEqual(present, {str(path): {'size': 3, 'sha256': ENV.sha256_file(path)}})
            self.assertEqual(missing, [str(path) + 'x'])

    def test_harness_sources_only_delete_their_own_run_inputs(self):
        # deletions are confined to the disposable run-inputs/fixtures dirs and the private run disk clone
        for path in sorted(HERE.glob('s107_*.py')) + sorted(HERE.glob('run_*.py')):
            for line in path.read_text().splitlines():
                if 'rmtree(' in line:
                    self.assertIn(path.name, ('s107_prepare.py', 's107_gate.py'))
                    self.assertTrue("rmtree(disk_dir" in line or "base / 'fixtures'" in line or line.strip() == 'shutil.rmtree(top)', line)
                if '.unlink(' in line:
                    self.assertIn(path.name, ('s107_run.py', 's107_gate.py'))
                    self.assertTrue("machine.work / 'disk.img'" in line or line.strip() in ('link.unlink()', 'top.unlink()'), line)
        main_rs = 'kernel/src/main.rs'
        for path in sorted(HERE.glob('s107_*.py')) + sorted(HERE.glob('run_*.py')):
            self.assertNotIn(main_rs, path.read_text(), path.name)


class SetFiles(unittest.TestCase):
    def test_go_checks_pass_for_the_three_sets_except_the_gate_record(self):
        for name in ENV.SET_NAMES:
            self.assertTrue(PF.check_set_files(name)['ok'], name)
            self.assertTrue(PF.check_hooks_provenance(name)['ok'], name)
            self.assertTrue(PF.check_inputs(name)['ok'], (name, PF.check_inputs(name)['detail']))

    def test_hash_mismatch_is_a_no_go(self):
        original = ENV.verify_set_files

        def tampered(name):
            record = original(name)
            record['match']['kernel'], record['all_match'] = False, False
            return record
        with mock.patch.object(PF.ENV, 'verify_set_files', tampered):
            self.assertFalse(PF.check_set_files('cand-headless')['ok'])

    def test_unrunnable_set_is_recorded_not_silently_dropped(self):
        self.assertIn('b2-desktop', ENV.UNRUNNABLE_SETS)
        self.assertIn('panicked', ENV.UNRUNNABLE_SETS['b2-desktop'])

    def test_redirects_point_at_existing_paths_and_leave_retained_files_alone(self):
        import s107_retained as RET
        self.assertTrue((Path(RET.OBS.W) / 'observability/heap-instructions.json').is_file())
        self.assertEqual(ENV.sha256_file(Path(RET.OBS.W) / 'observability/heap-instructions.json'),
                         ENV.sha256_file(ENV.REPORTS / 's10-4-evidence/protocol/heap-instructions.json'))
        self.assertTrue(PF.check_frozen_sources()['ok'])
        record, files = ENV.candidate_files('desktop')
        self.assertTrue(all(Path(item['file']).is_file() for item in files.values()))

    def test_preflight_cli_blocks_without_a_gate_record(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(PF, 'GATE_SUMMARY', Path(tmp) / 'missing.json'):
            result = PF.check_gate()
            self.assertFalse(result['ok'])


if __name__ == '__main__':
    unittest.main()
