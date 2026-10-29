"""S10-9b B2 desktop: task identification, coordinated memory observer, measurement machine and the exact-screen GUI surface.

Everything retained is IMPORTED unchanged: the retained observer transport/stop machinery (`observer.Observer`), the S10-4.13
coordinated layer (`InputGate`, `CoordinatedObserver`, `CoordinatedSurface`), the S10-4.15 `TemporalDesk`, the S10-7 `B2Targets`/
`B2MemoryObserver` fingerprint + memory approach, the S10-7 `MeasurementMachine` and the S10-7.2 memory observer.  New here:

* `B2DesktopTargets`: the two B2 desktop tasks (`terminal` = the GUI shell analogue/focus, `shell` = the autoloaded serial shell)
  identified by three relocation-free 48-byte .text windows against the signed ELFs of the v2 image, hash-pinned against the
  v2 `static-verification.json` file table.
* `B2DesktopObserver`: `B2MemoryObserver` entry/snapshot behaviour (read-only RSP `p`/`m` packets, hardware `Z1`, no guest write)
  on the coordinated layer so that GUI input and the stops cooperate exactly as in the retained desktop runs.
* `B2DesktopMachine`: the retained S10-7 `MeasurementMachine` for the desktop profile, no NVMe data disk, private ISO copy.
* `B2DesktopSurface`: the retained desktop launch/oracle sequence with the B2 terminal's own (source-literal) expected rows and the
  shared-subset commands judged by the retained `Desk.command` (exact typed line, exact output screen, negative controls).
"""
import json
import shutil
import sys
import threading
import time
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s109b_env as ENV  # noqa: E402
import s107_retained as RET  # noqa: E402  (puts every retained archive on sys.path)
import s107_machine as MACH  # noqa: E402
import s107_memory as MEM  # noqa: E402
import s107_b2 as B2  # noqa: E402

import common as X  # noqa: E402  (retained runtime/common.py: Stop, need, dump, D, Machine, Surface; same module s107_b2/s107_machine use)
HS, C, G, M = RET.HS, RET.C, RET.G, RET.M
OBS = RET.OBS
D = X.D

TOOLS = (('terminal', '/bin/terminal', 'terminal', 'b2-terminal'), ('shell', '/bin/shell', 'app-shell', 'b2-shell'))
FOCUS = 'terminal'
SNAPSHOT_FOCUS_DEPTH = ('user', 'kernel')


# ------------------------------------------------------------------ B2 terminal expected screens (source literals)
BANNER = ('=== CellOS GUI Terminal (x86_64) ===', "Type 'help' for commands. Single-reader stream lease active.")
PROMPT = D.PROMPT
INITIAL_ROWS = [BANNER[0], BANNER[1], '', PROMPT]
INITIAL_CURSOR = (len(PROMPT), 3)


def heap_arena_symbol(elf_path, *, kind):
    """(relative VA, size) of the task's static heap arena: B2 shell `HEAP_MEM` (retained helper) or the B2 terminal's custom arena."""
    if kind == 'b2-shell':
        return MEM.heap_arena_symbol(elf_path, kind=kind)
    if kind != 'b2-terminal':
        raise MEM.DiscoveryError(f'unknown heap arena kind {kind!r}')
    try:
        symbol = ENV.unique_symbol(ENV.elf_symbols(elf_path), b'8terminal', b'19___CUSTOM_HEAP_ARENA', size=4 * 1024 * 1024,
                                   what='B2 terminal heap arena')
    except ENV.EnvError as error:
        raise MEM.DiscoveryError(f'heap arena not discoverable in {elf_path}: {error}') from error
    if not any(seg['va'] <= symbol['value'] and symbol['value'] + symbol['size'] <= seg['va'] + seg['memsz'] for seg in ENV.elf_load_segments(elf_path)):
        raise MEM.DiscoveryError(f'heap arena {symbol["value"]:#x}+{symbol["size"]:#x} is outside every PT_LOAD segment')
    return symbol['value'], symbol['size']


