"""S10-9b offline tests: B2 desktop environment/pins/token, schedule vs the B2 terminal source, task identification, coordinated memory
observer (entry/snapshot/MRO/thread lifecycle), measurement machine argv, exact-screen surface sequence, classification and reporting. No VM."""
import json
import struct
import sys
import tempfile
import threading
import types
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s109b_env as ENV  # noqa: E402
import s109b_b2d as B2D  # noqa: E402
import s109b_run as RUN  # noqa: E402
import s109b_preflight as PF  # noqa: E402
import s107_retained as RET  # noqa: E402
import s107_memory as MEM  # noqa: E402
import s107_b2 as B2  # noqa: E402
import test_s107_wiring as TW  # noqa: E402  (retained S10-7 fixtures: fake guest with real page tables, fake read-only client)
import test_memory as TM  # noqa: E402

PAGE = 4096
import common as COMMON  # noqa: E402
X, C, D = COMMON, RET.C, COMMON.D
TERMINAL_BASE = 0x1_1000_0000          # a 256 MiB desktop slot (multiple of the 32 MiB entry stride)
SHELL_BASE = 0x1_2000_0000


def fake_machine(work):
    (work / 'evidence').mkdir(exist_ok=True)
    (work / 'sock').mkdir(exist_ok=True)
    return types.SimpleNamespace(work=work, gdb_path=work / 'sock/gdb.sock', a=types.SimpleNamespace(profile='desktop'))


class FakeGate:
    """The two InputGate members the observer layer touches."""

    def __init__(self):
        self.events = []
        self.terminal_reason = None

    def paused(self):
        gate = self

        class Scope:
            def __enter__(self):
                gate.events.append('paused-enter')

            def __exit__(self, *exc):
                gate.events.append('paused-exit')
                return False
        return Scope()

    def terminate(self, reason):
        self.events.append(('terminate', reason))
        self.terminal_reason = reason


class EnvAndPins(unittest.TestCase):
    def test_pins_equal_manifest_json_and_result_json(self):
        recorded = ENV.manifest_hashes()
        self.assertEqual({k: recorded[k] for k in ENV.PINNED}, ENV.PINNED)
        self.assertEqual(ENV.sha256_file(ENV.IMAGE_ROOT / 'manifest.json'), ENV.PINNED_MANIFEST)
        result = json.loads((ENV.V2_EVIDENCE / 'result.json').read_text())['boot_sets']['test-hooks']['sha256']
        self.assertEqual(result['kernel'], ENV.PINNED['kernel'])
        self.assertEqual(result['iso'], ENV.PINNED['iso'])
        self.assertEqual(result['qcow2'], ENV.PINNED['qcow2'])
        self.assertEqual(result['kernel_fs'], ENV.PINNED['kernel_fs_img'])
        self.assertEqual(result['init'], ENV.PINNED['init'])
        control = json.loads((ENV.V2_EVIDENCE / 'result.json').read_text())['boot_sets']['non-hooks']['sha256']['kernel']
        self.assertEqual(control, ENV.PINNED_CONTROL_KERNEL)

    def test_set_files_match_the_pins(self):
        record = ENV.verify_set_files()
        self.assertTrue(record['all_match'], record['match'])
        self.assertTrue(record['pins_equal_manifest_json'])
        self.assertEqual(set(record['match']), {'kernel', 'iso', 'qcow2', 'kernel_fs_img', 'init', 'control_kernel'})

    def test_a_changed_file_is_detected(self):
        with tempfile.TemporaryDirectory() as tmp:
            iso = Path(tmp) / 'x.iso'
            iso.write_bytes(b'not the iso')
            with mock.patch.object(ENV.B2DesktopSpec, 'iso', iso):
                record = ENV.verify_set_files()
        self.assertFalse(record['all_match'])
        self.assertFalse(record['match']['iso'])

    def test_provenance_requires_and_records_the_hooks_kernel(self):
        provenance = ENV.provenance()
        self.assertIn('test-hooks', provenance['kernel_features'])
        self.assertEqual(provenance['kernel_sha256'], ENV.PINNED['kernel'])
        self.assertTrue(provenance['measurement_kernel_is_not_the_promoted_kernel'])
        self.assertTrue(ENV.kernel_has_test_hooks(ENV.SPEC.kernel))
        self.assertFalse(ENV.kernel_has_test_hooks(ENV.SPEC.control_kernel))
        MEM.MemoryContext(provenance=provenance, kernel_path=ENV.SPEC.kernel, heap_arena_rel=None, user_pages=(16, 64), kernel_pages=(16, 64))
        with self.assertRaises(ValueError):
            MEM.MemoryContext(provenance=dict(provenance, kernel_features=['default']), kernel_path=ENV.SPEC.kernel, heap_arena_rel=None,
                              user_pages=(16,), kernel_pages=(16,))
        with self.assertRaises(ValueError):
            MEM.MemoryContext(provenance=provenance, kernel_path=ENV.SPEC.control_kernel, heap_arena_rel=None, user_pages=(16,), kernel_pages=(16,))

    def test_stack_pages_are_the_s10_7_2_values_for_b2_shell_and_terminal(self):
        self.assertEqual(ENV.SPEC.user_stack_pages, (16, 64))
        result = json.loads((ENV.REPORTS / 's10-7-evidence/test-hooks-kernels/result.json').read_text())
        text = json.dumps(result)
        self.assertIn('terminal', text)
        self.assertIn('STACK_PAGES=64', text)

    def test_token_name_and_single_claim(self):
        self.assertEqual(ENV.token().name, 'RUN_CONSUMED-s109-b2-desktop')
        self.assertEqual(ENV.token().parent, HERE)
        with self.assertRaises(KeyError):
            ENV.token('cand-desktop')
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / 'RUN_CONSUMED-x'
            self.assertFalse(ENV.run_consumed(path=target))
            ENV.claim_run(path=target)
            self.assertTrue(ENV.run_consumed(path=target))
            with self.assertRaises(FileExistsError):
                ENV.claim_run(path=target)
            self.assertEqual(oct(target.stat().st_mode & 0o777), '0o600')
            self.assertIn('never rerun', json.loads(target.read_text())['meaning'])

    def test_no_collision_with_any_earlier_run_token(self):
        import s107_env as E7
        import s108_env as E8
        import s109_env as E9
        earlier = {str(E7.SETS[n].token) for n in E7.SETS} | {str(E8.token(n)) for n in E8.SET_NAMES} | {str(E9.token(n)) for n in E9.SET_NAMES}
        self.assertNotIn(str(ENV.token()), earlier)
        self.assertTrue(all(Path(p).exists() for p in earlier if 's109' in p))

    def test_original_deadlines_unchanged(self):
        import s107_env as E7
        self.assertEqual(ENV.DEADLINES, E7.DEADLINES)
        self.assertEqual(ENV.DEADLINES, {'command_s': 120, 'status_s': 60, 'total_window_s': 900})
        schedule = ENV.schedule()
        self.assertEqual(schedule['deadlines']['command_s'], 120)
        self.assertEqual(schedule['deadlines']['total_window_s'], 900)


