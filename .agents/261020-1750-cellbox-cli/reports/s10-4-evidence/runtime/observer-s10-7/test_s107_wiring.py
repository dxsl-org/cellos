"""S10-7.4 offline tests: memory wiring (range discovery, fail-closed cases, entry proofs, mixin, schema, provenance). No VM."""
import json
import struct
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402
import s107_memory as MEM  # noqa: E402
import test_memory as TM  # noqa: E402  (retained S10-7.2 fixtures: FakeGuest with real 4-level tables)
import common as COMMON  # noqa: E402

PAGE = 4096
UTOP, KTOP = 0x7000_0000, 0x6000_0000
PC0 = 0x1_0000_0000
CPU_LOCALS, TABLES = 0xFFFF_FFFF_81AF_C100, 0xFFFF_FFFF_81AF_AF40
SYMBOLS = types.SimpleNamespace(cpu_locals=CPU_LOCALS, tables=TABLES)


def poke(guest, va, data):
    pa, _ = guest.view().translate(va & ~0xFFF)
    off = va & 0xFFF
    guest.pages[pa][off:off + len(data)] = data


def build(*, user_pages=16, kernel_pages=16, kernel_rsp=None, tss=None, other_cpu=0, guard_user=False, guard_kernel=False,
          prime_user=True, prime_kernel=True, frame_pc=PC0, frame_user_top=UTOP):
    guest = TM.FakeGuest()
    user_bottom, kernel_bottom = UTOP - user_pages * PAGE, KTOP - kernel_pages * PAGE
    guest.map_identity_range(user_bottom, UTOP, fill=TM.PATTERN if prime_user else 0)
    guest.map_identity_range(kernel_bottom, KTOP, fill=TM.PATTERN if prime_kernel else 0)
    guest.write(UTOP - 0x400, b'\x00' * 0x400)              # loader frame in the top user page
    guest.write(KTOP - 0x300, b'\x00' * 0x300)              # trap frame + switch frame in the top kernel page
    guest.write(KTOP - 288 + 264, struct.pack('<Q', frame_pc))           # seeded ViTrapFrame.sepc = entry
    guest.write(KTOP - 288 + 16, struct.pack('<Q', frame_user_top))      # seeded ViTrapFrame.regs[2] = user stack top
    guest.write(KTOP - 288 + 216, struct.pack('<5Q', frame_pc, 0x23, 0x202, UTOP - 0x3F0, 0x1B))   # iretq words built by __trap_exit
    if guard_user:
        guest.map_identity_range(user_bottom - 2 * PAGE, user_bottom - PAGE, fill=0)     # second guard page mapped, first absent
    if guard_kernel:
        guest.map_identity_range(kernel_bottom - 2 * PAGE, kernel_bottom - PAGE, fill=0)
    for page in {CPU_LOCALS & ~0xFFF, (TABLES + 68) & ~0xFFF}:
        pa = guest._table()
        guest.map(page, pa)
    rsp0 = KTOP - 288 if kernel_rsp is None else kernel_rsp
    poke(guest, CPU_LOCALS, struct.pack('<Q', rsp0))
    poke(guest, CPU_LOCALS + 64, struct.pack('<Q', other_cpu))
    poke(guest, TABLES + 68, struct.pack('<Q', rsp0 if tss is None else tss))
    return guest


