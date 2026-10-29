"""T2 Terminal entry identity, T3 per-hit invariants, T7/T8 integration on retained data (no VM/QEMU/RSP socket)."""
import copy
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hook_ledger as L
import hook_observer as HO
import hookfix as H

X, OBS = HO.X, HO.OBS
STATIC = HO.load_static()
ELF = Path(STATIC['terminal']['path']).read_bytes()
TRAFFIC = HERE.parent / 'observer-gui-temporal/desktop/evidence/rsp-traffic.jsonl'
MASK = 0x000ffffffffff000
BASE = 0x116000000
CR3 = 0x59000


class FakeClient:
    """Minimal stand-in for the retained RSP client: sparse virtual memory, registers, page tables."""

    def __init__(self):
        self.memory = {}  # va -> bytes segment
        self.regs = {'cs': 0x23, 'cr3': CR3}
        self.tables = {}
        self.log = []
        self._hardware_breakpoints = set()
        self.control = SimpleNamespace(steps=[])

    def map(self, va, data):
        self.memory[va] = bytes(data)

    def read_memory(self, addr, length):
        self.log.append(('m', addr, length))
        for start, data in self.memory.items():
            if start <= addr and addr + length <= start + len(data):
                return data[addr - start:addr - start + length]
        raise OBS.RspError(f'memory reply is not exact-length hex (unmapped {addr:#x})')

    def read_register(self, name):
        self.log.append(('p', name))
        return self.regs[name]

    def read_physical_memory(self, addr, length):
        self.log.append(('phys', addr))
        return bytes(self.tables[addr])

    def add_breakpoint(self, addr, length=1):
        self.log.append(('Z1', addr))
        self._hardware_breakpoints.add((addr, length))

    def remove_breakpoint(self, addr, length=1):
        self.log.append(('z1', addr))
        self._hardware_breakpoints.discard((addr, length))

    def map_pages(self, vas):
        counter = [0x100000]

        def table():
            counter[0] += 0x1000
            self.tables[counter[0]] = bytearray(4096)
            return counter[0]

        root = CR3
        self.tables[root] = bytearray(4096)
        for va in vas:
            current = root
            for shift in (39, 30, 21):
                idx = (va >> shift) & 511
                entry = struct.unpack_from('<Q', self.tables[current], idx * 8)[0]
                if not entry & 1:
                    child = table()
                    struct.pack_into('<Q', self.tables[current], idx * 8, child | 7)
                    entry = child | 7
                current = entry & MASK
            struct.pack_into('<Q', self.tables[current], ((va >> 12) & 511) * 8, (0x900000 + (va >> 12 & 0xfff) * 0x1000) | 7)


def make_observer(directory, client=None, phase='terminal-launch'):
    work = Path(directory)
    (work / 'evidence').mkdir(parents=True, exist_ok=True)
    observer = object.__new__(HO.HookObserver)
    observer.static = STATIC
    observer.hook = None
    observer.terminal_entries = 0
    observer.hook_failure = None
    observer.identity_gate = None
    observer.disarm_record = None
    observer.ledger = L.Ledger()
    observer.m = SimpleNamespace(work=work)
    observer.phase = phase
    observer.client = client or FakeClient()
    observer.breaks = set()
    observer.instances = {}
    observer.entry_ordinal = 0
    observer.watches = set()
    observer.focus = None
    return observer


def terminal_client(base=BASE):
    client = FakeClient()
    client.map(base, ELF[4096:4096 + 192732])  # first LOAD (R+X): ELF va 0 at the slot base
    client.regs['rsp'] = 0x3c7b000
    pages = set()
    for window in STATIC['windows']:
        pages.add((base + window['va']) & ~4095)
        pages.add((base + window['va'] + window['length'] - 1) & ~4095)
    client.map_pages(pages)
    return client