class B2DesktopTargets(B2.B2Targets):
    """Terminal + shell fingerprints (same three-window scheme as `B2Targets`, desktop image table)."""

    def __init__(self, signed_dir=None, static=None):
        signed_dir = Path(signed_dir) if signed_dir is not None else ENV.SPEC.signed
        table = (static if static is not None else ENV.static_verification())['image']['files']
        self.targets = []
        for name, image_path, signed, arena_kind in TOOLS:
            path = signed_dir / signed
            digest = ENV.sha256_file(path)
            if digest != table[image_path]['sha256']:
                raise MEM.DiscoveryError(f'{path} sha256 differs from the B2 desktop v2 image record for {image_path}')
            _data, _sections, loads = OBS.elf_layout(path)
            try:
                arena = heap_arena_symbol(path, kind=arena_kind)
            except MEM.DiscoveryError:
                arena = None      # no discoverable arena symbol: region omitted, recorded as null
            self.targets.append({'name': name, 'path': str(path), 'sha256': digest, 'windows': B2.fingerprint_windows(path),
                                 'loads': loads, 'heap_arena': arena, 'image_path': image_path})
        seen = {}
        for target in self.targets:
            key = tuple(window for _va, window in target['windows'])
            if key in seen:
                raise MEM.DiscoveryError(f'B2 desktop task fingerprints are not distinct: {target["name"]} == {seen[key]}')
            seen[key] = target['name']


# ------------------------------------------------------------------ observer
class B2DesktopObserver(B2.B2MemoryObserver, C.CoordinatedObserver):
    """MRO: B2DesktopObserver -> B2MemoryObserver (entry/snapshot/result writers) -> CoordinatedObserver (gate.paused around every stop,
    gate terminated on exit) -> retained `observer.Observer` (transport, 512 slot entry breakpoints, request queue)."""

    def __init__(self, machine, gate, targets, memory):
        self.gate = gate
        super().__init__(machine, targets, memory)

    # entry stop -------------------------------------------------------------
    def _b2_entry(self, pc):
        try:
            target = self.targets.match(self.client.read_memory, pc)
        except OBS.RspError as error:
            raise X.Stop(f'B2 entry fingerprint cannot be read: {error}')
        if target is None:
            self.unmatched_entries += 1
            return
        cs, cr3, rsp = (self.client.read_register(name) for name in ('cs', 'cr3', 'rsp'))
        X.need(cs & 3 == 3, 'B2 entry not in user mode')
        view = MEM.GuestView(cr3, self.client.read_physical_memory)
        try:
            record = MEM.entry_record(view, label=f'{target["name"]}#{len(self.spawns) + 1}', rsp=rsp, pc=pc,
                                      user_pages=self.memory.user_pages, kernel_pages=self.memory.kernel_pages, symbols=self.memory.symbols)
        except MEM.DiscoveryError as error:
            self.memory_failure = str(error)
            raise X.Stop(f'B2 memory wiring cannot establish stack ranges for {target["name"]}: {error}')
        spawn = {'ordinal': len(self.spawns) + 1, 'name': target['name'], 'base': pc, 'cr3': cr3, 'phase': self.phase,
                 'sha256': target['sha256'], 'entry': record, 'ns': time.monotonic_ns(), 'superseded': False}
        if pc in self.live:
            self.live[pc]['superseded'] = True
        self.spawns.append(spawn)
        self.live[pc] = spawn
        if target['name'] == FOCUS:
            self.focus = pc
        ENV.dump(self.m.work / f'evidence/memory-entry-{spawn["ordinal"]:02d}-{target["name"]}.json', {k: v for k, v in spawn.items()})

    # snapshots --------------------------------------------------------------
    def _snapshot(self, name):
        by_name = {t['name']: t for t in self.targets.targets}
        focus = [s for s in self.spawns if s['name'] == FOCUS]
        X.need(focus, f'B2 desktop snapshot: the {FOCUS} entry stop was never observed')
        out, unobserved, failed = [], [], []
        for spawn in self.spawns:
            if spawn['superseded']:
                unobserved.append({'spawn': spawn['ordinal'], 'name': spawn['name'], 'reason': 'slot re-entered by a later spawn'})
                continue
            view = MEM.GuestView(spawn['cr3'], self.client.read_physical_memory)
            if not self._stacks_present(view, spawn['entry']):
                unobserved.append({'spawn': spawn['ordinal'], 'name': spawn['name'],
                                   'reason': 'stack pages not present at the snapshot (task exited / frames released)'})
                continue
            record = MEM.collect(view, self._regions(by_name[spawn['name']], spawn), phase=name, instance=spawn['ordinal'],
                                 provenance=self.memory.provenance, primed_proofs=spawn['entry']['primed_proofs'])
            record['spawn'] = {'ordinal': spawn['ordinal'], 'name': spawn['name'], 'label': spawn['entry']['label'],
                               'primed_status': spawn['entry']['primed_status'], 'proof_scope': spawn['entry']['proof_scope']}
            out.append(record)
            if record['errors'] and spawn['name'] == FOCUS:
                failed.append(record['errors'])
        index = len(self.memory_records)
        document = {'schema': 'cellos.s10-9b.b2-desktop-memory-snapshot/1', 'phase': name, 'records': out, 'unobserved': unobserved,
                    'allocator_peak': None,
                    'allocator_peak_reason': 'B2 terminal/shell have no Cellbox heapwatch/allocator guard hooks; allocator peak is not measured for B2',
                    'focus_task': FOCUS,
                    'tools_not_spawned_note': 'the B2 GUI terminal interprets its builtins in-process (session.rs); ls/cat/echo/ps/kill ELFs are never spawned by a typed line',
                    'provenance': self.memory.provenance, 'monotonic_ns': time.monotonic_ns()}
        self.memory_records.append({'phase': name, 'observed': [r['spawn']['name'] for r in out], 'unobserved': unobserved,
                                    'ok': [r['ok'] for r in out], 'depth_bytes': {r['spawn']['name']: r['stack_depth_bytes'] for r in out}})
        ENV.dump(self.m.work / f'evidence/memory-{index:02d}-{name}.json', document)
        if failed:
            raise X.Stop(f'B2 desktop memory snapshot {name!r} failed closed: {failed!r}')