class Schedule(unittest.TestCase):
    def test_phase_order_and_snapshots(self):
        schedule = ENV.schedule()
        self.assertEqual([p['name'] for p in schedule['phases']],
                         ['desktop-ready', 'terminal-launch', 'initial-prompt', 'ordinary-prompt-idle', 'b2d-echo', 'b2d-ls', 'b2d-cat', 'b2d-ps', 'complete'])
        self.assertEqual(ENV.schedule_snapshots(schedule), ('initial-prompt', 'ordinary-prompt-idle', 'b2d-echo', 'b2d-ls', 'b2d-cat', 'b2d-ps', 'complete'))
        idle = next(p for p in schedule['phases'] if p['kind'] == 'idle')
        self.assertEqual((idle['seconds'], idle['samples_expected']), (10, 100))
        self.assertEqual([p['command'] for p in schedule['phases'] if p['kind'] == 'command'],
                         ['echo b2-shared-subset', 'ls /', 'cat /etc/cellos/system.toml', 'ps'])
        self.assertTrue(all(p.get('justification') for p in schedule['phases']))

    def test_shared_subset_is_the_b2_headless_subset_in_the_same_order(self):
        import s107_run as RUN7
        headless = RUN7.b2_schedule()
        order = [p['command'] for p in headless['phases'] if p['kind'] == 'command']
        desktop = [p['command'] for p in ENV.schedule()['phases'] if p['kind'] == 'command']
        self.assertEqual(order, desktop)
        self.assertEqual([p['name'] for p in headless['phases'] if p.get('snapshot')][:2], ['initial-prompt', 'ordinary-prompt-idle'])
        self.assertEqual(headless['phases'][-1]['name'], ENV.schedule()['phases'][-1]['name'])

    def test_dropped_phases_name_every_cellbox_only_phase(self):
        dropped = json.dumps(ENV.schedule()['dropped'])
        for needle in ('finite-stream', 'endless-collected-cat', 'endless-terminal-stream', 'rg-large', 'interactive-top', 'Ctrl-C', 'ssh', 'status epoch'):
            self.assertIn(needle, dropped)
        self.assertFalse(ENV.schedule()['status']['applicable'])

    def test_expected_rows_equal_the_b2_terminal_source_literals(self):
        result = PF.check_schedule()
        self.assertTrue(result['ok'], result['detail'])
        self.assertTrue(all(result['detail']['commands'].values()))

    def test_source_literal_parser_on_synthetic_source(self):
        text = '''
    pub fn new() { grid.write_str("=== B ===\\nline2\\n\\nCellos > "); }
    pub fn execute_command(&mut self, cmd: &str) {
        match parts[0] {
            "ls" => {
                self.grid.write_str("a/  b/\\n");
            }
            "ps" => {
                self.grid.write_str("PID\\n");
                self.grid.write_str(" 1  x\\n");
            }
            "echo" => {
                self.grid.write_char('\\n');
            }
            other => {
                let msg = format!("shell: command not found: {}\\n", other);
            }
        }
    }'''
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'session.rs'
            path.write_text(text)
            literals = PF.source_literals(path)
        self.assertEqual(literals['banner'], '=== B ===\nline2\n\nCellos > ')
        self.assertEqual(literals['tables']['ls'], ['a/  b/'])
        self.assertEqual(literals['tables']['ps'], ['PID', ' 1  x'])
        self.assertEqual(literals['not_found_prefix'], 'shell: command not found: ')
        self.assertEqual(PF.expected_command_rows(literals, 'cat /x'), ['shell: command not found: cat'])
        self.assertEqual(PF.expected_command_rows(literals, 'echo a  b'), ['a b'])

    def test_schedule_mismatch_with_the_source_is_a_no_go(self):
        schedule = ENV.schedule()
        for phase in schedule['phases']:
            if phase['name'] == 'b2d-ls':
                phase['expect_rows'] = ['bin/  apps/']
        with mock.patch.object(ENV, 'schedule', lambda path=None: schedule):
            self.assertFalse(PF.check_schedule()['ok'])

    def test_status_is_not_derivable_in_the_b2_terminal_and_select_status_fails_closed(self):
        rows = ['Cellos > echo st=$?', 'st=$?', 'Cellos > ']
        with self.assertRaises(MEM.StatusError):
            MEM.select_status(rows)