def retained_entry_reads():
    """The 17 slot-entry `m<base>,30` requests and replies retained from the S10-4.15 desktop run."""
    pending, found = {}, []
    for line in TRAFFIC.read_text().splitlines():
        event = json.loads(line)
        if event['event'] == 'packet_tx':
            text = bytes.fromhex(event['payload_hex']).decode('ascii', 'replace')
            match = re.fullmatch(r'm([0-9a-f]+),30', text)
            slot = match and int(match[1], 16) >= 0x100000000 and (int(match[1], 16) - 0x100000000) % (32 << 20) == 0
            pending = {'addr': int(match[1], 16)} if slot else {}
        elif event['event'] == 'packet_rx' and pending:
            found.append((pending['addr'], bytes.fromhex(bytes.fromhex(event['payload_hex']).decode())))
            pending = {}
    return found


class TerminalEntryIdentity(unittest.TestCase):
    def test_retained_traffic_selects_exactly_the_one_terminal_entry(self):
        reads = retained_entry_reads()
        # 17 slot-entry stops; the 21 retained 48-byte reads also include 4 extra Cellbox fingerprint windows.
        self.assertEqual(len(reads), 17)
        self.assertEqual(sum(r == bytes.fromhex(STATIC['cellbox_first48_hex']) for _a, r in reads), 2)
        self.assertEqual(sum(r == bytes.fromhex(STATIC['terminal_first48_hex']) for _a, r in reads), 1)
        selected = []
        for base, reply in reads:
            with tempfile.TemporaryDirectory() as directory:
                client = terminal_client(base)
                client.map(base, reply)  # the live first window exactly as retained (overrides ELF prefix)
                client.memory[base] = ELF[4096:4096 + 192732] if reply == bytes.fromhex(STATIC['terminal_first48_hex']) else reply
                observer = make_observer(directory, client)
                observer._terminal_entry(base)
                if observer.hook:
                    selected.append((base, observer.hook.pc))
                    self.assertEqual(observer.hook_failure, None)
                    self.assertIn(('Z1', base + 0x1da80), client.log)
                else:
                    self.assertFalse([e for e in client.log if e[0] == 'Z1'])
        self.assertEqual(selected, [(BASE, BASE + 0x1da80)])
        self.assertEqual(hex(selected[0][1]), '0x11601da80')

    def test_install_records_identity_and_never_touches_cellbox_lifecycle(self):
        with tempfile.TemporaryDirectory() as directory:
            observer = make_observer(directory, terminal_client())
            observer._terminal_entry(BASE)
            hook = observer.hook
            self.assertEqual((hook.pc, hook.cr3, hook.stack_top), (BASE + 0x1da80, CR3, 0x3c7b000))
            self.assertEqual((observer.instances, observer.entry_ordinal, observer.focus), ({}, 0, None))
            self.assertIn(hook.pc, observer.breaks)
            self.assertEqual([e for e in observer.client.log if e[0] == 'Z1'], [('Z1', hook.pc)])
            identity = json.loads((Path(directory) / 'evidence/feed-hook-identity.json').read_text())
            self.assertEqual(identity['z1_installed'], 'Z1,11601da80,1')
            self.assertEqual(len(identity['windows_checked']), 7)

    def expect_stop(self, observer, code):
        with self.assertRaises(HO.HookStop) as raised:
            observer._terminal_entry(BASE)
        self.assertEqual(raised.exception.code, code)
        self.assertEqual(observer.hook_failure['code'], code)
        self.assertIsNone(observer.hook)
        self.assertFalse([e for e in observer.client.log if e[0] == 'Z1'])

    def test_failures_arm_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            self.expect_stop(make_observer(directory, terminal_client(), phase='boot'), 'terminal-phase')
        with tempfile.TemporaryDirectory() as directory:
            client = terminal_client()
            client.regs['cs'] = 0x10
            self.expect_stop(make_observer(directory, client), 'terminal-entry-cs')
        for window in STATIC['windows'][1:]:
            with self.subTest(window['label']), tempfile.TemporaryDirectory() as directory:
                client = terminal_client()
                data = bytearray(client.memory[BASE])
                data[window['va'] + window['length'] // 2] ^= 0xff
                client.memory[BASE] = bytes(data)
                self.expect_stop(make_observer(directory, client), 'window-mismatch')
        with self.subTest('unmapped page'), tempfile.TemporaryDirectory() as directory:
            client = terminal_client()
            client.tables.clear()
            client.tables[CR3] = bytearray(4096)
            self.expect_stop(make_observer(directory, client), 'text-page-unmapped')

    def test_unreadable_text_window_is_a_hook_stop_not_a_host_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            client = terminal_client()
            client.memory[BASE] = client.memory[BASE][:0x1da80]  # text after the entry window unreadable
            self.expect_stop(make_observer(directory, client), 'window-read-failed')

    def test_second_terminal_entry_is_a_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            observer = make_observer(directory, terminal_client())
            observer._terminal_entry(BASE)
            with self.assertRaises(HO.HookStop) as raised:
                observer._terminal_entry(BASE)
            self.assertEqual(raised.exception.code, 'second-terminal-entry')

    def test_cellbox_and_other_cell_fingerprints_are_ignored(self):
        cellbox = bytes.fromhex(STATIC['cellbox_first48_hex'])
        with tempfile.TemporaryDirectory() as directory:
            client = FakeClient()
            client.map(BASE, cellbox)
            observer = make_observer(directory, client)
            observer._terminal_entry(BASE)
            self.assertIsNone(observer.hook)
            self.assertEqual(observer.terminal_entries, 0)
            self.assertEqual(client.log, [('m', BASE, 48)])

    def test_static_identity_is_required_and_bound_to_qualified_terminal(self):
        bad = copy.deepcopy(STATIC)
        bad['terminal']['sha256'] = '0' * 64
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 's.json'
            path.write_text(json.dumps(bad))
            with self.assertRaises(HO.M.Stop):
                HO.load_static(path)


class HitInvariants(unittest.TestCase):
    RSP = 0x3c7aef0

    def setup_hit(self, directory, data=b'hello terminal', site=0x14b, **override):
        client = FakeClient()
        base = BASE
        hook = HO.Hook(base, CR3, 0x3c7b000, STATIC)
        observer = make_observer(directory, client)
        observer.hook = hook
        rsp = override.get('rsp', self.RSP)
        buffer = override.get('rdx', rsp + 0x20)
        client.regs.update(rsp=rsp, rdi=override.get('rdi', 0x3c60000), rsi=override.get('rsi', 0x3c60100),
                           rdx=buffer, rcx=override.get('rcx', len(data)), cr3=override.get('cr3', CR3),
                           cs=override.get('cs', 0x23))
        client.map(rsp, (base + override.get('site', site)).to_bytes(8, 'little') + b'\0' * 24 + data + b'\0' * 64)
        client.map(buffer, data) if override.get('rdx') else None
        return observer, hook, client

    def hit(self, observer, hook, client):
        return observer._feed_hit(hook, b'T05thread:01;', client.read_register)

    def test_good_hit_records_bytes_stream_and_pins_sink_grid(self):
        with tempfile.TemporaryDirectory() as directory:
            observer, hook, client = self.setup_hit(directory)
            self.hit(observer, hook, client)
            self.hit(observer, hook, client)
            hits = observer.ledger.snapshot()
            self.assertEqual([h['seq'] for h in hits], [1, 2])
            self.assertEqual(bytes.fromhex(hits[0]['data_hex']), b'hello terminal')
            self.assertEqual((hits[0]['stream'], hits[0]['ret_off'], hits[0]['rsp']), ('stdout', 0x14b, self.RSP))
            self.assertEqual((hook.rdi, hook.rsi, hook.hits), (0x3c60000, 0x3c60100, 2))

    def test_stderr_site_is_tagged(self):
        with tempfile.TemporaryDirectory() as directory:
            observer, hook, client = self.setup_hit(directory, site=0x1e5)
            self.hit(observer, hook, client)
            self.assertEqual(observer.ledger.snapshot()[0]['stream'], 'stderr')

    def test_every_invariant_violation_stops_with_its_code(self):
        cases = {
            'hit-cs': dict(cs=0x10), 'hit-cr3': dict(cr3=0x1234000), 'hit-rsp-window': dict(rsp=0x100),
            'hit-length': dict(rcx=0), 'hit-length ': dict(rcx=513),
            'hit-buffer-address': dict(rdx=0x3c7aef0 + 0x28), 'hit-return-site': dict(site=0x777),
        }
        for code, override in cases.items():
            with self.subTest(code), tempfile.TemporaryDirectory() as directory:
                observer, hook, client = self.setup_hit(directory, **override)
                if 'rdx' in override:
                    client.map(override['rdx'], b'hello terminal')
                with self.assertRaises(HO.HookStop) as raised:
                    self.hit(observer, hook, client)
                self.assertEqual(raised.exception.code, code.strip())
                self.assertEqual(observer.hook_failure['code'], code.strip())
                self.assertEqual(observer.ledger.count(), 0)

    def test_pinned_sink_or_grid_drift_stops(self):
        for register in ('rdi', 'rsi'):
            with self.subTest(register), tempfile.TemporaryDirectory() as directory:
                observer, hook, client = self.setup_hit(directory)
                self.hit(observer, hook, client)
                client.regs[register] += 0x40
                with self.assertRaises(HO.HookStop) as raised:
                    self.hit(observer, hook, client)
                self.assertEqual(raised.exception.code, 'hit-pinned-sink-grid')
                self.assertEqual(observer.ledger.count(), 1)

    def test_ledger_violation_becomes_hook_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            observer, hook, client = self.setup_hit(directory, site=0x1a5)  # loop site without burst predecessor
            with self.assertRaises(HO.HookStop) as raised:
                self.hit(observer, hook, client)
            self.assertEqual(raised.exception.code, 'drain-grammar')

    def test_page_crossing_512_byte_read_is_exact(self):
        data = bytes(range(256)) * 2
        with tempfile.TemporaryDirectory() as directory:
            observer, hook, client = self.setup_hit(directory, data=data)
            self.hit(observer, hook, client)
            hit = observer.ledger.snapshot()[0]
            self.assertEqual(bytes.fromhex(hit['data_hex']), data)
            self.assertEqual(hit['length'], 512)

    def test_unreadable_payload_or_return_slot_is_a_hook_stop(self):
        for override in ({}, {'ret': True}):
            with self.subTest(override), tempfile.TemporaryDirectory() as directory:
                observer, hook, client = self.setup_hit(directory)
                client.memory.clear()
                if override:
                    client.map(self.RSP, (BASE + 0x14b).to_bytes(8, 'little'))
                with self.assertRaises(HO.HookStop) as raised:
                    self.hit(observer, hook, client)
                self.assertEqual(raised.exception.code, 'hit-memory-reply')

    def test_real_client_rejects_short_long_nonhex_and_error_memory_replies(self):
        for reply in (b'', b'00', b'zz' * 4, b'E14', b'00' * 9):
            with self.subTest(reply=reply):
                client = OBS.RspClient.__new__(OBS.RspClient)
                client._lock = __import__('threading').RLock()
                client._packet_size = 4096
                client.request = lambda command, reply=reply: reply
                with self.assertRaises(OBS.RspError):
                    client.read_memory(0x1000, 8)


class HandlerDelegationAndDisarm(unittest.TestCase):
    def test_cellbox_stops_use_identical_register_reads_to_the_retained_handler(self):
        """The wrapper must not add a single packet for non-hook stops (rip cached, not re-read)."""
        for reply in (b'T05thread:01;', b'T05thread:01;watch:114a80c0;'):
            logs = {}
            for label, cls in (('retained', OBS.Observer), ('hook', HO.HookObserver)):
                with tempfile.TemporaryDirectory() as directory:
                    observer = make_observer(directory, FakeClient())
                    observer.__class__ = cls
                    observer.client.regs['rip'] = 0x2000000 + 0x1234
                    observer.instances = {}
                    observer.writer_sites = observer.writer_next = observer.posts = set()
                    observer.updates = {}
                    observer.draw_rel = observer.flush_rel = 0
                    try:
                        observer._handle(reply)
                    except HO.M.Stop as error:
                        logs[label] = (type(error).__name__, str(error)[:30], list(observer.client.log))
            self.assertEqual(logs['retained'], logs['hook'], reply)
            self.assertEqual([e for e in logs['hook'][2] if e == ('p', 'rip')], [('p', 'rip')])

    def test_hook_stop_is_handled_with_one_rip_read_and_restores_client(self):
        with tempfile.TemporaryDirectory() as directory:
            observer, hook, client = HitInvariants().setup_hit(directory)
            observer.__class__ = HO.HookObserver
            client.regs['rip'] = hook.pc
            observer._handle(b'T05thread:01;')
            self.assertEqual([e for e in client.log if e == ('p', 'rip')], [('p', 'rip')])
            self.assertNotIn('read_register', vars(client))
            self.assertEqual(observer.ledger.count(), 1)

    def test_watch_stop_at_hook_pc_is_not_a_hook_hit(self):
        with tempfile.TemporaryDirectory() as directory:
            observer, hook, client = HitInvariants().setup_hit(directory)
            client.regs['rip'] = hook.pc
            observer.updates, observer.instances, observer.writer_sites, observer.writer_next = {}, {}, set(), set()
            with self.assertRaises(HO.M.Stop):
                observer._handle(b'T05thread:01;watch:1000;')
            self.assertEqual(observer.ledger.count(), 0)

    def test_disarm_requires_three_way_count_and_removes_exactly_the_hook(self):
        with tempfile.TemporaryDirectory() as directory:
            observer, hook, client = HitInvariants().setup_hit(directory)
            client.regs['rip'] = hook.pc
            client.add_breakpoint(hook.pc)
            client.add_breakpoint(0x100000000)
            observer.breaks = {hook.pc, 0x100000000}
            for _ in range(3):
                observer._handle(b'T05thread:01;')
            scope = lambda: {'addr': hook.pc, 'trace': {'tb_entries': [{'text': f'tb:0x7f pc={hook.pc:#x}'}]}}  # noqa: E731
            client.control.steps = [scope(), scope()]
            with self.assertRaises(HO.HookStop) as raised:
                observer._disarm()
            self.assertEqual(raised.exception.code, 'count-mismatch')
            self.assertTrue(hook.armed)
            client.control.steps.append(scope())
            client.control.steps.append({'addr': hook.pc, 'error': 'RspError: x'})  # failed primary is not a success
            observer._disarm()
            self.assertFalse(hook.armed)
            self.assertEqual(client._hardware_breakpoints, {(0x100000000, 1)})
            self.assertNotIn(hook.pc, observer.breaks)
            self.assertEqual(observer.disarm_record['scopes']['failed_primary_scopes'], 1)
            with self.assertRaises(HO.HookStop):
                observer._disarm()


class RecoveryAtHookPc(unittest.TestCase):
    """The unchanged retained admission proof treats the hook PC like any enrolled Z1 site."""

    def fixture(self, include_terminal_instance=False):
        import amendment as A
        hook_pc = BASE + 0x1da80
        pc = f'{hook_pc:x}'
        lines = [('gdbstub_io_command', 'Received: p2', 11), ('gdbstub_io_reply', 'Sent: 0010000000000000', 11),
                 ('gdbstub_io_command', f'Received: z1,{pc},1', 11), ('gdbstub_io_reply', 'Sent: OK', 11),
                 ('gdbstub_io_command', 'Received: s', 11), ('breakpoint_singlestep', 'cpu=0 enable=7', 11),
                 ('exec_tb', f'tb:0xab00 pc={hook_pc:#x}', 22), ('exec_tb_exit', 'tb:0xab00 flags=0x3', 22),
                 ('gdbstub_hit_break', 'RUN_STATE_DEBUG', 11), ('gdbstub_io_reply', 'Sent: T05thread:01;', 11),
                 ('breakpoint_singlestep', 'cpu=0 enable=0', 11), ('gdbstub_io_command', 'Received: p2', 11),
                 ('gdbstub_io_reply', 'Sent: 0010000000000000', 11),
                 ('gdbstub_io_command', f'Received: Z1,{pc},1', 11), ('gdbstub_io_reply', 'Sent: OK', 11)]
        raw = ''.join(f'{tid}@123.{n:06d}:{event} {text}\n' for n, (event, text, tid) in enumerate(lines, 1)).encode()
        parsed = A.X.trace_interval(raw)
        parsed.update(raw_sha256=hashlib.sha256(raw).hexdigest(), offset=0, end_offset=len(raw),
                      enabled_before_after_acknowledged=True)
        bytes16 = '55' + '90' * 15

        def ctx(stop='T05thread:01;'):
            return {'rip': hook_pc, 'eflags': 0x202, 'cs': 0x23, 'cr3': CR3, 'rsp': 0x3c7aef0,
                    'instruction_read_address': hook_pc, 'instruction_bytes': bytes16, 'sstepbits': 'ENABLE=1,NOIRQ=2,NOTIMER=4',
                    'sstep': '0x7', 'tf': False, 'rf': False, 'stop': stop}
        cpus = {'cpus': [{'cpu-index': 0, 'target': 'x86_64', 'thread-id': 22}],
                'status': {'running': False, 'status': 'debug', 'singlestep': False}}
        breakpoints = [(hook_pc, 1), (0x114000000 + 0x1234, 1), (0x100000000, 1)]
        first = {'ordinal': 5, 'addr': hook_pc, 'before': ctx(), 'after': ctx(), 'qmp_before': cpus, 'qmp_after': cpus,
                 'breakpoints_before': breakpoints, 'breakpoints_after': breakpoints,
                 'watches_before': [(0x1148a80c0, 8)], 'watches_after': [(0x1148a80c0, 8)],
                 'error': f'RspError: {A.X.NO_PROGRESS}', 'trace': parsed}
        instances = [0x114000000] + ([BASE] if include_terminal_instance else [])
        live = {'phase': 'finite-stream', 'verified_instance_count': len(instances), 'instances': instances,
                'entry_ordinal': 1, 'focus': 0x114000000, 'heap_watches': [(0x1148a80c0, 8)]}
        health = {'same_binary': True, 'source_exact': True, 'successful_scopes': 2044, 'evidence_sha256': {'x': 'a' * 64}}
        return A, (first, ctx(), cpus, set(breakpoints), {(0x1148a80c0, 8)}, raw, health, True, live, copy.deepcopy(live))

    def test_zero_retirement_at_the_hook_pc_is_admitted_by_the_unchanged_proof(self):
        A, args = self.fixture()
        proof = A.admit(*args)
        self.assertTrue(proof['admitted'])
        self.assertEqual((proof['site'], proof['exit_flag']), (BASE + 0x1da80, 3))

    def test_registering_the_terminal_in_instances_would_break_the_lifecycle_proof(self):
        A, args = self.fixture(include_terminal_instance=True)
        with self.assertRaises(HO.M.Stop):
            A.admit(*args)

    def test_missing_hook_in_census_is_rejected(self):
        A, args = self.fixture()
        bad = list(args)
        bad[3] = {b for b in args[3] if b[0] != BASE + 0x1da80}
        with self.assertRaises(HO.M.Stop):
            A.admit(*bad)

    def test_hook_census_stages(self):
        entries = {(0x100000000 + slot * 32 * 1024 * 1024, 1) for slot in range(512)}
        self.assertEqual(len(entries), 512)
        installed = entries | {(BASE + 0x1da80, 1)}
        self.assertEqual(len(installed - entries), 1)
        self.assertEqual(installed - {(BASE + 0x1da80, 1)}, entries)


if __name__ == '__main__':
    unittest.main()