class Discovery(unittest.TestCase):
    def test_user_stack_derivation(self):
        view = build().view()
        found = MEM.discover_user_stack(view, UTOP - 0x3F0, allowed_pages=(16,))
        self.assertEqual((found['bottom'], found['top'], found['pages']), (UTOP - 16 * PAGE, UTOP, 16))

    def test_user_stack_wrong_run_fails_closed(self):
        for pages in (15, 17):
            with self.assertRaises(MEM.DiscoveryError, msg=f'{pages} pages'):
                MEM.discover_user_stack(build(user_pages=pages).view(), UTOP - 0x3F0, allowed_pages=(16,))

    def test_user_stack_guard_mapped_fails_closed(self):
        with self.assertRaisesRegex(MEM.DiscoveryError, 'guard'):
            MEM.discover_user_stack(build(guard_user=True, user_pages=16).view(), UTOP - 0x3F0, allowed_pages=(16,))

    def test_user_stack_allowed_set_for_b2_tools(self):
        self.assertEqual(MEM.discover_user_stack(build(user_pages=64).view(), UTOP - 8, allowed_pages=(16, 64))['pages'], 64)

    def test_kernel_stack_derivation(self):
        view = build().view()
        found = MEM.discover_kernel_stack(view, SYMBOLS, allowed_pages=(16,), entry_pc=PC0, user_top=UTOP)
        self.assertEqual((found['bottom'], found['top'], found['pages'], found['kernel_rsp']), (KTOP - 16 * PAGE, KTOP, 16, KTOP - 288))
        self.assertEqual(found['tss_rsp0'], found['kernel_rsp'])
        self.assertEqual((found['owner_check']['sepc'], found['owner_check']['regs2'], found['owner_check']['iretq_cs']), (PC0, UTOP, 0x23))

    def test_kernel_stack_fail_closed_cases(self):
        cases = {
            'kernel_rsp zero': dict(kernel_rsp=0, tss=0),
            'tss disagrees': dict(tss=KTOP - 288 - 16),
            'second hart populated': dict(other_cpu=KTOP - 288),
            'not 288 below the page top': dict(kernel_rsp=KTOP - 296, tss=KTOP - 296),
            'wrong page count': dict(kernel_pages=15),
            'guard mapped': dict(guard_kernel=True),
        }
        for name, kwargs in cases.items():
            with self.subTest(name), self.assertRaises(MEM.DiscoveryError):
                MEM.discover_kernel_stack(build(**kwargs).view(), SYMBOLS, allowed_pages=(16,), entry_pc=PC0, user_top=UTOP)

    def test_kernel_stack_of_another_task_fails_closed(self):
        # geometry is valid (16 mapped pages, guards, 288-byte frame) but the seeded trap frame names a different task
        for name, kwargs in {'other entry pc': dict(frame_pc=PC0 + 32 * 1024 * 1024), 'other user stack top': dict(frame_user_top=UTOP + 0x20000)}.items():
            with self.subTest(name), self.assertRaisesRegex(MEM.DiscoveryError, 'not this task'):
                MEM.discover_kernel_stack(build(**kwargs).view(), SYMBOLS, allowed_pages=(16,), entry_pc=PC0, user_top=UTOP)

    def test_kernel_stack_unreadable_record_fails_closed(self):
        guest = build()
        pa, _ = guest.view().translate(CPU_LOCALS & ~0xFFF)
        guest.unreadable.add(pa)
        with self.assertRaisesRegex(MEM.DiscoveryError, 'unreadable'):
            MEM.discover_kernel_stack(guest.view(), SYMBOLS, allowed_pages=(16,), entry_pc=PC0, user_top=UTOP)

    def test_kernel_symbols_not_in_a_pie_kernel(self):
        # the real measurement kernels are non-PIE EXEC images: static higher-half addresses, matching `nm`
        for name in ENV.SET_NAMES:
            spec = ENV.SETS[name]
            symbols = MEM.KernelSymbols(spec.kernel)
            nm = subprocess.run(['nm', str(spec.kernel)], check=True, capture_output=True, text=True).stdout.splitlines()
            want = {line.split()[2]: int(line.split()[0], 16) for line in nm if len(line.split()) == 3 and ('CPU_LOCALS' in line or '6TABLES' in line)}
            self.assertIn(symbols.cpu_locals, want.values(), name)
            self.assertIn(symbols.tables, want.values(), name)
            self.assertGreaterEqual(symbols.cpu_locals, 0xFFFF_8000_0000_0000)

    def test_heap_arena_from_elf_symbols(self):
        _record, files = ENV.candidate_files('desktop')
        rel, size = MEM.heap_arena_symbol(files['/bin/cellbox']['file'], kind='cellbox')
        self.assertEqual((rel, size), (0x2A8060, 6 * 1024 * 1024))
        _record, files = ENV.candidate_files('headless')
        rel, size = MEM.heap_arena_symbol(files['/bin/cellbox']['file'], kind='cellbox')
        self.assertEqual(size, 6 * 1024 * 1024)
        rel, size = MEM.heap_arena_symbol(ENV.B2_IMAGE / 'signed/headless-app-shell', kind='b2-shell')
        self.assertEqual((rel, size), (0xBC050, 1024 * 1024))
        with self.assertRaises(MEM.DiscoveryError):
            MEM.heap_arena_symbol(ENV.B2_IMAGE / 'signed/headless-app-shell', kind='cellbox')   # no such symbol: fail closed