# ------------------------------------------------------------------ machine
class B2DesktopMachine(MACH.MeasurementMachine):
    """Retained S10-7 `MeasurementMachine` for the B2 desktop v2 set: files verified against the pins, a PRIVATE copy of the hooks ISO
    (read-only to QEMU), the retained desktop `D.Machine` argv with the NVMe data disk dropped (B2 needs none), gdb stub + `-S`."""

    def __init__(self, spec, work, *, observed):
        import argparse
        self.spec = spec
        self.work = Path(work)
        self.work.mkdir(mode=0o700, parents=True, exist_ok=False)
        for name in ('evidence', 'scratch', 'sock', 'iso'):
            (self.work / name).mkdir(mode=0o700)
        check = ENV.verify_set_files(spec)
        X.need(check['all_match'], f'measurement set files differ from the pins: {check["match"]}')
        X.need(ENV.kernel_has_test_hooks(spec.kernel), 'measurement kernel is not a test-hooks build')
        X.need(not ENV.kernel_has_test_hooks(spec.control_kernel), 'control kernel unexpectedly has test hooks')
        private_iso = self.work / 'iso' / spec.iso.name
        shutil.copyfile(spec.iso, private_iso)
        X.need(ENV.sha256_file(private_iso) == ENV.PINNED['iso'], 'private ISO copy differs from the pinned ISO')
        self.private_iso = private_iso
        self.inputs = {'iso': {'path': str(private_iso), 'sha256': ENV.PINNED['iso']}, 'files': {}}
        self.source_disk_sha = None
        glyph = spec.glyph_bin
        X.need(ENV.sha256_file(glyph) == ENV.GLYPH_SHA, 'glyph oracle is not the retained GLYPH_SHA binary')
        args = argparse.Namespace(profile=spec.profile, work=str(work), evidence=str(self.work / 'evidence'), iso=str(private_iso),
                                  accel='tcg', window=ENV.DEADLINES['total_window_s'], glyph_bin=str(glyph))
        D.Machine.__init__(self, args, 'measurement', str(self.work / 'disk.img'), False)
        self._drop_data_disk()
        self.observed = observed
        self.start_ns = None
        self.started = False
        if observed:
            self.gdb_path = self.work / 'sock/gdb.sock'
            self.argv += ['-chardev', f'socket,path={self.gdb_path},server=on,wait=off,id=gdb0', '-gdb', 'chardev:gdb0', '-S']
        self.provenance = ENV.provenance(spec)
        ENV.dump(self.work / 'evidence/input-provenance.json', {
            'set': spec.name, 'kernel_provenance': self.provenance, 'profile': spec.profile, 'iso': self.inputs['iso'],
            'argv': self.argv, 'fixture_disk': None, 'fixture_disk_sha256': None, 'glyph_sha256': ENV.sha256_file(glyph),
            'mode': 'observed' if observed else 'untraced', 'set_files': check})

    def stop(self):
        raw = D.Machine.stop(self) if self.started else b''
        if self.started:
            X.dump(self.work / 'evidence/serial-receipts.json', self.serial.receipts)
            self.serial.conn.close()
            self.err.close()
            self.qmp.file.close()
            self.qmp.sock.close()
        now = ENV.verify_set_files(self.spec)
        proof = {'set': self.spec.name, 'files_match_pins': now['all_match'], 'actual': now['actual'],
                 'private_iso_unchanged': ENV.sha256_file(self.private_iso) == ENV.PINNED['iso']}
        X.dump(self.work / 'evidence/final-input-proof.json', proof)
        X.need(proof['files_match_pins'] and proof['private_iso_unchanged'], 'measurement input changed')
        return raw