class Targets(unittest.TestCase):
    def test_terminal_and_shell_are_distinct_hash_pinned_and_relocation_free(self):
        targets = B2D.B2DesktopTargets()
        self.assertEqual([t['name'] for t in targets.targets], ['terminal', 'shell'])
        table = ENV.static_verification()['image']['files']
        for target in targets.targets:
            self.assertEqual(len(target['windows']), 3)
            self.assertTrue(all(len(w) == 48 for _va, w in target['windows']))
            self.assertEqual(target['sha256'], table[target['image_path']]['sha256'])
        arenas = {t['name']: t['heap_arena'] for t in targets.targets}
        self.assertEqual(arenas['terminal'][1], 4 * 1024 * 1024)
        self.assertEqual(arenas['shell'][1], 1024 * 1024)

    def test_match_by_three_windows_and_tolerates_other_cells(self):
        targets = B2D.B2DesktopTargets()
        for target in targets.targets:
            memory = {TERMINAL_BASE + va: w for va, w in target['windows']}
            read = lambda addr, length, memory=memory: memory.get(addr, b'\0' * length)  # noqa: E731
            self.assertEqual(targets.match(read, TERMINAL_BASE)['name'], target['name'])
            memory[TERMINAL_BASE + target['windows'][2][0]] = b'\xff' * 48
            self.assertIsNone(targets.match(read, TERMINAL_BASE))

    def test_tampered_signed_binary_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            for name in ('terminal', 'app-shell'):
                (Path(tmp) / name).write_bytes((ENV.SPEC.signed / name).read_bytes())
            data = bytearray((Path(tmp) / 'terminal').read_bytes())
            data[-1] ^= 0xFF
            (Path(tmp) / 'terminal').write_bytes(bytes(data))
            with self.assertRaises(MEM.DiscoveryError):
                B2D.B2DesktopTargets(signed_dir=tmp)

    def test_heap_arena_kinds(self):
        rel, size = B2D.heap_arena_symbol(ENV.SPEC.signed / 'terminal', kind='b2-terminal')
        self.assertEqual(size, 4 * 1024 * 1024)
        rel_shell, size_shell = B2D.heap_arena_symbol(ENV.SPEC.signed / 'app-shell', kind='b2-shell')
        self.assertEqual(size_shell, 1024 * 1024)
        with self.assertRaises(MEM.DiscoveryError):
            B2D.heap_arena_symbol(ENV.SPEC.signed / 'app-shell', kind='b2-terminal')      # no such symbol: fail closed
        with self.assertRaises(MEM.DiscoveryError):
            B2D.heap_arena_symbol(ENV.SPEC.signed / 'terminal', kind='cellbox')


def add_task(guest, *, user_top, kernel_top, pc, pages):
    """A second task in the same fake guest (own stacks, same page tables): same seeding as the retained `build`."""
    guest.map_identity_range(user_top - pages * PAGE, user_top, fill=TM.PATTERN)
    guest.map_identity_range(kernel_top - pages * PAGE, kernel_top, fill=TM.PATTERN)
    guest.write(user_top - 0x400, b'\x00' * 0x400)
    guest.write(kernel_top - 0x300, b'\x00' * 0x300)
    guest.write(kernel_top - 288 + 264, struct.pack('<Q', pc))
    guest.write(kernel_top - 288 + 16, struct.pack('<Q', user_top))
    guest.write(kernel_top - 288 + 216, struct.pack('<5Q', pc, 0x23, 0x202, user_top - 0x3F0, 0x1B))


def observer_for(tmp, guest, targets=None, gate=None):
    work = Path(tmp)
    machine = fake_machine(work)
    ctx = TW.context()
    ctx.provenance = ENV.provenance()
    ctx.user_pages = ctx.kernel_pages = (16, 64)
    observer = B2D.B2DesktopObserver(machine, gate or FakeGate(), targets or B2D.B2DesktopTargets(), ctx)
    observer.client = TW.FakeClient(guest)
    return observer


def enter(observer, target_name, base, rsp):
    target = next(t for t in observer.targets.targets if t['name'] == target_name)
    windows = {base + va: w for va, w in target['windows']}
    guest = observer.client.guest
    observer.client.read_memory = lambda addr, length: windows.get(addr, b'\0' * length)
    observer.client.read_register = lambda name: {'cs': 0x23, 'cr3': guest.cr3, 'rsp': rsp}[name]
    observer._b2_entry(base)


def shrink_loads(observer, base):
    for target in observer.targets.targets:
        target['loads'] = [{'va': 0, 'memsz': 2 * PAGE, 'filesz': PAGE, 'flags': 5}]
        target['heap_arena'] = (0x1000, PAGE)


