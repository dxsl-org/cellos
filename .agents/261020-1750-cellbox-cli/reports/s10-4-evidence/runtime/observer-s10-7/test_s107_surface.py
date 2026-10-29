"""S10-7.4 offline tests: status integration, original deadlines/order, one-extra-step policy untouched, B2 schedule subset. No VM."""
import inspect
import json
import re
import sys
import types
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402
import s107_machine as MACH  # noqa: E402
import s107_run as RUN  # noqa: E402
import common as COMMON  # noqa: E402
import status_select as SS  # noqa: E402

D = COMMON.D
RUNTIME = ENV.RUNTIME
COMMON_SRC = (RUNTIME / 'common.py').read_text()


class Deadlines(unittest.TestCase):
    def test_constants_equal_the_original_harness(self):
        self.assertEqual(inspect.signature(COMMON.Surface.command).parameters['timeout'].default, ENV.DEADLINES['command_s'])
        self.assertEqual(ENV.DEADLINES['command_s'], 120)
        status_calls = re.findall(r"self\.command\('status','echo st=\$\?',(\d+)\)", COMMON_SRC)
        self.assertEqual(status_calls, [str(ENV.DEADLINES['status_s'])])
        self.assertEqual(ENV.DEADLINES['status_s'], 60)
        self.assertIn(f"time.monotonic_ns()-self.m.start_ns<{ENV.DEADLINES['total_window_s']}_000_000_000", COMMON_SRC)
        self.assertEqual(ENV.DEADLINES['total_window_s'], 900)
        # ready() waits (900 s) for the prompt / desktop, exactly as the retained code
        self.assertEqual(len(re.findall(r',900,\'(?:exact initial idle prompt prefix|desktop ready)\'\)', COMMON_SRC)), 2)
        self.assertEqual(ENV.DEADLINES, {'command_s': 120, 'status_s': 60, 'total_window_s': 900})

    def test_retained_desktop_runner_uses_the_same_window(self):
        import run_hook
        self.assertIn("'original_window_seconds': 900", inspect.getsource(run_hook.run_desktop))
        self.assertIn("'original_window_seconds': ENV.DEADLINES['total_window_s']", inspect.getsource(RUN.run_desktop))

    def test_machine_window_and_status_timeouts_are_the_constants(self):
        source = inspect.getsource(MACH)
        self.assertIn("window=ENV.DEADLINES['total_window_s']", source)
        self.assertIn("ENV.DEADLINES['status_s']", source)
        self.assertNotRegex(source, r'timeout\s*=\s*\d')

    def test_status_cmd_constant_is_the_retained_command(self):
        import hook_ledger
        self.assertEqual(ENV.STATUS_CMD, SS.CMD)
        self.assertEqual(ENV.STATUS_CMD, hook_ledger.STATUS)
        self.assertIn("'echo st=$?'", COMMON_SRC)

    def test_new_sources_introduce_no_other_numeric_deadline(self):
        for name in ('s107_run.py', 's107_machine.py', 's107_b2.py', 's107_memory.py'):
            text = (HERE / name).read_text()
            for number in re.findall(r'(?:wait|timeout|deadline)\w*\s*[=(]\s*(\d+)', text):
                self.assertIn(int(number), (5,), f'{name}: unexpected deadline literal {number}')   # 5 s = retained RSP/QMP socket timeout


class ScheduleOrder(unittest.TestCase):
    def test_candidate_snapshot_order_is_the_retained_order(self):
        streams = re.findall(r"self\.snapshot\('([a-z-]+)'\)", inspect.getsource(COMMON.Surface.streams))
        run_src = (RUNTIME / 'run.py').read_text()
        order = re.findall(r"s\.ready\(\)|s\.sample_idle\('([a-z-]+)'\)|s\.streams\(\)|s\.top_idle\(\)|s\.snapshot\('([a-z-]+)'\)", run_src)
        idle = re.search(r"s\.sample_idle\('([a-z-]+)'\)", run_src).group(1)
        top_idle = re.search(r"self\.sample_idle\('([a-z-]+)'\)", inspect.getsource(COMMON.Surface.top_idle)).group(1)
        expected = ['initial-prompt', idle] + streams + [top_idle, 'complete']
        self.assertEqual(list(RUN.CANDIDATE_SNAPSHOTS), expected)
        self.assertTrue(order)

    def test_new_runners_call_the_schedule_in_the_original_order(self):
        import run_hook
        for function, label in ((RUN.run_desktop, 'run_desktop'), (RUN.run_headless, 'run_headless')):
            source = inspect.getsource(function)
            positions = [source.index(token) for token in ("surface.ready()", "surface.sample_idle('ordinary-prompt-idle')",
                                                            "surface.streams()", "surface.top_idle()", "surface.phase('complete')",
                                                            "surface.snapshot('complete')")]
            self.assertEqual(positions, sorted(positions), label)
        original = inspect.getsource(run_hook.run_desktop)
        positions = [original.index(token) for token in ("surface.ready()", "sample_idle('ordinary-prompt-idle')", "surface.streams()",
                                                         "surface.top_idle()", "surface.phase('complete')", "surface.snapshot('complete')")]
        self.assertEqual(positions, sorted(positions))

    def test_headless_candidate_matches_the_retained_headless_profile_run(self):
        # runtime/run.py: headless runs streams() (long_stream) then top_idle(); the new headless runner does both for candidates
        source = inspect.getsource(RUN.run_headless)
        self.assertIn("result['streams'] = surface.streams()", source)
        self.assertIn("result['top_idle'] = surface.top_idle()", source)