class EntryRecord(unittest.TestCase):
    def record(self, guest, **kwargs):
        return MEM.entry_record(guest.view(), label='t', rsp=UTOP - 0x3F0, pc=PC0, user_pages=(16,), kernel_pages=(16,), symbols=SYMBOLS, **kwargs)

    def test_exact_proofs_taken_at_entry(self):
        record = self.record(build())
        self.assertTrue(record['primed_status']['user-stack']['verified'] and record['primed_status']['kernel-stack']['verified'])
        self.assertEqual(record['primed_status']['user-stack']['limit'], UTOP - PAGE)          # rsp page excluded
        self.assertEqual(record['primed_status']['kernel-stack']['limit'], KTOP - PAGE)        # trap-frame page excluded
        self.assertIsNotNone(record['primed_proofs']['user-stack'])
        self.assertIn('excluded from the proof', record['proof_scope'])

    def test_dirty_prime_is_an_honest_flag_not_a_stop(self):
        guest = build()
        guest.write(KTOP - 5 * PAGE + 7, b'\x01')                  # stray byte below the trap-frame page
        record = self.record(guest)
        self.assertFalse(record['primed_status']['kernel-stack']['verified'])
        self.assertIsNone(record['primed_proofs']['kernel-stack'])
        self.assertEqual(record['primed_status']['kernel-stack']['first_differing_va'], KTOP - 5 * PAGE + 7)
        self.assertTrue(record['primed_status']['user-stack']['verified'])

    def test_unprimed_stack_cannot_claim_exact(self):
        record = self.record(build(prime_user=False))
        self.assertFalse(record['primed_status']['user-stack']['verified'])

    def test_unreadable_memory_stops(self):
        guest = build()
        guest.unreadable.add(UTOP - 10 * PAGE)
        with self.assertRaisesRegex(MEM.DiscoveryError, 'failed to run'):
            self.record(guest)

    def test_observer_stack_must_equal_independent_derivation(self):
        stack = {'bottom': UTOP - 16 * PAGE + PAGE, 'top': UTOP, 'pages': 15}
        with self.assertRaisesRegex(MEM.DiscoveryError, 'independently derived'):
            self.record(build(), user_stack=stack)

    def test_rsp_in_bottom_page_leaves_nothing_to_prove(self):
        with self.assertRaises(MEM.DiscoveryError):
            MEM.entry_record(build().view(), label='t', rsp=UTOP - 16 * PAGE + 8, pc=PC0, user_pages=(16,), kernel_pages=(16,), symbols=SYMBOLS)


# ------------------------------------------------------------------ mixin over a fake retained observer
class FakeClient:
    """Only the read-only surface the retained observer already uses; any other attribute is a test failure (no writes)."""

    def __init__(self, guest, heap=0x1234):
        self.guest, self.calls, self.heap = guest, [], heap

    def read_physical_memory(self, addr, length):
        self.calls.append('read_physical_memory')
        return self.guest.read_physical(addr, length)

    def read_memory(self, addr, length):
        self.calls.append('read_memory')
        return self.heap.to_bytes(8, 'little')

    def __getattr__(self, name):
        raise AssertionError(f'memory wiring used a non-read-only client call: {name}')


class FakeObserver:
    def __init__(self, machine, guest, entry_pc):
        self.m, self.client, self.entry_pc = machine, FakeClient(guest), entry_pc
        self.instances, self.focus, self.loads, self.base_snapshots, self.written = {}, None, [{'va': 0, 'memsz': 3 * PAGE, 'filesz': PAGE, 'flags': 5}], [], False

    def _entry(self, pc):
        if pc == self.entry_pc:
            self.instances[pc] = {'id': 1, 'role': 'headless', 'base': pc, 'cr3': self.client.guest.cr3, 'initial_rsp': UTOP - 0x3F0,
                                  'stack_top': UTOP, 'stack_bottom': UTOP - 16 * PAGE, 'heap': pc + 0x40}
            self.focus = pc

    def _snapshot(self, name):
        self.base_snapshots.append(name)
        return 'retained-result'

    def write_results(self):
        self.written = True