class DesktopObserver(unittest.TestCase):
    def two_task_guest(self):
        guest = TW.build(user_pages=64, kernel_pages=64, frame_pc=TERMINAL_BASE)
        guest.map_identity_range(TERMINAL_BASE, TERMINAL_BASE + 4 * PAGE, fill=0)
        guest.map_identity_range(SHELL_BASE, SHELL_BASE + 4 * PAGE, fill=0)
        return guest

    def test_mro_reuses_retained_behaviour_unchanged(self):
        O = B2D.B2DesktopObserver
        self.assertEqual([c.__name__ for c in O.__mro__[:5]], ['B2DesktopObserver', 'B2MemoryObserver', 'CoordinatedObserver', 'Observer', 'Thread'])
        self.assertIs(O._stopped, C.CoordinatedObserver._stopped)
        self.assertIs(O.run, C.CoordinatedObserver.run)
        self.assertIs(O._handle, B2.B2MemoryObserver._handle)
        self.assertIs(O.write_results, B2.B2MemoryObserver.write_results)
        self.assertIs(O._regions, B2.B2MemoryObserver._regions)
        self.assertIs(O._stacks_present, B2.B2MemoryObserver._stacks_present)
        self.assertIs(O.snapshot, RET.OBS.Observer.snapshot)
        self.assertEqual([name for name in vars(O) if not name.startswith('__')], ['_b2_entry', '_snapshot'])

    def test_terminal_entry_records_ranges_proofs_and_focus(self):
        guest = self.two_task_guest()
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            self.assertEqual(observer.focus, TERMINAL_BASE)
            spawn = observer.spawns[0]
            self.assertEqual(spawn['name'], 'terminal')
            self.assertEqual(spawn['entry']['user_stack']['pages'], 64)
            self.assertEqual({k: v['verified'] for k, v in spawn['entry']['primed_status'].items()}, {'user-stack': True, 'kernel-stack': True})
            self.assertTrue((Path(tmp) / 'evidence/memory-entry-01-terminal.json').is_file())
            self.assertNotIn('write', ' '.join(observer.client.calls))

    def test_unrelated_entries_are_counted_and_ignored(self):
        guest = self.two_task_guest()
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            observer.client.read_memory = lambda addr, length: b'\x90' * length
            observer._b2_entry(TERMINAL_BASE)
            self.assertEqual((observer.unmatched_entries, observer.spawns, observer.focus), (1, [], None))

    def test_entry_range_failure_stops_the_run_closed(self):
        guest = TW.build(user_pages=64, kernel_pages=64, frame_pc=TERMINAL_BASE, guard_user=True)
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            with self.assertRaises(X.Stop):
                enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            self.assertIn('stack', observer.memory_failure)

    def test_user_mode_is_required_at_an_entry_stop(self):
        guest = self.two_task_guest()
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            target = observer.targets.targets[0]
            windows = {TERMINAL_BASE + va: w for va, w in target['windows']}
            observer.client.read_memory = lambda addr, length: windows.get(addr, b'\0' * length)
            observer.client.read_register = lambda name: {'cs': 0x10, 'cr3': guest.cr3, 'rsp': TW.UTOP - 0x3F0}[name]
            with self.assertRaises(X.Stop):
                observer._b2_entry(TERMINAL_BASE)

    def test_snapshot_records_terminal_and_shell_and_states_allocator_not_measured(self):
        guest = self.two_task_guest()
        add_task(guest, user_top=0x7800_0000, kernel_top=0x6800_0000, pc=SHELL_BASE, pages=16)
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            TW.poke(guest, TW.CPU_LOCALS, struct.pack('<Q', 0x6800_0000 - 288))
            TW.poke(guest, TW.TABLES + 68, struct.pack('<Q', 0x6800_0000 - 288))
            enter(observer, 'shell', SHELL_BASE, 0x7800_0000 - 0x3F0)
            shrink_loads(observer, TERMINAL_BASE)
            observer._snapshot('b2d-echo')
            document = json.loads((Path(tmp) / 'evidence/memory-00-b2d-echo.json').read_text())
            self.assertEqual([r['spawn']['name'] for r in document['records']], ['terminal', 'shell'])
            self.assertIsNone(document['allocator_peak'])
            self.assertIn('not measured for B2', document['allocator_peak_reason'])
            self.assertEqual(document['focus_task'], 'terminal')
            self.assertEqual(document['schema'], 'cellos.s10-9b.b2-desktop-memory-snapshot/1')
            self.assertIn('in-process', document['tools_not_spawned_note'])
            self.assertTrue(all(r['ok'] for r in document['records']))
            self.assertEqual(document['provenance']['set'], 'b2-desktop')
            self.assertEqual(observer.memory_records[0]['observed'], ['terminal', 'shell'])

    def test_known_answer_stack_depth_of_the_terminal(self):
        guest = self.two_task_guest()
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            shrink_loads(observer, TERMINAL_BASE)
            guest.write(TW.UTOP - 0x4000 - 8, b'\x11' * 8)          # a dirtied word 16 KiB + 8 below the top
            observer._snapshot('b2d-ps')
            record = json.loads((Path(tmp) / 'evidence/memory-00-b2d-ps.json').read_text())['records'][0]
            self.assertEqual(record['stack_depth_bytes']['user'], 0x4000 + 8)
            self.assertTrue(record['all_stack_depths_exact'])
            self.assertTrue(record['spawn']['primed_status']['user-stack']['verified'])

    def test_snapshot_without_terminal_entry_stops(self):
        guest = self.two_task_guest()
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            with self.assertRaises(X.Stop):
                observer._snapshot('complete')
            add_task(guest, user_top=0x7800_0000, kernel_top=0x6800_0000, pc=SHELL_BASE, pages=16)
            TW.poke(guest, TW.CPU_LOCALS, struct.pack('<Q', 0x6800_0000 - 288))
            TW.poke(guest, TW.TABLES + 68, struct.pack('<Q', 0x6800_0000 - 288))
            enter(observer, 'shell', SHELL_BASE, 0x7800_0000 - 0x3F0)
            with self.assertRaises(X.Stop):                        # a shell-only run is not a B2 desktop run
                observer._snapshot('complete')

    def test_terminal_memory_error_stops_but_a_shell_error_is_only_recorded(self):
        guest = self.two_task_guest()
        add_task(guest, user_top=0x7800_0000, kernel_top=0x6800_0000, pc=SHELL_BASE, pages=16)
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            TW.poke(guest, TW.CPU_LOCALS, struct.pack('<Q', 0x6800_0000 - 288))
            TW.poke(guest, TW.TABLES + 68, struct.pack('<Q', 0x6800_0000 - 288))
            enter(observer, 'shell', SHELL_BASE, 0x7800_0000 - 0x3F0)
            shrink_loads(observer, TERMINAL_BASE)
            guest.unreadable.add(0x7800_0000 - 8 * PAGE)                    # one page of the SHELL user stack becomes unreadable
            observer._snapshot('b2d-ls')                                    # no Stop: the focus task is fine
            document = json.loads((Path(tmp) / 'evidence/memory-00-b2d-ls.json').read_text())
            by_name = {r['spawn']['name']: r for r in document['records']}
            self.assertTrue(by_name['terminal']['ok'])
            self.assertFalse(by_name['shell']['ok'])
            self.assertTrue(by_name['shell']['errors'])
            guest.unreadable.add(TW.UTOP - 8 * PAGE)                        # now one page of the TERMINAL user stack
            with self.assertRaises(X.Stop):
                observer._snapshot('b2d-cat')

    def test_a_slot_reentry_supersedes_the_older_spawn(self):
        guest = self.two_task_guest()
        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            self.assertEqual([s['superseded'] for s in observer.spawns], [True, False])
            shrink_loads(observer, TERMINAL_BASE)
            observer._snapshot('complete')
            document = json.loads((Path(tmp) / 'evidence/memory-00-complete.json').read_text())
            self.assertEqual([r['spawn']['ordinal'] for r in document['records']], [2])
            self.assertEqual(document['unobserved'][0]['reason'], 'slot re-entered by a later spawn')

    def test_stops_run_under_the_gate_and_the_gate_ends_with_the_thread(self):
        """Real thread lifecycle (the S10-7 `_stop` collision class): starts, services a snapshot request, finishes and joins cleanly."""
        guest = self.two_task_guest()
        gate = FakeGate()

        class Rsp:
            def __init__(self):
                self._state, self.calls, self.guest = 'stopped', [], guest

            def connect(self):
                self.calls.append('connect')

            def add_breakpoint(self, addr):
                self.calls.append(('Z1', addr))

            def remove_breakpoint(self, addr):
                self.calls.append(('z1', addr))

            def resume(self):
                self._state = 'running'

            def wait_stop(self, timeout=None):
                raise RET.OBS.RspTimeout('quiet guest')

            def interrupt(self):
                self._state = 'stopped'
                return b'T02thread:01;'

            def read_register(self, name):
                return 0

            def read_physical_memory(self, addr, length):
                return self.guest.read_physical(addr, length)

            def close(self):
                self.calls.append('close')

        with tempfile.TemporaryDirectory() as tmp:
            observer = observer_for(tmp, guest, gate=gate)
            enter(observer, 'terminal', TERMINAL_BASE, TW.UTOP - 0x3F0)
            shrink_loads(observer, TERMINAL_BASE)
            rsp = Rsp()
            observer.client = rsp
            observer.start()
            self.assertTrue(observer.ready.wait(5))
            observer.snapshot('initial-prompt')
            observer.shutdown()
            self.assertFalse(observer.is_alive())
            self.assertIsNone(observer.error)
            self.assertEqual(sum(1 for c in rsp.calls if isinstance(c, tuple) and c[0] == 'Z1'), 512)
            self.assertIn('paused-enter', gate.events)
            self.assertEqual(gate.events.count('paused-enter'), gate.events.count('paused-exit'))
            self.assertEqual(gate.events[-1][0], 'terminate')
            self.assertTrue((Path(tmp) / 'evidence/memory-00-initial-prompt.json').is_file())
            self.assertEqual(json.loads((Path(tmp) / 'evidence/observation.json').read_text())['status'], 'COMPLETE')


