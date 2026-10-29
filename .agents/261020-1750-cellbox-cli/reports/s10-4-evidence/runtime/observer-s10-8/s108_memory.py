"""S10-8.0 fixed memory wiring: `MemoryMixin` with the colliding `_stop` renamed.

Defect (S10-7.4, both class-C runs): `s107_memory.MemoryMixin._stop(self, message)` overrides `threading.Thread._stop()`
(the retained `Observer` is a `threading.Thread`), which CPython calls with no argument when a thread finishes, so thread
completion raised `TypeError: MemoryMixin._stop() missing 1 required positional argument: 'message'` and replaced the real
stop.  Here the helper is `_wiring_stop`; nothing else in the mixin changes except the evidence schema label.

Everything that is not the mixin (range discovery, entry records, `MemoryContext`, symbols) is the unchanged S10-7 code,
re-exported from `s107_memory`; the S10-7 file stays byte-identical.
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s107_env as ENV  # noqa: E402
import s107_memory as M7  # noqa: E402
from s107_memory import (DiscoveryError, KernelSymbols, MemoryContext, discover_kernel_stack, discover_user_stack,  # noqa: E402,F401
                         entry_record, heap_arena_symbol, prime_proof, StatusError, select_status)
from guest_view import GuestView, PAGE  # noqa: E402
from memory_snapshot import collect, regions_from_observer_instance  # noqa: E402
from common import Stop  # noqa: E402  (retained exception type)

SCHEMA = 'cellos.s10-8.memory-run-summary/1'


class MemoryMixin:
    """Adds entry-stop discovery/proof and a memory record at every retained snapshot point.

    Place first in the MRO: `class MyObserver(MemoryMixin, <retained observer>)`; the constructor takes the extra
    keyword `memory=MemoryContext(...)`.  `SKIP_SNAPSHOT_NAMES` lists retained control requests that are not phases
    (the feed-hook disarm).  Wiring failures are raised as `common.Stop` through `_wiring_stop` (a name that exists
    nowhere in `threading.Thread`, `Observer`, `CoordinatedObserver`, `HookObserver` or `Machine`; see s108_census).
    """
    SKIP_SNAPSHOT_NAMES = ()

    def __init__(self, *args, memory, **kwargs):
        self.memory = memory
        self.memory_entries = {}
        self.memory_records = []
        super().__init__(*args, **kwargs)

    # entry stop -----------------------------------------------------------------
    def _entry(self, pc):
        before = set(self.instances)
        super()._entry(pc)
        if pc in self.instances and pc not in before:
            self._memory_entry(self.instances[pc])

    def _memory_entry(self, inst):
        view = GuestView(inst['cr3'], self.client.read_physical_memory)
        stack = {'bottom': inst['stack_bottom'], 'top': inst['stack_top'], 'pages': (inst['stack_top'] - inst['stack_bottom']) // PAGE,
                 'source': 'retained observer instance (entry-stop rsp page, 16 pages mapped, 2 guard pages verified unmapped)'}
        try:
            record = entry_record(view, label=f"{inst['role']}#{inst['id']}", rsp=inst['initial_rsp'], pc=inst['base'], user_pages=self.memory.user_pages,
                                  kernel_pages=self.memory.kernel_pages, symbols=self.memory.symbols, user_stack=stack)
        except DiscoveryError as error:
            self.memory_failure = str(error)
            raise self._wiring_stop(f'memory wiring cannot establish stack ranges for instance {inst["id"]}: {error}')
        record['instance'] = inst['id']
        record['role'] = inst['role']
        self.memory_entries[inst['base']] = record
        ENV.dump(self.m.work / f'evidence/memory-entry-{inst["id"]}.json', record)

    def _wiring_stop(self, message):
        return Stop(message)

    # snapshot -------------------------------------------------------------------
    def _snapshot(self, name):
        result = super()._snapshot(name)
        if name not in self.SKIP_SNAPSHOT_NAMES:
            self._memory_snapshot(name)
        return result

    def _memory_snapshot(self, name):
        if self.focus is None or self.focus not in self.memory_entries:
            raise self._wiring_stop('memory snapshot: no focus instance with an entry-stop record')
        inst = self.instances[self.focus]
        entry = self.memory_entries[self.focus]
        view = GuestView(inst['cr3'], self.client.read_physical_memory)
        used = int.from_bytes(self.client.read_memory(inst['heap'], 8), 'little')
        arena = None
        if self.memory.heap_arena_rel is not None:
            arena = (inst['base'] + self.memory.heap_arena_rel[0], self.memory.heap_arena_rel[1])
        kernel = (entry['kernel_stack']['bottom'], entry['kernel_stack']['top'] - entry['kernel_stack']['bottom'])
        regions = regions_from_observer_instance(inst, self.loads, kernel_stack=kernel, heap_arena=arena, heap_used_bytes=used)
        record = collect(view, regions, phase=name, instance=inst['id'], provenance=self.memory.provenance,
                         primed_proofs=entry['primed_proofs'])
        record['entry'] = {k: entry[k] for k in ('label', 'initial_rsp', 'primed_status', 'proof_scope', 'role')}
        record['entry']['kernel_stack'] = entry['kernel_stack']
        record['entry']['user_stack'] = entry['user_stack']
        record['sp_not_used'] = 'the vCPU rsp at a pause belongs to an arbitrary task, not the focus instance; no sp cross-check is claimed'
        index = len(self.memory_records)
        self.memory_records.append({'phase': name, 'instance': inst['id'], 'ok': record['ok'], 'exact': record['all_stack_depths_exact'],
                                    'depth_bytes': record['stack_depth_bytes'], 'errors': record['errors']})
        ENV.dump(self.m.work / f'evidence/memory-{index:02d}-{name}.json', record)
        if record['errors']:
            raise self._wiring_stop(f'memory snapshot {name!r} failed closed: {record["errors"]!r}')
        return record

    def write_results(self):
        super().write_results()
        try:
            ENV.dump(self.m.work / 'evidence/memory-summary.json', {
                'schema': SCHEMA, 'provenance': self.memory.provenance,
                'entries': list(self.memory_entries.values()), 'snapshots': self.memory_records,
                'failure': getattr(self, 'memory_failure', None)})
        except Exception:  # noqa: BLE001 - result writing must not mask the observer error
            pass
