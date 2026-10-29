"""S10-4.17 additive Terminal `ByteSink::feed` entry hook on the retained coordinated observer.

Prior sources are imported unchanged.  Hardware `Z1` only (the retained client allow-list),
read-only `p`/`m` packets, no guest write.  The Terminal hook state is NOT registered in
`observer.instances`/`entry_ordinal` (the retained lifecycle proof requires every instance
to own a heap watch).
"""
import json
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
RUNTIME = HERE.parent
sys.path.insert(0, str(HERE.parent / 'observer-gui-temporal'))
import gui_temporal as G
import observer as OBS
import hook_ledger as L

C, X, F, M = G.C, G.X, G.F, G.M
FEED_REL = 0x1da80
DISARM = '\0feed-hook-disarm'
STACK_WINDOW = 1 << 20


class HookStop(M.Stop):
    def __init__(self, code, reason):
        super().__init__(f'feed-hook {code}: {reason}')
        self.code, self.reason = code, reason


def load_static(path=HERE / 'static-identity.json'):
    static = json.loads(Path(path).read_text())
    X.need(static['terminal']['sha256'] == '816718a20188bb797a65622d17087010e8d41c7a933248dc303847c7d11ff3cc',
           'static identity is not for the qualified Terminal')
    X.need(static['callers']['call_count'] == 4 and static['feed']['va'] == FEED_REL and
           static['window_relocation_free'] is True and static['entry_fingerprints_differ'] is True,
           'static identity incomplete')
    return static


class Hook:
    def __init__(self, base, cr3, stack_top, static):
        self.base, self.cr3, self.stack_top = base, cr3, stack_top
        self.pc = base + FEED_REL
        self.armed = True
        self.rdi = self.rsi = None
        self.sites = {base + off: off for off in L.RET_STREAM}
        self.hits = 0