class Machine(unittest.TestCase):
    def test_argv_is_the_retained_desktop_argv_without_the_data_disk(self):
        with tempfile.TemporaryDirectory() as tmp:
            machine = B2D.B2DesktopMachine(ENV.SPEC, Path(tmp) / 'vm', observed=True)
            argv = machine.argv
            text = ' '.join(argv)
            self.assertIn('-machine q35 -accel tcg -cpu qemu64,+pdpe1gb -m 256M -display none', text)
            self.assertIn('-netdev user,id=net0,restrict=on', text)
            self.assertIn('-device virtio-keyboard-pci -device virtio-tablet-pci', text)
            self.assertNotIn('nvme', text)
            self.assertNotIn('p10fx', text)
            self.assertNotIn('-smp', argv)
            self.assertEqual(argv[-5:], ['-chardev', f'socket,path={machine.gdb_path},server=on,wait=off,id=gdb0', '-gdb', 'chardev:gdb0', '-S'])
            self.assertIn(f'file={machine.private_iso},media=cdrom,readonly=on,format=raw', text)
            self.assertEqual(ENV.sha256_file(machine.private_iso), ENV.PINNED['iso'])
            self.assertEqual(oct(Path(tmp, 'vm', 'sock').stat().st_mode & 0o777), '0o700')
            self.assertEqual(PF.argv_private_violations(argv, str(machine.work)), [])
            record = json.loads((machine.work / 'evidence/input-provenance.json').read_text())
            self.assertEqual(record['argv'], argv)
            self.assertIsNone(record['fixture_disk'])
            self.assertEqual(record['kernel_provenance']['set'], 'b2-desktop')
            machine.stop()
            proof = json.loads((machine.work / 'evidence/final-input-proof.json').read_text())
            self.assertTrue(proof['files_match_pins'] and proof['private_iso_unchanged'])

    def test_argv_equals_the_s10_9_cand_desktop_argv_modulo_iso_and_data_disk(self):
        import s107_machine as MACH
        recorded = (ENV.REPORTS / 's10-9-evidence/measurement-runs/cand-desktop/evidence/qemu-argv-measurement.txt').read_text().split()
        with tempfile.TemporaryDirectory() as tmp:
            machine = B2D.B2DesktopMachine(ENV.SPEC, Path(tmp) / 'vm', observed=False)
            got = [a.replace(str(machine.work), '$WORK') for a in machine.argv]
        normalise = lambda argv: ['<ISO>' if a.startswith('file=') and a.endswith('media=cdrom,readonly=on,format=raw') else a for a in argv]  # noqa: E731
        self.assertEqual(normalise(MACH.drop_data_disk(recorded)), normalise(got))

    def test_wrong_inputs_refuse_before_anything_is_created_for_qemu(self):
        with tempfile.TemporaryDirectory() as tmp:
            bad = Path(tmp) / 'bad-glyph'
            bad.write_bytes(b'x')
            with mock.patch.object(ENV.B2DesktopSpec, 'glyph_bin', bad):
                with self.assertRaises(X.Stop):
                    B2D.B2DesktopMachine(ENV.SPEC, Path(tmp) / 'vm', observed=True)