class Observer(MEM.MemoryMixin, FakeObserver):
    SKIP_SNAPSHOT_NAMES = ('\0disarm',)


def context(**kwargs):
    ctx = MEM.MemoryContext.__new__(MEM.MemoryContext)
    ctx.provenance = ENV.provenance('cand-headless')
    ctx.symbols, ctx.heap_arena_rel, ctx.user_pages, ctx.kernel_pages = SYMBOLS, kwargs.get('arena', (0x2000, 2 * PAGE)), (16,), (16,)
    return ctx


class Mixin(unittest.TestCase):
    PC = PC0

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.work = Path(self.tmp.name)
        (self.work / 'evidence').mkdir()
        self.machine = types.SimpleNamespace(work=self.work)
        self.addCleanup(self.tmp.cleanup)

    def observer(self, guest=None, **kwargs):
        guest = guest or build()
        # the ELF segments and the heap arena must be mapped too
        guest.map_identity_range(self.PC, self.PC + 3 * PAGE, fill=0)
        return Observer(self.machine, guest, self.PC, memory=context(**kwargs))

    def test_entry_records_ranges_and_proofs_and_writes_evidence(self):
        observer = self.observer()
        observer._entry(self.PC)
        entry = observer.memory_entries[self.PC]
        self.assertEqual(entry['kernel_stack']['top'], KTOP)
        self.assertTrue(all(v['verified'] for v in entry['primed_status'].values()))
        self.assertTrue((self.work / 'evidence/memory-entry-1.json').is_file())

    def test_unrelated_entries_are_ignored(self):
        observer = self.observer()
        observer._entry(self.PC + 32 * 1024 * 1024)
        self.assertEqual(observer.memory_entries, {})

    def test_entry_range_failure_stops_the_run(self):
        observer = self.observer(build(kernel_rsp=0, tss=0))
        with self.assertRaises(COMMON.Stop) as raised:
            observer._entry(self.PC)
        self.assertIn('cannot establish stack ranges', str(raised.exception))
        self.assertIn('kernel_rsp', observer.memory_failure)

    def test_snapshot_chains_retained_then_memory_and_uses_only_reads(self):
        observer = self.observer()
        observer._entry(self.PC)
        result = observer._snapshot('initial-prompt')
        self.assertEqual(result, 'retained-result')
        self.assertEqual(observer.base_snapshots, ['initial-prompt'])
        record = json.loads((self.work / 'evidence/memory-00-initial-prompt.json').read_text())
        self.assertTrue(record['ok'] and record['all_stack_depths_exact'])
        self.assertEqual(set(observer.client.calls), {'read_physical_memory', 'read_memory'})
        self.assertEqual(record['heap_arena']['label'], 'heap-arena')
        self.assertEqual(record['provenance']['kernel_features'], ENV.provenance('cand-headless')['kernel_features'])

    def test_known_answer_depth_after_stack_use(self):
        guest = build()
        guest.write(UTOP - 0x1800, b'\x01' * 16)                    # user stack reaches 0x1800 bytes
        guest.write(KTOP - 0x2100, b'\x02' * 16)                    # kernel stack reaches 0x2100 bytes
        observer = self.observer(guest)
        observer._entry(self.PC)
        guest.write(UTOP - 0x2800, b'\x03')                         # grows after the entry stop
        observer._snapshot('complete')
        record = json.loads((self.work / 'evidence/memory-00-complete.json').read_text())
        self.assertEqual(record['stack_depth_bytes'], {'user': 0x2800, 'kernel': 0x2100})

    def test_disarm_request_is_not_a_memory_snapshot(self):
        observer = self.observer()
        observer._entry(self.PC)
        observer._snapshot('\0disarm')
        self.assertEqual(observer.memory_records, [])

    def test_snapshot_failure_is_retained_and_stops(self):
        guest = build()
        observer = self.observer(guest)
        observer._entry(self.PC)
        guest.unreadable.add(UTOP - 8 * PAGE)
        with self.assertRaises(COMMON.Stop):
            observer._snapshot('finite-stream')
        record = json.loads((self.work / 'evidence/memory-00-finite-stream.json').read_text())
        self.assertFalse(record['ok'])
        self.assertTrue(record['errors'])

    def test_snapshot_without_entry_record_stops(self):
        observer = self.observer()
        observer.focus = self.PC
        observer.instances[self.PC] = {}
        with self.assertRaises(COMMON.Stop):
            observer._snapshot('complete')

    def test_unproven_saturated_stack_is_flagged_not_exact(self):
        guest = build()
        observer = self.observer(guest)
        guest.write(KTOP - 5 * PAGE, b'\x01')                       # prime proof will fail for the kernel stack
        observer._entry(self.PC)
        guest.write(KTOP - 15 * PAGE, b'\x01')
        observer._snapshot('complete')
        record = json.loads((self.work / 'evidence/memory-00-complete.json').read_text())
        self.assertFalse(record['all_stack_depths_exact'])
        self.assertFalse(record['entry']['primed_status']['kernel-stack']['verified'])

    def test_write_results_summarises(self):
        observer = self.observer()
        observer._entry(self.PC)
        observer._snapshot('complete')
        observer.write_results()
        self.assertTrue(observer.written)
        summary = json.loads((self.work / 'evidence/memory-summary.json').read_text())
        self.assertEqual(summary['schema'], 'cellos.s10-7.memory-run-summary/1')
        self.assertEqual(len(summary['snapshots']), 1)