# ------------------------------------------------------------------ surface
class B2DesktopSurface(C.CoordinatedSurface):
    """Retained desktop sequence (desktop ready -> terminal launch -> exact initial screen) with the B2 terminal's literal rows, plus
    the shared-subset commands through the retained exact-screen `Desk.command`.  Replaces `ready`; adds `shared_subset`."""

    def __init__(self, machine, observer, gate, schedule=None):
        self.schedule = schedule if schedule is not None else ENV.schedule()
        self.command_results = []
        super().__init__(machine, observer, gate)

    def ready(self):
        initial = next(p for p in self.schedule['phases'] if p['name'] == 'initial-prompt')
        rows = list(initial['expect_rows'])
        X.need(rows == INITIAL_ROWS, 'schedule initial rows differ from the B2 terminal source rows')
        with self.gate.caller_scope(ENV.DEADLINES['total_window_s']):
            self.phase('desktop-ready')
            self.wait(lambda: '[desktop] Ready. Entering event loop.' in self.m.text(), 900, 'desktop ready')
            self.timestamps['desktop_ready_ns'] = time.monotonic_ns()
            self.pause(3)
            self.d = G.TemporalDesk(self.m.a, self.m, D.Verdict([]), self.check, self.gate)
            d = self.d
            pre = d.shot('desktop-before-terminal')
            X.need(pre.size == (1280, 800), 'desktop resolution')
            X.need(d.locate(pre) is None, 'Terminal already present')
            self.phase('terminal-launch')
            self.timestamps['terminal_click_ns'] = time.monotonic_ns()
            d.click_px(D.TASKBAR['terminal'], D.TASKBAR_Y)
            d.park()
            self.wait(lambda: '[terminal] Terminal surface ready. Entering event loop.' in self.m.text(), 300, 'Terminal surface')
            image = d.expect_screen(rows, 'runtime initial exact prompt (B2 terminal)', 240)
            self.timestamps['exact_prompt_ns'] = time.monotonic_ns()
            d.shot('prompt', image)
            org = d.org
            region = pre.crop((org[0], org[1], org[0] + D.WIN_W, org[1] + D.WIN_H))
            X.need(d.diff(region, d.pad(rows), 0, D.ROWS - 1, d.mcursor(rows))[0] > 1000, 'desktop prompt negative control')
            _, window = d.window(image)
            X.need(d.diff(window, d.pad(rows[:-1] + ['Cellos # ']), 0, D.ROWS - 1, INITIAL_CURSOR)[0] >= D.MIN_NEGATIVE_DIFF, 'wrong-prompt negative control')
            X.need(d.cursor_of(window) == INITIAL_CURSOR, 'initial GUI cursor')
            d.click_px(org[0] + 360, org[1] + 300)
            d.park()
            self.pause(2)
            d.model[:] = rows
            d.state['img'] = image
            self.phase('initial-prompt')
            self.snapshot('initial-prompt')

    def shared_subset(self):
        results = []
        for phase in self.schedule['phases']:
            if phase['kind'] != 'command':
                continue
            self.phase(phase['name'])
            start = time.monotonic_ns()
            with self.gate.caller_scope(ENV.DEADLINES['command_s']):
                alternative = self.d.command(phase['name'], phase['command'], [list(phase['expect_rows'])], timeout=ENV.DEADLINES['command_s'])
            end = time.monotonic_ns()
            self.check()
            entry = {'phase': phase['name'], 'command': phase['command'], 'expect_rows': phase['expect_rows'], 'exact_alternative': alternative,
                     'exact_screen_match': alternative == 0, 'start_ns': start, 'end_ns': end, 'seconds': (end - start) / 1e9,
                     'model_rows_after': list(self.d.model), 'status': 'not_applicable (B2 GUI terminal has no exit status)'}
            self.command_results.append(entry)
            results.append(entry)
            self.events.append({'command': phase['command'], 'name': phase['name'], 'start_ns': start, 'end_ns': end, 'rows': list(self.d.model)})
            if phase.get('snapshot'):
                self.snapshot(phase['name'])
        return results