class FakeDesk:
    instances = []

    def __init__(self, args, machine, verdict, check, gate):
        self.calls, self.model, self.state, self.org, self.temporal_epochs, self.failure_records = [], [], {}, (140, 100), [], []
        FakeDesk.instances.append(self)

    def shot(self, label, img=None):
        self.calls.append(('shot', label))
        return types.SimpleNamespace(size=(1280, 800), crop=lambda box: ('crop', box))

    def locate(self, img):
        return None

    def click_px(self, x, y):
        self.calls.append(('click', x, y))

    def park(self):
        self.calls.append('park')

    def expect_screen(self, rows, label, timeout):
        self.calls.append(('expect', list(rows), timeout))
        return 'IMAGE'

    def diff(self, win, rows, first, last, cursor):
        self.calls.append(('diff', list(rows)[-1] if rows else None, cursor))
        return 5000, 255

    pad = staticmethod(lambda rows: list(rows))
    mcursor = staticmethod(lambda rows: (len(rows[-1]), len(rows) - 1))

    def window(self, img):
        return (140, 100), 'WINDOW'

    def cursor_of(self, window):
        return B2D.INITIAL_CURSOR

    def command(self, name, text, outputs, timeout=180):
        self.calls.append(('command', name, text, [list(o) for o in outputs], timeout))
        self.model[:] = self.model[:-1] + [self.model[-1] + text] + list(outputs[0]) + ['Cellos > ']
        return 0


class FakeSurfaceGate:
    def __init__(self):
        self.scopes = []

    def caller_scope(self, seconds):
        gate = self

        class Scope:
            def __enter__(self):
                gate.scopes.append(('enter', seconds))

            def __exit__(self, *exc):
                gate.scopes.append(('exit', seconds))
                return False
        return Scope()

    phase = 'ORDINARY_RESUME_PENDING_OR_RUNNING'


class FakeSurfaceMachine:
    def __init__(self):
        self.a = types.SimpleNamespace(profile='desktop')
        self.start_ns = __import__('time').monotonic_ns()

    def text(self):
        return '[desktop] Ready. Entering event loop. [terminal] Terminal surface ready. Entering event loop.'

    def alive(self):
        return True


class FakeObserverForSurface:
    def __init__(self):
        self.phases, self.snapshots = [], []

    def set_phase(self, name):
        self.phases.append(name)

    def snapshot(self, name):
        self.snapshots.append(name)

    def check(self):
        return None