class RealObserverConstruction(unittest.TestCase):
    """The retained observers construct under the redirected retained paths (heap-instruction file, candidate ELF)."""

    def machine(self, tmp, profile):
        work = Path(tmp)
        (work / 'evidence').mkdir()
        (work / 'sock').mkdir()
        _record, files = ENV.candidate_files(profile)
        return types.SimpleNamespace(work=work, gdb_path=work / 'sock/gdb.sock', a=types.SimpleNamespace(profile=profile),
                                     inputs={'files': {'/bin/cellbox': {'file': files['/bin/cellbox']['file']}}})

    def test_plain_observer_headless(self):
        import s107_run as RUN
        with tempfile.TemporaryDirectory() as tmp:
            machine = self.machine(tmp, 'headless')
            observer = RUN.MemoryPlainObserver(machine, memory=RUN.memory_context(ENV.SETS['cand-headless']))
            observer.client.close()
            observer.events.close()
            observer.coverage.close()
            self.assertEqual(len(observer.updates), 2)
            self.assertIs(type(observer)._entry, RUN.MEM.MemoryMixin._entry)

    def test_hook_observer_desktop(self):
        import s107_run as RUN
        with tempfile.TemporaryDirectory() as tmp:
            machine = self.machine(tmp, 'desktop')
            observer = RUN.MemoryHookObserver(machine, object(), RUN.HO.load_static(), Path(tmp) / 'ledger.jsonl', memory=RUN.memory_context(ENV.SETS['cand-desktop']))
            observer.client.close()
            observer.ledger.close()
            observer.events.close()
            observer.coverage.close()
            self.assertEqual(RUN.MemoryHookObserver.SKIP_SNAPSHOT_NAMES, (RUN.HO.DISARM,))
            # chain order: memory mixin first, then the retained feed-hook observer; retained stop/step methods are inherited unchanged
            self.assertEqual(RUN.MemoryHookObserver.__mro__[1], RUN.MEM.MemoryMixin)
            self.assertIs(RUN.MemoryHookObserver._handle, RUN.HO.HookObserver._handle)
            self.assertIs(RUN.MemoryHookObserver._stopped, RUN.C.CoordinatedObserver._stopped)