class OneExtraStepPolicy(unittest.TestCase):
    def test_new_code_does_not_touch_the_stepping_policy(self):
        for cls in (RUN.MemoryHookObserver, RUN.MemoryPlainObserver):
            for name in ('step_over_breakpoint', 'resume', '_stopped', 'run', 'add_bp', 'add_wp'):
                self.assertNotIn(name, vars(cls), f'{cls.__name__} overrides {name}')
                self.assertNotIn(name, vars(RUN.MEM.MemoryMixin))
        for name in ('s107_memory.py', 's107_machine.py', 's107_surface.py'):
            path = HERE / name
            if path.exists():
                self.assertNotRegex(path.read_text(), r'extra_used|max_additional|step_over_breakpoint|_additional_active')

    def test_runners_use_the_retained_clients(self):
        desktop, headless = inspect.getsource(RUN.run_desktop), inspect.getsource(RUN.run_headless)
        self.assertIn('RET.F.AtomicClient(observer, control, gate)', desktop)      # as run_hook.run_desktop
        self.assertIn('R.RecurringClient(observer, control)', headless)            # as recurring.run_profile
        self.assertIn('C.S.SerializedTraceControl(machine)', desktop)
        self.assertIn('A.AmendedTraceControl(machine)', headless)
        import run_hook
        self.assertIn('F.AtomicClient(observer, control, gate)', inspect.getsource(run_hook.run_desktop))

    def test_retained_recurrence_constants_are_unchanged(self):
        text = (RUNTIME / 'observer-recurring/recurring.py').read_text()
        self.assertIn("'max_additional_steps_per_event': 1", text)
        self.assertIn('terminal event forbids recurrence or a third step', text)


class StatusIntegration(unittest.TestCase):
    OLD = ['Cellos > echo st=$?', 'st=0', 'Cellos > top | cat', 'Cellos > echo st=$?', 'st=130']

    def test_desktop_history_with_st0_and_st130_selects_the_current_row(self):
        rows = ['Cellos > echo st=$?', 'st=0', 'Cellos > cmd', 'Cellos > echo st=$?', 'st=130', 'Cellos >']
        self.assertEqual(MACH.select_status_rows(rows), 130)
        self.assertEqual(MACH.select_status_rows(['Cellos > echo st=$?', 'st=0', 'Cellos > echo st=$?', 'st=130', 'Cellos >']), 130)

    def test_fail_closed_cases(self):
        cases = {'missing echo': ['st=0', 'Cellos >'], 'no status after echo': ['Cellos > echo st=$?', 'Cellos >'],
                 'duplicate after echo': ['Cellos > echo st=$?', 'st=0', 'st=130', 'Cellos >'],
                 'status only before echo': ['st=0', 'Cellos > echo st=$?', 'Cellos >']}
        for name, rows in cases.items():
            with self.subTest(name), self.assertRaises(COMMON.Stop):
                MACH.select_status_rows(rows)

    def test_retained_surface_status_would_have_failed_on_the_same_history(self):
        # the defect S10-7.3 fixes: the retained rule needs exactly ONE st= row on the whole screen
        out = ['Cellos > echo st=$?', 'st=0', 'Cellos > echo st=$?', 'st=130', 'Cellos >']
        retained = [line.strip() for line in out if re.fullmatch('st=[0-9]+', line.strip())]
        self.assertEqual(len(retained), 2)
        self.assertEqual(MACH.select_status_rows(out), 130)

    def test_desktop_mixin_is_in_the_surface_and_uses_the_original_command_and_deadline(self):
        desktop, headless = MACH.surface_classes(RUN.HS.HookSurface)
        self.assertIs(desktop.status, MACH.DesktopStatusMixin.status)
        self.assertIs(headless.status, MACH.HeadlessStatusMixin.status)
        self.assertIs(RUN.MeasuredDesktopSurface.__mro__[2], RUN.HS.HookSurface)

        class Probe(MACH.DesktopStatusMixin):
            calls = []

            def command(self, name, cmd, timeout=120):
                self.calls.append((name, cmd, timeout))
                return ['Cellos > echo st=$?', 'st=130', 'Cellos >']
        probe = Probe()
        self.assertEqual(probe.status(), 130)
        self.assertEqual(Probe.calls, [('status', 'echo st=$?', 60)])

    def test_interrupt_and_streams_call_the_replaced_status(self):
        for method in (COMMON.Surface.interrupt, COMMON.Surface.streams):
            self.assertIn('self.status()', inspect.getsource(method))

    def headless(self, raw_after):
        class Shell:
            def __init__(self):
                self.raw_bytes, self.timeouts = b'USER: Cellos > ', []

            def raw(self):
                return self.raw_bytes

            def submit(self, cmd):
                at = len(self.raw_bytes)
                self.raw_bytes += raw_after
                return 1, at

            def wait_prompt(self, count, timeout):
                self.timeouts.append(timeout)
                return True

        class Surf(MACH.HeadlessStatusMixin):
            def __init__(self):
                self.sh, self.events = Shell(), []

            def check(self):
                pass
        return Surf()

    def test_headless_mixin_selects_status_after_the_verified_echo(self):
        surf = self.headless(b'echo st=$?\r\nUSER: st=130\r\nUSER: Cellos > ')
        self.assertEqual(surf.status(), 130)
        self.assertEqual(surf.sh.timeouts, [60])
        self.assertEqual(surf.events[0]['rows'], ['echo st=$?', 'st=130'])

    def test_headless_mixin_fails_closed(self):
        for name, raw in {'duplicate': b'echo st=$?\r\nUSER: st=0\r\nUSER: st=1\r\nUSER: Cellos > ',
                          'missing': b'echo st=$?\r\nUSER: nothing\r\nUSER: Cellos > ',
                          'diverged echo': b'echo st=7\r\nUSER: st=0\r\nUSER: Cellos > '}.items():
            with self.subTest(name), self.assertRaises(COMMON.Stop):
                self.headless(raw).status()