class HookObserver(C.CoordinatedObserver):
    def __init__(self, machine, gate, static, ledger_path):
        self.static = static
        self.hook = None
        self.terminal_entries = 0
        self.hook_failure = None
        self.identity_gate = None
        self.disarm_record = None
        self.ledger = L.Ledger(ledger_path)
        super().__init__(machine, gate)

    # ------------------------------------------------------------ failures
    def _fail(self, code, reason):
        self.hook_failure = {'code': code, 'reason': reason, 'ns': time.monotonic_ns()}
        raise HookStop(code, reason)

    # ------------------------------------------------------------ Terminal entry
    def _entry(self, pc):
        super()._entry(pc)
        if pc in self.instances:
            return
        self._terminal_entry(pc)

    def _terminal_entry(self, pc):
        want = bytes.fromhex(self.static['terminal_first48_hex'])
        try:
            got = self.client.read_memory(pc, 48)
        except OBS.RspError as error:
            raise M.Stop(f'entry fingerprint cannot be read: {error}')
        if got != want:
            return
        self.terminal_entries += 1
        if self.terminal_entries != 1 or self.hook is not None:
            self._fail('second-terminal-entry', f'entry {pc:#x}')
        if self.phase != 'terminal-launch':
            self._fail('terminal-phase', f'Terminal entry in phase {self.phase!r}')
        cs = self.client.read_register('cs')
        cr3 = self.client.read_register('cr3')
        sp = self.client.read_register('rsp')
        if cs & 3 != 3:
            self._fail('terminal-entry-cs', f'cs={cs:#x}')
        checks = []
        cache = {}
        for window in self.static['windows']:
            try:
                actual = self.client.read_memory(pc + window['va'], window['length'])
            except OBS.RspError as error:
                if not str(error).startswith('memory reply'):
                    raise
                self._fail('window-read-failed', f"{window['label']}: {error}")
            if actual != bytes.fromhex(window['hex']):
                self._fail('window-mismatch', window['label'])
            checks.append(window['label'])
        pages = {((pc + w['va']) & ~4095) for w in self.static['windows']}
        pages |= {((pc + w['va'] + w['length'] - 1) & ~4095) for w in self.static['windows']}
        for page in sorted(pages):
            pa, _witness = OBS.walk_page(page, cr3, self.client.read_physical_memory, cache)
            if pa is None:
                self._fail('text-page-unmapped', f'{page:#x}')
        top = (sp + 4095) & ~4095
        hook = Hook(pc, cr3, top, self.static)
        self.client.add_breakpoint(hook.pc)
        self.breaks.add(hook.pc)
        self.hook = hook
        self.identity_gate = {'entry': pc, 'hook_pc': hook.pc, 'cr3': cr3, 'cs': cs, 'initial_rsp': sp,
                              'stack_top': top, 'windows_checked': checks, 'pages': sorted(pages),
                              'phase': self.phase, 'ns': time.monotonic_ns(), 'terminal_entry_count': 1,
                              'z1_installed': f'Z1,{hook.pc:x},1'}
        X.dump(self.m.work / 'evidence/feed-hook-identity.json', self.identity_gate)

    # ------------------------------------------------------------ stop handling
    def _handle(self, reply):
        client = self.client
        real = client.read_register
        cache = {}

        def cached(name):
            if name == 'rip' and 'rip' in cache:
                return cache['rip']
            value = real(name)
            if name == 'rip':
                cache['rip'] = value
            return value

        client.read_register = cached
        try:
            pc = cached('rip')
            hook = self.hook
            if hook is not None and hook.armed and pc == hook.pc and b'watch:' not in reply:
                return self._feed_hit(hook, reply, real)
            return super()._handle(reply)
        finally:
            del client.read_register

    def _feed_hit(self, hook, reply, read):
        ns = time.monotonic_ns()
        rsp, rdi, rsi, rdx, rcx = (read(name) for name in ('rsp', 'rdi', 'rsi', 'rdx', 'rcx'))
        cr3, cs = read('cr3'), read('cs')
        if cs & 3 != 3:
            self._fail('hit-cs', f'cs={cs:#x}')
        if cr3 != hook.cr3:
            self._fail('hit-cr3', f'{cr3:#x} != {hook.cr3:#x}')
        if not hook.stack_top - STACK_WINDOW <= rsp < hook.stack_top:
            self._fail('hit-rsp-window', f'rsp={rsp:#x}')
        if hook.rdi is None:
            hook.rdi, hook.rsi = rdi, rsi
        if (rdi, rsi) != (hook.rdi, hook.rsi):
            self._fail('hit-pinned-sink-grid', f'rdi={rdi:#x} rsi={rsi:#x}')
        if not 1 <= rcx <= 0x200:
            self._fail('hit-length', f'rcx={rcx}')
        if rdx != rsp + 0x20:
            self._fail('hit-buffer-address', f'rdx={rdx:#x} rsp={rsp:#x}')
        try:
            ret = int.from_bytes(self.client.read_memory(rsp, 8), 'little')
            offset = ret - hook.base
            if offset not in L.RET_STREAM:
                self._fail('hit-return-site', f'return {ret:#x} offset {offset:#x}')
            data = self.client.read_memory(rdx, rcx)
        except OBS.RspError as error:
            if not str(error).startswith('memory reply'):
                raise
            self._fail('hit-memory-reply', str(error))
        if len(data) != rcx:
            self._fail('hit-payload', 'short payload')
        hook.hits += 1
        record = L.hit_record(hook.hits, ns, rsp, offset, rcx, data, cr3, rdi, rsi)
        try:
            self.ledger.append(record)
        except L.LedgerViolation as error:
            self._fail(error.code, error.reason)

    # ------------------------------------------------------------ disarm (request queue at stop)
    def _snapshot(self, name):
        if name == DISARM:
            return self._disarm()
        return super()._snapshot(name)

    def disarm_hook(self):
        self.snapshot(DISARM)
        return self.disarm_record

    def scope_census(self):
        hook = self.hook
        steps = [s for s in self.client.control.steps if s.get('addr') == hook.pc]
        good = [s for s in steps if 'error' not in s and 'capture_error' not in s]
        with_entry = [s for s in good if any(
            e['text'].endswith(f' pc={hook.pc:#x}') for e in s.get('trace', {}).get('tb_entries', []))]
        return {'scopes_at_hook': len(steps), 'successful_scopes': len(good), 'successful_with_hook_tb_entry': len(with_entry),
                'failed_primary_scopes': len(steps) - len(good)}

    def _disarm(self):
        hook = self.hook
        if hook is None or not hook.armed:
            self._fail('disarm-state', 'hook not armed')
        census = self.scope_census()
        hits = self.ledger.count()
        record = {'ns': time.monotonic_ns(), 'ledger_hits': hits, 'ledger_bytes': self.ledger.total_bytes(),
                  'hook_hits': hook.hits, 'scopes': census, 'hook_pc': hook.pc}
        self.disarm_record = record
        if not (hits == hook.hits == census['successful_scopes'] == census['successful_with_hook_tb_entry']):
            record['count_mismatch'] = True
            self._fail('count-mismatch', json.dumps(record))
        before = sorted(self.client._hardware_breakpoints)
        self.client.remove_breakpoint(hook.pc)
        self.breaks.discard(hook.pc)
        hook.armed = False
        record['breakpoint_census_before'] = len(before)
        record['breakpoint_census_after'] = len(self.client._hardware_breakpoints)
        X.need(record['breakpoint_census_after'] == record['breakpoint_census_before'] - 1 and
               (hook.pc, 1) not in self.client._hardware_breakpoints, 'disarm census mismatch')
        X.dump(self.m.work / 'evidence/feed-hook-disarm.json', record)

    def write_results(self):
        super().write_results()
        try:
            self.ledger.close()
        except Exception:
            pass