class Provenance(unittest.TestCase):
    def test_every_set_records_test_hooks_and_the_measurement_kernel_hash(self):
        for name in ENV.SET_NAMES:
            provenance = ENV.provenance(name)
            self.assertIn('test-hooks', provenance['kernel_features'], name)
            self.assertEqual(provenance['kernel_sha256'], ENV.sha256_file(ENV.SETS[name].kernel), name)
            self.assertNotEqual(provenance['kernel_sha256'], provenance['control_kernel_non_hooks_sha256'])
            self.assertTrue(provenance['measurement_kernel_is_not_the_promoted_kernel'])

    def test_memory_context_refuses_wrong_provenance(self):
        spec = ENV.SETS['cand-headless']
        good = ENV.provenance(spec.name)
        base = dict(kernel_path=spec.kernel, heap_arena_rel=(0x2A8060, 6 * 1024 * 1024), user_pages=(16,), kernel_pages=(16,))
        MEM.MemoryContext(provenance=good, **base)
        with self.assertRaisesRegex(ValueError, 'test-hooks'):
            MEM.MemoryContext(provenance=dict(good, kernel_features=['default']), **base)
        with self.assertRaisesRegex(ValueError, 'sha256'):
            MEM.MemoryContext(provenance=dict(good, kernel_sha256='0' * 64), **base)
        with self.assertRaisesRegex(ValueError, 'sha256'):
            MEM.MemoryContext(provenance=dict(good, kernel_sha256=''), **base)
        control = dict(base, kernel_path=spec.control_kernel)
        with self.assertRaisesRegex(ValueError, 'not a test-hooks build|sha256'):
            MEM.MemoryContext(provenance=dict(good, kernel_sha256=ENV.sha256_file(spec.control_kernel)), **control)

    def test_memory_record_requires_test_hooks_provenance(self):
        view = build().view()
        with self.assertRaisesRegex(ValueError, 'test-hooks'):
            MEM.collect(view, [], phase='x', provenance={'kernel_build': 'k', 'profile': 'p', 'kernel_features': ['default']})

    def test_hook_kernel_symbol_present_only_in_hooks_builds(self):
        for name in ENV.SET_NAMES:
            spec = ENV.SETS[name]
            self.assertTrue(ENV.kernel_has_test_hooks(spec.kernel), name)
            self.assertFalse(ENV.kernel_has_test_hooks(spec.control_kernel), name)

    def test_set_files_match_the_test_hooks_manifest(self):
        for name in ENV.SET_NAMES:
            record = ENV.verify_set_files(name)
            self.assertTrue(record['all_match'], (name, record['match']))


class RecordSchema(unittest.TestCase):
    REQUIRED = {'schema', 'phase', 'instance', 'cr3', 'monotonic_ns', 'provenance', 'resident', 'stacks', 'stack_depth_bytes', 'heap_arena',
                'errors', 'ok', 'all_stack_depths_exact', 'limits'}

    def test_collect_schema_and_serialisability(self):
        guest = build()
        guest.map_identity_range(0x1_0000_0000, 0x1_0000_0000 + 3 * PAGE, fill=0)
        regions = MEM.regions_from_observer_instance(
            {'base': 0x1_0000_0000, 'stack_bottom': UTOP - 16 * PAGE, 'stack_top': UTOP}, [{'va': 0, 'memsz': 2 * PAGE, 'filesz': PAGE, 'flags': 5}],
            kernel_stack=(KTOP - 16 * PAGE, 16 * PAGE), heap_arena=(0x1_0000_1000, PAGE), heap_used_bytes=77)
        record = MEM.collect(guest.view(), regions, phase='p', provenance=ENV.provenance('cand-desktop'), instance=3,
                             primed_proofs={'user-stack': None, 'kernel-stack': None})
        self.assertEqual(record['schema'], 'cellos.s10-7.memory-snapshot/1')
        self.assertTrue(self.REQUIRED <= set(record))
        self.assertEqual({s['label'] for s in record['stacks']}, {'user-stack', 'kernel-stack'})
        self.assertEqual({r['kind'] for r in record['resident']['regions']}, {'elf-segment', 'user-stack', 'kernel-stack', 'heap-arena'})
        self.assertEqual(record['heap_arena']['extra']['allocator_used_bytes'], 77)
        self.assertFalse(record['all_stack_depths_exact'])             # no proofs given: honest
        json.dumps(record)

    def test_summarize_handles_candidate_and_b2_shapes(self):
        import s107_run as RUN
        guest = build()
        regions = MEM.regions_from_observer_instance({'base': 0x1_0000_0000, 'stack_bottom': UTOP - 16 * PAGE, 'stack_top': UTOP}, [],
                                                     kernel_stack=(KTOP - 16 * PAGE, 16 * PAGE))
        record = MEM.collect(guest.view(), regions, phase='a', provenance=ENV.provenance('cand-desktop'), instance=1)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'memory-00-a.json').write_text(json.dumps(record))
            (root / 'memory-01-b.json').write_text(json.dumps({'phase': 'b', 'records': [dict(record, phase='b', spawn={'name': 'shell'})], 'unobserved': []}))
            table = RUN.summarize_memory(root)
        self.assertEqual([(r['phase'], r['task']) for r in table], [('a', 1), ('b', 'shell')])