class Surface(unittest.TestCase):
    def surface(self, schedule=None):
        FakeDesk.instances.clear()
        observer, gate = FakeObserverForSurface(), FakeSurfaceGate()
        surface = B2D.B2DesktopSurface(FakeSurfaceMachine(), observer, gate, schedule)
        surface.pause = lambda seconds: None
        return surface, observer, gate

    def test_ready_sequence_uses_the_b2_terminal_rows_and_the_retained_controls(self):
        surface, observer, gate = self.surface()
        with mock.patch.object(B2D.G, 'TemporalDesk', FakeDesk):
            surface.ready()
        desk = FakeDesk.instances[0]
        expects = [c for c in desk.calls if isinstance(c, tuple) and c[0] == 'expect']
        self.assertEqual(expects, [('expect', B2D.INITIAL_ROWS, 240)])
        diffs = [c for c in desk.calls if isinstance(c, tuple) and c[0] == 'diff']
        self.assertEqual(diffs[0][1], 'Cellos > ')                       # pre-launch region vs the expected screen
        self.assertEqual(diffs[1][1], 'Cellos # ')                       # wrong-prompt negative control
        self.assertEqual(diffs[1][2], B2D.INITIAL_CURSOR)
        self.assertIn(('click', D.TASKBAR['terminal'], D.TASKBAR_Y), desk.calls)
        self.assertEqual(observer.phases, ['desktop-ready', 'terminal-launch', 'initial-prompt'])
        self.assertEqual(observer.snapshots, ['initial-prompt'])
        self.assertEqual(desk.model, B2D.INITIAL_ROWS)
        self.assertIn('desktop_ready_ns', surface.timestamps)
        self.assertIn('exact_prompt_ns', surface.timestamps)
        self.assertEqual(gate.scopes[0], ('enter', 900))

    def test_ready_refuses_a_schedule_whose_rows_differ_from_the_source_rows(self):
        schedule = ENV.schedule()
        schedule['phases'][2]['expect_rows'][0] = '=== Cellos shell ready ==='
        surface, _observer, _gate = self.surface(schedule)
        with mock.patch.object(B2D.G, 'TemporalDesk', FakeDesk):
            with self.assertRaises(X.Stop):
                surface.ready()

    def test_shared_subset_runs_each_command_once_in_order_with_scope_and_snapshots(self):
        surface, observer, gate = self.surface()
        with mock.patch.object(B2D.G, 'TemporalDesk', FakeDesk):
            surface.ready()
        observer.phases.clear()
        observer.snapshots.clear()
        results = surface.shared_subset()
        desk = FakeDesk.instances[0]
        commands = [c for c in desk.calls if isinstance(c, tuple) and c[0] == 'command']
        self.assertEqual([c[2] for c in commands], ['echo b2-shared-subset', 'ls /', 'cat /etc/cellos/system.toml', 'ps'])
        self.assertTrue(all(c[4] == 120 for c in commands))
        self.assertEqual([c[3][0] for c in commands][2], ['shell: command not found: cat'])
        self.assertEqual(observer.phases, ['b2d-echo', 'b2d-ls', 'b2d-cat', 'b2d-ps'])
        self.assertEqual(observer.snapshots, ['b2d-echo', 'b2d-ls', 'b2d-cat', 'b2d-ps'])
        self.assertEqual([r['exact_screen_match'] for r in results], [True] * 4)
        self.assertTrue(all('not_applicable' in r['status'] for r in results))
        self.assertEqual(gate.scopes.count(('enter', 120)), 4)
        self.assertEqual([r['phase'] for r in surface.command_results], ['b2d-echo', 'b2d-ls', 'b2d-cat', 'b2d-ps'])

    def test_an_oracle_failure_stops_and_nothing_is_retried(self):
        surface, observer, _gate = self.surface()
        with mock.patch.object(B2D.G, 'TemporalDesk', FakeDesk):
            surface.ready()
        desk = FakeDesk.instances[0]
        calls = []

        def failing(name, text, outputs, timeout=180):
            calls.append(name)
            raise X.Stop('timeout waiting for b2d-ls: output')
        desk.command = failing
        with self.assertRaises(X.Stop):
            surface.shared_subset()
        self.assertEqual(calls, ['b2d-echo'])
        self.assertNotIn('b2d-echo', observer.snapshots[1:])


