"""S10-9.0b offline tests: harvest of every `[stack-baseline]` serial line of the retained logs.  Read-only; no VM."""
import inspect
import json
import math
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
import s109_stack_baseline as SB  # noqa: E402
import s109_env as ENV  # noqa: E402

LINE = '[ WARN] [stack-baseline] name=cellbox phase=boot kind=user used_bytes=35352 used_pages=9 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input'


class Parsing(unittest.TestCase):
    def test_parse_the_emitted_format(self):
        record = SB.parse_line(LINE)
        self.assertEqual(record, {'level': 'WARN', 'name': 'cellbox', 'phase': 'boot', 'kind': 'user', 'used_bytes': 35352, 'used_pages': 9,
                                  'alloc_bytes': 73728, 'usable_bytes': 65536, 'baseline': 'authoritative-input'})

    def test_malformed_and_non_emission_lines_are_not_records(self):
        self.assertIsNone(SB.parse_line(LINE.replace('used_bytes=35352 ', '')))                 # missing field
        self.assertIsNone(SB.parse_line('USER: [stack-baseline] name=x'))
        self.assertIsNone(SB.parse_line(LINE.replace('35352', 'abc')))                          # non-integer value: malformed, never a silent 0

    def test_harvest_log_keeps_line_numbers_context_and_reports_malformed_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 's.log'
            path.write_text('boot\nUSER: Cellos > \nUSER: top --records\n' + LINE + '\n' + LINE.replace('user', 'kernel') + '\n'
                            '[ WARN] [stack-baseline] name=broken\nmention of [stack-baseline] in prose\n')
            result = SB.harvest_log(path)
        self.assertEqual(result['lines_total'], 7)
        self.assertEqual([n for n, _r, _b in result['records']], [4, 5])
        self.assertEqual(result['records'][0][2], ['boot', 'USER: Cellos > ', 'USER: top --records'])      # context only before the first line of a block
        self.assertEqual(result['records'][1][2], [])
        self.assertEqual([m['line'] for m in result['malformed']], [6])

    def test_key_format(self):
        self.assertEqual(SB.key_for('desktop', 'cand-desktop-test-hooks', 'a' * 64, 'cellbox', 'boot', 'user', 2),
                         'desktop/cand-desktop-test-hooks@aaaaaaaaaaaa/cellbox/boot/user#2')

    def test_duplicate_cell_lines_get_occurrence_numbers_per_log(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / 's.log'
            log.write_text(LINE.replace('35352', '15016') + '\n' + LINE + '\n')
            table = SB.build_table([{'source': 'x', 'run': 'x', 'set': 'cand-desktop', 'path': log, 'log': 'cooked'}])
        keys = sorted(table['entries'])
        self.assertEqual([k.rsplit('#', 1)[1] for k in keys], ['1', '2'])
        self.assertEqual([table['entries'][k]['used_bytes'] for k in keys], [15016, 35352])


class RetainedTable(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.table = SB.build_table()
        cls.entries = cls.table['entries']

    def entry(self, profile, build, cell, kind, occurrence=1):
        sha = SB.kernel_of(build)[2]
        return self.entries[SB.key_for(profile, f'{build}-test-hooks', sha, cell, 'boot', kind, occurrence)]

    def test_counts_and_sources(self):
        self.assertEqual(len(self.entries), 26)
        self.assertEqual(sum(len(e['observations']) for e in self.entries.values()), 72)
        present = [s for s in self.table['per_source'] if s['present']]
        self.assertEqual(len(self.table['per_source']), 5 * 2 + 4 + 1)
        self.assertEqual(len(present), 5 * 2 + 4 + 1)
        by_run = {}
        for source in self.table['per_source']:
            by_run.setdefault(source['run'], []).append(source['lines'])
        self.assertEqual(by_run['s10-7.4/cand-headless'], [8, 8])
        self.assertEqual(by_run['s10-7.4/cand-desktop'], [10, 10])
        self.assertEqual(by_run['s10-7.4/b2-headless'], [8, 8])
        self.assertEqual(by_run['s10-8/cand-desktop'], [10, 10])
        self.assertEqual(by_run['s10-8/cand-headless'], [0, 0])                                 # S10-8 headless never reached the sample tick
        for smoke in ('cand-headless', 'cand-desktop', 'b2-headless', 'b2-desktop'):
            self.assertEqual(by_run[f's10-7-boot-smoke/{smoke}'], [0])                           # boot smokes ended before the sample
        self.assertEqual(by_run['s10-7-b2-desktop-diagnostic'], [0])

    def test_every_source_has_no_malformed_baseline_line(self):
        for source in self.table['per_source']:
            self.assertEqual(source.get('malformed', []), [], source['source'])

    def test_known_values(self):
        self.assertEqual(self.entry('headless', 'cand-headless', 'cellbox', 'user')['used_bytes'], 45720)
        self.assertEqual(self.entry('headless', 'cand-headless', 'cellbox', 'kernel')['used_bytes'], 16072)
        self.assertEqual(self.entry('desktop', 'cand-desktop', 'cellbox', 'user', 1)['used_bytes'], 15016)
        self.assertEqual(self.entry('desktop', 'cand-desktop', 'cellbox', 'user', 2)['used_bytes'], 35352)
        self.assertEqual(self.entry('desktop', 'cand-desktop', 'vfs', 'user')['used_bytes'], 73432)
        self.assertEqual(self.entry('desktop', 'cand-desktop', 'vfs', 'user')['usable_bytes'], 262144)
        self.assertEqual(self.entry('headless', 'b2-headless', 'shell', 'user')['used_bytes'], 0)
        self.assertEqual(self.entry('headless', 'b2-headless', 'shell', 'kernel')['used_bytes'], 288)
        self.assertEqual(self.entry('headless', 'cand-headless', 'net', 'kernel')['used_bytes'], 14568)
        self.assertEqual(self.entry('desktop', 'cand-desktop', 'net', 'kernel')['used_bytes'], 14544)

    def test_all_values_agree_across_logs_and_runs(self):
        for key, entry in self.entries.items():
            self.assertTrue(entry['value_equal_across_all_logs'], key)
            self.assertIsNotNone(entry['used_bytes'], key)
            self.assertIsNotNone(entry['usable_bytes'], key)
        desktop = self.entry('desktop', 'cand-desktop', 'cellbox', 'user', 2)
        self.assertEqual(desktop['independent_runs'], ['s10-7.4/cand-desktop', 's10-8/cand-desktop'])

    def test_arithmetic_invariants_of_every_observation(self):
        for key, entry in self.entries.items():
            for item in entry['observations']:
                self.assertLessEqual(item['used_bytes'], item['usable_bytes'], key)
                self.assertEqual(item['used_pages'], math.ceil(item['used_bytes'] / 4096), key)
                self.assertEqual(item['alloc_bytes'], item['usable_bytes'] + 8192, key)           # usable + two guard pages
                self.assertEqual(item['baseline'], 'authoritative-input', key)
                self.assertEqual(item['level'], 'WARN', key)

    def test_every_entry_is_labelled_boot_phase_only_and_keyed_by_profile_kernel_cell_phase_kind(self):
        for key, entry in self.entries.items():
            self.assertEqual(entry['phase'], 'boot')
            self.assertIn('boot-phase only', entry['scope'])
            profile, kernel, cell, phase, kind_occurrence = key.split('/')
            self.assertEqual((profile, cell, phase), (entry['profile'], entry['cell'], 'boot'))
            self.assertEqual(kernel, f"{entry['kernel_build']}@{entry['kernel_sha256'][:12]}")
            self.assertEqual(kind_occurrence, f"{entry['kind']}#{entry['occurrence']}")
            self.assertIn(entry['kind'], ('kernel', 'user'))

    def test_kernel_hash_of_every_entry_is_the_measurement_kernel_manifest_hash(self):
        manifest = json.loads((SB.E7.TH_KERNELS / 'result.json').read_text())['sets']
        for entry in self.entries.values():
            set_name = entry['kernel_build'].replace('-test-hooks', '')
            self.assertEqual(entry['kernel_sha256'], manifest[set_name]['test_hooks_kernel_sha256'])
            self.assertEqual(entry['profile'], 'headless' if set_name.endswith('headless') else 'desktop')

    def test_cooked_and_raw_logs_carry_the_identical_block(self):
        agreement = SB.cooked_vs_raw(self.table['per_source'])
        self.assertEqual(len(agreement), 5)
        self.assertTrue(all(item['equal'] for item in agreement.values()))

    def test_cross_checks_against_the_memory_observer_depths(self):
        checks = SB.cross_checks(self.table)
        self.assertEqual(len(checks), 3)
        for check in checks:
            self.assertTrue(check['baseline_user_equals_one_of_the_measured_depths'], check['run'])
            self.assertTrue(check['baseline_kernel_equals_measured_kernel_depth'], check['run'])


class Completeness(unittest.TestCase):
    def test_scan_flags_an_unharvested_emission_file_and_ignores_prose_mentions(self):
        with tempfile.TemporaryDirectory() as tmp:
            reports = Path(tmp)
            (reports / 'harvested.log').write_text(LINE + '\n')
            (reports / 'extra.log').write_text('noise\n' + LINE + '\n')
            (reports / 'notes.json').write_text('{"markers": "[ WARN] [stack-baseline] name=x in prose"}\n')
            (reports / 'task.rs').write_text('// [stack-baseline] name=cellbox format\n')
            with mock.patch.object(SB.ENV, 'REPORTS', reports), mock.patch.object(SB, 'sources', return_value=[{'path': reports / 'harvested.log'}]):
                result = SB.completeness_scan()
        self.assertEqual(result['files_with_emission_lines'], ['extra.log', 'harvested.log'])
        self.assertEqual(result['files_not_harvested'], ['extra.log'])
        self.assertFalse(result['complete'])
        with tempfile.TemporaryDirectory() as tmp:
            reports = Path(tmp)
            (reports / 'harvested.log').write_text(LINE + '\n')
            with mock.patch.object(SB.ENV, 'REPORTS', reports), mock.patch.object(SB, 'sources', return_value=[{'path': reports / 'harvested.log'}]):
                self.assertTrue(SB.completeness_scan()['complete'])

    def test_grep_pattern_matches_only_line_start_emissions(self):
        import re
        pattern = re.compile(SB.EMIT.replace('^', '^', 1))
        self.assertTrue(pattern.search(LINE))
        self.assertFalse(pattern.search('x ' + LINE))
        self.assertFalse(pattern.search('"' + LINE))


class Hygiene(unittest.TestCase):
    def test_writes_only_the_new_evidence_directory_and_no_vm_machinery(self):
        source = (HERE / 's109_stack_baseline.py').read_text()
        self.assertEqual(ENV.STACK_BASELINE_EVIDENCE, ENV.REPORTS / 's10-9-evidence' / 's10-9.0b-stack-baseline')
        self.assertEqual(source.count('destination.mkdir('), 1)
        self.assertEqual(source.count('ENV.dump('), 1)
        for forbidden in ('Popen', 'qemu-system', 'os.kill', 'write_memory', 'run-desktop.sh', 'kernel/src/main.rs'):
            self.assertNotIn(forbidden, source, forbidden)
        self.assertIn("subprocess.run(['grep', '-rlaE'", source)
        self.assertIn('BOOT-PHASE ONLY', inspect.getsource(SB.build_result))


if __name__ == '__main__':
    unittest.main()