class B2Observer(unittest.TestCase):
    def test_b2_targets_are_distinct_hash_checked_and_windows_relocation_free(self):
        import s107_b2 as B2
        targets = B2.B2Targets()
        self.assertEqual([t['name'] for t in targets.targets], ['shell', 'ls', 'cat', 'echo', 'ps', 'kill'])
        for target in targets.targets:
            self.assertEqual(len(target['windows']), 3)
            self.assertTrue(all(len(w) == 48 for _va, w in target['windows']))
            self.assertTrue(target['heap_arena'] is None or target['heap_arena'][1] == 1024 * 1024, target['name'])
        self.assertIsNotNone(targets.targets[0]['heap_arena'])         # the persistent shell must have a discoverable arena

    def test_b2_match_by_three_windows(self):
        import s107_b2 as B2
        targets = B2.B2Targets()
        shell = targets.targets[0]
        memory = {}
        for va, window in shell['windows']:
            memory[0x1_0400_0000 + va] = window
        read = lambda addr, length: memory.get(addr, b'\0' * length)  # noqa: E731
        self.assertEqual(targets.match(read, 0x1_0400_0000)['name'], 'shell')
        memory[0x1_0400_0000 + shell['windows'][1][0]] = b'\xff' * 48
        self.assertIsNone(targets.match(read, 0x1_0400_0000))

    def test_b2_snapshot_document_states_allocator_not_measured(self):
        import s107_b2 as B2
        guest = build(frame_pc=0x1_0400_0000)
        guest.map_identity_range(0x1_0400_0000, 0x1_0400_0000 + 4 * PAGE, fill=0)
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            (work / 'evidence').mkdir()
            (work / 'sock').mkdir()
            machine = types.SimpleNamespace(work=work, gdb_path=work / 'sock/gdb.sock', a=types.SimpleNamespace(profile='headless'))
            targets = B2.B2Targets()
            ctx = context()
            ctx.provenance = ENV.provenance('b2-headless')
            observer = B2.B2MemoryObserver(machine, targets, ctx)
            observer.client = FakeClient(guest)
            observer.client.read_register = lambda name: {'cs': 0x23, 'cr3': guest.cr3, 'rsp': UTOP - 0x3F0}[name]
            observer.client.read_memory = lambda addr, length: dict((0x1_0400_0000 + va, w) for va, w in targets.targets[0]['windows']).get(addr, b'\0' * length)
            observer._b2_entry(0x1_0400_0000)
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            self.assertEqual(observer.focus, 0x1_0400_0000)
            shell = targets.targets[0]
            shell['loads'] = [{'va': 0, 'memsz': 2 * PAGE, 'filesz': PAGE, 'flags': 5}]
            shell['heap_arena'] = (0x2000, PAGE)
            observer._snapshot('b2-echo')
            document = json.loads((work / 'evidence/memory-00-b2-echo.json').read_text())
            self.assertIsNone(document['allocator_peak'])
            self.assertIn('not measured for B2', document['allocator_peak_reason'])
            self.assertEqual([r['spawn']['name'] for r in document['records']], ['shell'])
            self.assertIn('builtins', document['tools_not_spawned_note'])
            self.assertEqual(document['provenance']['set'], 'b2-headless')

    def test_b2_snapshot_without_shell_entry_stops(self):
        import s107_b2 as B2
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            (work / 'evidence').mkdir()
            (work / 'sock').mkdir()
            machine = types.SimpleNamespace(work=work, gdb_path=work / 'sock/gdb.sock', a=types.SimpleNamespace(profile='headless'))
            observer = B2.B2MemoryObserver(machine, B2.B2Targets(), context())
            self.addCleanup(observer.events.close)
            self.addCleanup(observer.coverage.close)
            with self.assertRaises(COMMON.Stop):
                observer._snapshot('complete')


class NoGuestWrites(unittest.TestCase):
    def test_wiring_sources_contain_no_guest_write_primitives(self):
        for name in ('s107_memory.py', 's107_b2.py', 's107_machine.py', 's107_run.py'):
            text = (HERE / name).read_text()
            for forbidden in ('write_memory', "request('M", 'request("M', 'Z0,', 'add_watchpoint', 'write_register', "request('P"):
                self.assertNotIn(forbidden, text, f'{name}: {forbidden}')


if __name__ == '__main__':
    unittest.main()