class Classification(unittest.TestCase):
    def table_dir(self, tmp, phases, *, ok=True, exact=True, task='terminal'):
        evidence = Path(tmp) / 'evidence'
        evidence.mkdir()
        for index, phase in enumerate(phases):
            record = {'phase': phase, 'ok': ok, 'all_stack_depths_exact': exact, 'stack_depth_bytes': {'user': 9168, 'kernel': 14016}, 'errors': [],
                      'resident': {'regions': [{'label': 'elf-load-0', 'kind': 'elf-segment', 'size_bytes': 5, 'mapped_backed_bytes': 4096, 'fully_mapped': True}],
                                   'total': {'distinct_frame_bytes': 4096}},
                      'stacks': [{'label': 'user-stack', 'usable_bytes': 262144, 'depth_bytes': 9168, 'exact': exact, 'status': 'partial'}],
                      'heap_arena': {'size_bytes': 4194304}, 'spawn': {'name': task, 'ordinal': 1}}
            (evidence / f'memory-{index:02d}-{phase}.json').write_text(json.dumps({'phase': phase, 'records': [record], 'unobserved': []}))
        return evidence

    def classify(self, tmp, phases, *, status='COMPLETE', ready=True, **kwargs):
        schedule = ENV.schedule()
        table = RUN.summarize_memory(self.table_dir(tmp, phases, **kwargs)) if phases else []
        facts = RUN.facts_from_table(table, 'terminal')
        return RUN.classify_run(status=status, expected_snapshots=ENV.schedule_snapshots(schedule), taken=facts['taken'], entries_ok=bool(phases), ready=ready,
                                kind='b2', snapshot_ok=facts['ok'], snapshot_exact=facts['exact'], allocator_measured=False)

    def test_all_phases_complete_without_allocator_is_class_b(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = self.classify(tmp, ENV.schedule_snapshots(ENV.schedule()))
        self.assertEqual(result['class'], 'B')
        self.assertIn('allocator peak not measured', result['reason'])
        self.assertEqual(result['missing_snapshots'], [])

    def test_partial_run_is_class_c_no_snapshot_d_never_ready_e(self):
        names = ENV.schedule_snapshots(ENV.schedule())
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(self.classify(tmp, names[:3], status='STOP')['class'], 'C')
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(self.classify(tmp, [], status='STOP')['class'], 'D')
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(self.classify(tmp, [], status='STOP', ready=False)['class'], 'E')

    def test_only_the_focus_task_feeds_the_classification_facts(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = self.classify(tmp, ENV.schedule_snapshots(ENV.schedule()), task='shell')     # shell records alone: terminal never snapshotted
        self.assertEqual(result['class'], 'D')

    def test_inexact_flag_keeps_class_b_with_the_reason(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = self.classify(tmp, ENV.schedule_snapshots(ENV.schedule()), exact=False)
        self.assertEqual(result['class'], 'B')
        self.assertIn('not every stack depth is pre-run-proven exact', result['reason'])

    def test_memory_report_idle_and_phase_facts(self):
        schedule = ENV.schedule()
        with tempfile.TemporaryDirectory() as tmp:
            evidence = self.table_dir(tmp, ['initial-prompt', 'complete'])
            rows = RUN.memory_report(evidence)
        self.assertEqual([r['phase'] for r in rows], ['initial-prompt', 'complete'])
        self.assertEqual(rows[0]['regions'][0]['mapped_backed_bytes'], 4096)
        self.assertEqual(rows[0]['stacks'][0]['depth_bytes'], 9168)
        idle = RUN.idle_facts({'ordinary_idle': {'samples': [{'changed_pixels': 0}, {'changed_pixels': 3}], 'qemu_cpu_ticks_delta': 7, 'serial_bytes': 0}})
        self.assertEqual((idle['samples'], idle['frames_with_changed_pixels'], idle['samples_expected']), (2, 1, 100))
        phases = RUN.phase_facts({'surface_events': [{'phase': 'desktop-ready'}, {'phase': 'terminal-launch'}], 'command_results': []}, schedule)
        self.assertEqual(phases['phases_entered'], ['desktop-ready', 'terminal-launch'])
        self.assertIn('complete', phases['phases_not_entered'])


class RunnerOrderAndSafety(unittest.TestCase):
    def test_preflight_precedes_the_token_which_precedes_every_vm_action(self):
        source = (HERE / 's109b_run.py').read_text()
        main = source[source.index('def main('):]
        self.assertLess(main.index('PF.preflight()'), main.index('ENV.claim_run()'))
        self.assertLess(main.index('ENV.claim_run()'), main.index('run_b2_desktop(spec'))
        self.assertLess(main.index('if not outcome'), main.index('ENV.claim_run()'))
        run = source[source.index('def run_b2_desktop('):source.index('# ------------------------------------------------------------------ reporting')]
        self.assertLess(run.index('B2D.B2DesktopMachine('), run.index('machine.start()'))
        self.assertEqual(run.count('machine.start()'), 1)
        self.assertEqual(run.count('surface.ready()'), 1)
        self.assertNotIn('retry', run.lower().replace('no retry', ''))

    def test_sources_contain_no_guest_write_or_forbidden_host_primitives(self):
        for name in ('s109b_b2d.py', 's109b_run.py', 's109b_env.py', 's109b_preflight.py'):
            text = (HERE / name).read_text()
            argv_words = () if name == 's109b_preflight.py' else ('hostfwd', '-vnc', 'run-desktop')     # the preflight only NAMES them as forbidden checks
            for forbidden in ('write_memory', "request('M", 'request("M', 'Z0,', 'add_watchpoint', 'write_register', "request('P", *argv_words,
                              'pkill', 'killall', 'os.kill', 'shell=True', '-enable-kvm', 'git commit', 'git push'):
                self.assertNotIn(forbidden, text, f'{name}: {forbidden}')

    def test_only_own_qemu_is_ever_terminated(self):
        text = (ENV.RUNTIME / 'common.py').read_text() + (ENV.REPO / 'scripts/phase10_canonical_drive.py').read_text()
        for forbidden in ('pkill', 'killall', 'os.kill('):
            self.assertNotIn(forbidden, text)
        self.assertIn('self.proc.terminate()', (ENV.REPO / 'scripts/phase10_canonical_drive.py').read_text())

    def test_run_writes_only_under_the_two_new_roots(self):
        source = (HERE / 's109b_run.py').read_text()
        self.assertIn('ENV.RUN_EVIDENCE', source)
        self.assertEqual(ENV.RUN_EVIDENCE.parent, ENV.EVIDENCE)
        self.assertEqual(ENV.EVIDENCE, ENV.REPORTS / 's10-9-evidence' / 'b2-desktop-run')
        self.assertEqual(ENV.WORK_ROOT, Path('/tmp/cellbox-s10-9b'))
        self.assertTrue(str(ENV.HARNESS_EVIDENCE).startswith(str(ENV.EVIDENCE)))


if __name__ == '__main__':
    unittest.main()