class B2Schedule(unittest.TestCase):
    SCHEDULE = json.loads((HERE / 'schedule-b2.json').read_text())
    ORIGINAL = {'initial-prompt', 'ordinary-prompt-idle', 'finite-stream', 'endless-collected-cat', 'endless-terminal-stream', 'rg-large',
                'interactive-top', 'interactive-top-idle', 'complete'}

    def test_original_phase_names_are_the_retained_ones(self):
        names = set(re.findall(r"self\.phase\('([a-z-]+)'\)", COMMON_SRC)) | {'initial-prompt', 'ordinary-prompt-idle', 'complete'}
        names |= {'interactive-top-idle', 'ordinary-prompt-idle'}
        names.discard('terminal-launch')                                 # desktop-only GUI launch step, not a schedule phase
        self.assertEqual(names, self.ORIGINAL)

    def test_kept_and_dropped_partition_the_original_phases_in_order(self):
        kept = [p['original_phase'] for p in self.SCHEDULE['phases'] if p['original_phase']]
        dropped = [d['original_phase'] for d in self.SCHEDULE['dropped'] if d['original_phase'] in self.ORIGINAL]
        self.assertEqual(set(kept) | set(dropped), self.ORIGINAL)
        self.assertFalse(set(kept) & set(dropped))
        order = ['initial-prompt', 'ordinary-prompt-idle', 'finite-stream', 'endless-collected-cat', 'endless-terminal-stream', 'rg-large',
                 'interactive-top', 'interactive-top-idle', 'complete']
        self.assertEqual(kept, sorted(kept, key=order.index))
        for phase in self.SCHEDULE['phases']:
            self.assertTrue(phase['justification'])
        for item in self.SCHEDULE['dropped']:
            self.assertTrue(item['reason'])

    def test_deadlines_idle_and_prompt_are_the_originals(self):
        deadlines = {k: v for k, v in self.SCHEDULE['deadlines'].items() if k != 'note'}
        self.assertEqual(deadlines, ENV.DEADLINES)
        self.assertEqual(self.SCHEDULE['prompt'], D.PROMPT)
        idle = self.SCHEDULE['idle']
        default_seconds = inspect.signature(COMMON.Surface.sample_idle).parameters['seconds'].default
        self.assertEqual((idle['seconds'], idle['samples_expected']), (default_seconds, 100))
        self.assertIn('target+=.1', COMMON_SRC)                          # 10 Hz sampler
        self.assertEqual(self.SCHEDULE['status_command'], ENV.STATUS_CMD)

    def test_commands_are_the_shared_subset_only(self):
        allowed = {'echo', 'ls', 'cat', 'ps'}
        for phase in self.SCHEDULE['phases']:
            if phase['kind'] != 'command':
                continue
            command = phase['command']
            self.assertIn(command.split()[0], allowed, command)
            for forbidden in ('|', 'top', 'rg ', 'edit', 'ssh', ';', '&&', '>', '<', '$('):
                self.assertNotIn(forbidden, command, command)
            self.assertEqual(phase['expect_status'], 0)

    def test_phase_names_are_unique_and_kinds_valid(self):
        names = [p['name'] for p in self.SCHEDULE['phases']]
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual({p['kind'] for p in self.SCHEDULE['phases']}, {'ready', 'idle', 'command', 'snapshot'})
        self.assertEqual(names[0], 'initial-prompt')
        self.assertEqual(names[-1], 'complete')
        self.assertEqual(RUN.b2_snapshots(self.SCHEDULE), tuple(names))     # every kept phase snapshots

    def test_every_command_exists_in_the_b2_shell_and_the_cat_file_in_the_b2_image(self):
        executor = Path('/tmp/cellbox-s10-7/worktree/cells/tools/shell/src/executor.rs').read_text()
        for tool in ('ls', 'cat', 'echo', 'ps'):
            self.assertIn(f'"{tool}" => Some(with_legacy_parts', executor)
        self.assertIn('"top" =>', executor)                              # B2 top exists but is a different builtin TUI: dropped on purpose
        verification = json.loads((ENV.TH_KERNELS / 'b2-headless.static-verification.json').read_text())
        cat = next(p for p in self.SCHEDULE['phases'] if p['name'] == 'b2-cat')
        self.assertIn(cat['command'].split()[1], verification['image_file_sha256'])
        self.assertIn('[logging]', verification['image_system_toml'])
        self.assertEqual(cat['expect_row'], '[logging]')

    def test_b2_surface_runs_commands_then_status_and_records_mismatch_without_stopping(self):
        order = []

        class Surf(RUN.B2Surface):
            def __init__(self):
                self.events = []

            def phase(self, name):
                order.append(('phase', name))

            def command(self, name, cmd, timeout=120):
                order.append(('command', cmd, timeout))
                return ['[logging]'] if 'cat' in cmd else ['x']

            def status(self):
                order.append(('status',))
                return 1 if len([o for o in order if o[0] == 'status']) == 3 else 0

            def snapshot(self, name):
                order.append(('snapshot', name))
        results = Surf().shared_subset(self.SCHEDULE)
        self.assertEqual([r['command'] for r in results], ['echo b2-shared-subset', 'ls /', 'cat /etc/cellos/system.toml', 'ps'])
        self.assertEqual([r['status_ok'] for r in results], [True, True, False, True])
        self.assertEqual([r['row_seen'] for r in results], [False, None, True, None])
        self.assertTrue(all(o[2] == 120 for o in order if o[0] == 'command'))
        self.assertEqual([o[1] for o in order if o[0] == 'snapshot'], ['b2-echo', 'b2-ls', 'b2-cat', 'b2-ps'])

    def test_b2_sets_have_no_data_disk_and_no_cellbox_features(self):
        import s107_preflight as PF
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            argv = MACH.dry_argv(ENV.SETS['b2-headless'], tmp, observed=True, with_data_disk=False)
            self.assertFalse(any('p10fx' in a or 'nvme' in a for a in argv))
            self.assertEqual(PF.argv_private_violations(argv, tmp), [])
            with_disk = MACH.dry_argv(ENV.SETS['cand-headless'], tmp, observed=True)
            self.assertTrue(any('p10fx' in a for a in with_disk))
            self.assertEqual(len(with_disk) - len(argv), 4)


class Classification(unittest.TestCase):
    base = dict(status='COMPLETE', expected_snapshots=('a', 'b'), taken=['a', 'b'], entries_ok=True, ready=True, kind='candidate',
                snapshot_ok=True, snapshot_exact=True, allocator_measured=True)

    def klass(self, **kw):
        return RUN.classify_run(**{**self.base, **kw})['class']

    def test_classes(self):
        self.assertEqual(self.klass(), 'A')
        self.assertEqual(self.klass(snapshot_exact=False), 'B')
        self.assertEqual(self.klass(allocator_measured=False), 'B')
        self.assertEqual(self.klass(status='STOP', taken=['a']), 'C')
        self.assertEqual(self.klass(taken=['a']), 'C')
        self.assertEqual(self.klass(taken=[], entries_ok=False, status='STOP'), 'D')
        self.assertEqual(self.klass(ready=False, taken=[], status='STOP'), 'E')


if __name__ == '__main__':
    unittest.main()
