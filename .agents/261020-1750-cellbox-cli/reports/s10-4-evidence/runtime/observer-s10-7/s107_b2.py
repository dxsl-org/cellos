"""S10-7.4 B2 (historical) headless observer: entry stops + stack/resident memory records, nothing else.

The retained `observer.Observer` is cellbox-specific (heap counter write-watches, cellbox text fingerprints).  B2 has no
Cellbox heapwatch, so this observer keeps only the retained transport/stop machinery (`run`, `_stopped`, `_pending`,
`snapshot`, `shutdown`, the 512 slot entry breakpoints, the RSP client with read-only `p`/`m` packets and hardware `Z1`)
and replaces `__init__`, `_handle`, `_entry`, `_snapshot` and `write_results`.  Allocator peak is NOT measured for B2.

Task identity at an entry stop = byte equality of three 48-byte `.text` windows (start, middle, end) against the signed B2
ELF of each of the six measured tools (`shell ls cat echo ps kill`), pairwise distinct (checked).  Other cells (init,
services, drivers) are counted and ignored.
"""
import json
import queue
import struct
import sys
import threading
import time
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402
import s107_memory as MEM  # noqa: E402

import common as X  # noqa: E402
import observer as OBS  # noqa: E402
from rsp import RspClient, RspError  # noqa: E402

B2_TOOLS = (('shell', 'headless-app-shell', '/bin/shell'), ('ls', 'headless-ls', '/bin/ls'), ('cat', 'headless-cat', '/bin/cat'),
            ('echo', 'headless-echo', '/bin/echo'), ('ps', 'headless-ps', '/bin/ps'), ('kill', 'headless-kill', '/bin/kill'))
WINDOW = 48


def fingerprint_windows(elf_path, width=WINDOW):
    """Three relocation-free windows of `.text`: [(VA relative to the load base, bytes)]."""
    data, sections, _loads = OBS.elf_layout(elf_path)
    text = sections['.text']
    addr, offset, size = text[3], text[4], text[5]
    if size < 3 * width:
        raise MEM.DiscoveryError(f'{elf_path}: .text too small for three windows')
    relocs = []
    reloc = sections.get('.rela.dyn')
    if reloc:
        relocs = [struct.unpack_from('<Q', data, at)[0] for at in range(reloc[4], reloc[4] + reloc[5], 24)]
    out = []
    for delta in (0, (size // 2) & ~15, size - width):
        va = addr + delta
        if any(va <= r < va + width for r in relocs):
            raise MEM.DiscoveryError(f'{elf_path}: fingerprint window at {va:#x} covers a relocation')
        out.append((va, data[offset + delta:offset + delta + width]))
    return out


class B2Targets:
    def __init__(self, signed_dir=ENV.B2_IMAGE / 'signed'):
        self.targets = []
        expected = json.loads((ENV.TH_KERNELS / 'b2-headless.static-verification.json').read_text())['image_file_sha256']
        for name, signed, image_path in B2_TOOLS:
            path = Path(signed_dir) / signed
            digest = ENV.sha256_file(path)
            if digest != expected[image_path]:
                raise MEM.DiscoveryError(f'{path} sha256 differs from the B2 headless image record for {image_path}')
            _data, _sections, loads = OBS.elf_layout(path)
            try:
                arena = MEM.heap_arena_symbol(path, kind='b2-shell')
            except MEM.DiscoveryError:
                arena = None      # no discoverable arena symbol: region omitted, recorded as null
            self.targets.append({'name': name, 'path': str(path), 'sha256': digest, 'windows': fingerprint_windows(path),
                                 'loads': loads, 'heap_arena': arena})
        seen = {}
        for target in self.targets:
            key = tuple(window for _va, window in target['windows'])
            if key in seen:
                raise MEM.DiscoveryError(f'B2 tool fingerprints are not distinct: {target["name"]} == {seen[key]}')
            seen[key] = target['name']

    def match(self, read_memory, pc):
        """The target whose three windows equal guest memory at `pc`, else None."""
        for target in self.targets:
            ok = True
            for va, want in target['windows']:
                if read_memory(pc + va, len(want)) != want:
                    ok = False
                    break
            if ok:
                return target
        return None


class B2MemoryObserver(OBS.Observer):
    def __init__(self, machine, targets, memory):
        threading.Thread.__init__(self, daemon=True)
        self.m = machine
        self.profile = machine.a.profile
        self.phase = 'boot'
        self.error = None
        self.ready = threading.Event()
        self.done = threading.Event()
        self.requests = queue.Queue()
        self.instances = {}
        self.focus = None
        self.entry_ordinal = 0
        self.client = RspClient(str(machine.gdb_path), str(machine.work / 'evidence/rsp-traffic.jsonl'), timeout=5)
        self.breaks, self.watches = set(), set()
        self.events = (machine.work / 'evidence/heap-events.jsonl').open('w')       # retained run() closes these two handles
        self.coverage = (machine.work / 'evidence/coverage-events.jsonl').open('w')
        self.stack_events, self.snapshots, self.draw_events = [], [], []
        self.pause_ns = 0
        self.counters = 0
        self.targets, self.memory = targets, memory
        self.spawns, self.live, self.unmatched_entries = [], {}, 0
        self.memory_records = []

    # ------------------------------------------------------------------ stops
    def _handle(self, reply):
        pc = self.client.read_register('rip')
        if self.is_entry(pc):
            self._b2_entry(pc)
            return
        raise X.Stop(f'unattributed B2 stop pc={pc:#x}, reply={reply!r}')

    def _b2_entry(self, pc):
        try:
            target = self.targets.match(self.client.read_memory, pc)
        except RspError as error:
            raise X.Stop(f'B2 entry fingerprint cannot be read: {error}')
        if target is None:
            self.unmatched_entries += 1
            return
        cs, cr3, rsp = (self.client.read_register(name) for name in ('cs', 'cr3', 'rsp'))
        X.need(cs & 3 == 3, 'B2 entry not in user mode')
        view = MEM.GuestView(cr3, self.client.read_physical_memory)
        try:
            record = MEM.entry_record(view, label=f'{target["name"]}#{len(self.spawns) + 1}', rsp=rsp, pc=pc,
                                      user_pages=self.memory.user_pages, kernel_pages=self.memory.kernel_pages,
                                      symbols=self.memory.symbols)
        except MEM.DiscoveryError as error:
            self.memory_failure = str(error)
            raise X.Stop(f'B2 memory wiring cannot establish stack ranges for {target["name"]}: {error}')
        spawn = {'ordinal': len(self.spawns) + 1, 'name': target['name'], 'base': pc, 'cr3': cr3, 'phase': self.phase,
                 'sha256': target['sha256'], 'entry': record, 'ns': time.monotonic_ns(), 'superseded': False}
        if pc in self.live:
            self.live[pc]['superseded'] = True
        self.spawns.append(spawn)
        self.live[pc] = spawn
        if target['name'] == 'shell':
            self.focus = pc
        ENV.dump(self.m.work / f'evidence/memory-entry-{spawn["ordinal"]:02d}-{target["name"]}.json', {k: v for k, v in spawn.items()})

    # ------------------------------------------------------------------ snapshots
    def _regions(self, target, spawn):
        from memory_snapshot import regions_from_observer_instance
        entry = spawn['entry']
        inst = {'base': spawn['base'], 'stack_bottom': entry['user_stack']['bottom'], 'stack_top': entry['user_stack']['top']}
        arena = None
        if target['heap_arena'] is not None:
            arena = (spawn['base'] + target['heap_arena'][0], target['heap_arena'][1])
        kernel = (entry['kernel_stack']['bottom'], entry['kernel_stack']['top'] - entry['kernel_stack']['bottom'])
        return regions_from_observer_instance(inst, target['loads'], kernel_stack=kernel, heap_arena=arena)

    def _stacks_present(self, view, entry):
        for key in ('user_stack', 'kernel_stack'):
            for va in range(entry[key]['bottom'], entry[key]['top'], MEM.PAGE):
                if view.translate(va)[0] is None:
                    return False
        return True

    def _snapshot(self, name):
        by_name = {t['name']: t for t in self.targets.targets}
        shell = [s for s in self.spawns if s['name'] == 'shell']
        X.need(shell, 'B2 snapshot: the shell entry stop was never observed')
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
            if record['errors'] and spawn['name'] == 'shell':
                failed.append(record['errors'])
        index = len(self.memory_records)
        document = {'schema': 'cellos.s10-7.b2-memory-snapshot/1', 'phase': name, 'records': out, 'unobserved': unobserved,
                    'allocator_peak': None,
                    'allocator_peak_reason': 'B2 shell has no Cellbox heapwatch/allocator guard hooks; allocator peak is not measured for B2',
                    'tools_not_spawned_note': 'ls/cat/echo/ps/kill are shell builtins at this commit (executor.rs dispatch_builtin); the ELFs exist but a typed command does not spawn them',
                    'provenance': self.memory.provenance, 'monotonic_ns': time.monotonic_ns()}
        self.memory_records.append({'phase': name, 'observed': [r['spawn']['name'] for r in out], 'unobserved': unobserved,
                                    'ok': [r['ok'] for r in out], 'depth_bytes': {r['spawn']['name']: r['stack_depth_bytes'] for r in out}})
        ENV.dump(self.m.work / f'evidence/memory-{index:02d}-{name}.json', document)
        if failed:
            raise X.Stop(f'B2 memory snapshot {name!r} failed closed: {failed!r}')

    def write_results(self):
        try:
            ENV.dump(self.m.work / 'evidence/observation.json', {
                'status': 'STOP_OBSERVATION' if self.error else 'COMPLETE', 'error': self.error, 'profile': self.profile,
                'spawns': self.spawns, 'unmatched_entries': self.unmatched_entries, 'snapshots': self.memory_records,
                'observer_pause_ns': self.pause_ns, 'memory_failure': getattr(self, 'memory_failure', None),
                'allocator_peak': None, 'resident_bytes': None, 'historical_runtime_baseline': None})
        except Exception:  # noqa: BLE001
            pass
