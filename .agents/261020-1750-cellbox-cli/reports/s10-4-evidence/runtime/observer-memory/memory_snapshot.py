"""S10-7.2 memory-snapshot collector (host-side, read-only, NOT wired into any live run).

Input is the region list the retained observer snapshot already derives (ELF PT_LOAD ranges
`inst['base']+s['va']` / `memsz`, user stack `inst['stack_bottom']..inst['stack_top']`) plus the
two inputs the observer does not know (kernel-stack usable range, heap-arena range).  Output is
one JSON-serialisable record: per-region mapped-backed bytes, exact sentinel stack depths,
provenance and a `limits` list.  Per-region failures are RETAINED in the record (`errors`,
`ok=false`); the caller (S10-7.4 wiring) must treat `ok=false` as a STOP.
"""
import json
import sys
import time

sys.dont_write_bytecode = True
from guest_view import GuestMemoryError  # noqa: E402
from resident import Region, count_regions  # noqa: E402
from stack_watermark import PrimeVerificationError, StackRangeError, measure  # noqa: E402

SCHEMA = 'cellos.s10-7.memory-snapshot/1'
STACK_KINDS = ('user-stack', 'kernel-stack')
LIMITS = (
    'All numbers are guest-resident bytes under QEMU/TCG of one paused vCPU; not host RSS, working set, physical-memory, or Intel/VMX production figures.',
    'mapped_backed_bytes counts guest-present 4 KiB pages under the eager SAS identity mapping (no demand paging/swap found in kernel/src by source reading): resident by construction, not an accessed/dirty-bit measurement.',
    'Stack depth is a sentinel scan: valid only if the stack was primed (kernel feature `test-hooks`, pattern 0xA5, whole usable range). It is meaningful ONLY for the separately built labelled test-hooks measurement kernel, never for the promoted candidate kernel.',
    'depth_bytes is a LOWER BOUND of true depth: zero-valued or pattern-equal (0xA5) stores are invisible; any non-pattern byte (including a stray write) below the real deepest store raises the reported depth.',
    'exact=true only with a pre-run verify_primed proof; a fully consumed range without proof (saturated-unproven) cannot be told from an unprimed stack and is not a valid depth.',
    'Peaks are since priming (task spawn); a snapshot is point-in-time at the pause and says nothing about later growth. The kernel stack depth includes the spawn trap frame at its top; the user stack depth includes initial argv/auxv-like setup if the loader writes it.',
    'The heap arena is reported separately and may lie inside an ELF segment (.bss), so region sums can double count; use total.distinct_frame_bytes[_excluding_heap_arena]. allocator_used_bytes (Heap.used) is a separate observer measurement, not residency.',
)


def regions_from_observer_instance(inst, loads, *, kernel_stack=None, heap_arena=None, heap_used_bytes=None):
    """Regions as `observer.Observer._snapshot` derives them (+ the two inputs it lacks).

    inst: observer instance dict (`base`, `stack_bottom`, `stack_top`); loads: `elf_layout()[2]`.
    kernel_stack / heap_arena: optional `(va, size)` supplied by the caller.
    """
    regions = []
    for index, seg in enumerate(loads):
        regions.append(Region(f'elf-load-{index}', 'elf-segment', inst['base'] + seg['va'], seg['memsz'],
                              {'p_flags': seg.get('flags'), 'filesz': seg.get('filesz')}))
    regions.append(Region('user-stack', 'user-stack', inst['stack_bottom'], inst['stack_top'] - inst['stack_bottom'],
                          {'source': 'observer instance stack_bottom/stack_top (guard pages below verified unmapped at entry)'}))
    if kernel_stack is not None:
        regions.append(Region('kernel-stack', 'kernel-stack', kernel_stack[0], kernel_stack[1],
                              {'source': 'caller-supplied usable range (the observer does not know it)'}))
    if heap_arena is not None:
        extra = {'source': 'caller-supplied'}
        if heap_used_bytes is not None:
            extra['allocator_used_bytes'] = heap_used_bytes
        regions.append(Region('heap-arena', 'heap-arena', heap_arena[0], heap_arena[1], extra))
    return regions


def _need_provenance(provenance):
    if not isinstance(provenance, dict) or not provenance.get('kernel_build') or not provenance.get('profile'):
        raise ValueError('provenance must carry non-empty kernel_build and profile')
    if 'test-hooks' not in provenance.get('kernel_features', ()):
        raise ValueError("provenance.kernel_features must include 'test-hooks': sentinel depths are valid only on that kernel")


def collect(view, regions, *, phase, provenance, instance=None, stack_sp=None, primed_proofs=None, now_ns=None):
    """Build the record.  `stack_sp`/`primed_proofs`: dicts keyed by region label."""
    _need_provenance(provenance)
    stack_sp, primed_proofs = stack_sp or {}, primed_proofs or {}
    errors, stacks, resident = [], [], None
    try:
        resident = count_regions(view, regions)
    except GuestMemoryError as error:
        errors.append({'where': 'resident', 'type': type(error).__name__, 'message': str(error)})
    for region in regions:
        if region.kind not in STACK_KINDS:
            continue
        entry = {'label': region.label, 'kind': region.kind}
        try:
            entry.update(measure(view, region.va, region.va + region.size, primed_proof=primed_proofs.get(region.label),
                                 sp=stack_sp.get(region.label)))
        except (GuestMemoryError, StackRangeError, PrimeVerificationError) as error:
            entry.update(status='error', exact=False, depth_valid=False, error={'type': type(error).__name__, 'message': str(error)})
            errors.append({'where': region.label, 'type': type(error).__name__, 'message': str(error)})
        stacks.append(entry)
    depths = {}
    for kind, key in (('user-stack', 'user'), ('kernel-stack', 'kernel')):
        mine = [s for s in stacks if s['kind'] == kind]
        if len(mine) == 1 and mine[0].get('depth_valid'):
            depths[key] = mine[0]['depth_bytes']
    record = {
        'schema': SCHEMA, 'phase': phase, 'instance': instance, 'cr3': view.cr3,
        'monotonic_ns': time.monotonic_ns() if now_ns is None else now_ns,
        'provenance': provenance, 'resident': resident, 'stacks': stacks, 'stack_depth_bytes': depths,
        'heap_arena': next((r for r in (resident or {}).get('regions', []) if r['kind'] == 'heap-arena'), None),
        'errors': errors,
        'ok': not errors and all(s.get('depth_valid') for s in stacks),
        'all_stack_depths_exact': bool(stacks) and all(s.get('exact') and s.get('depth_valid') for s in stacks),
        'limits': list(LIMITS),
    }
    json.dumps(record)  # fail now if anything non-serialisable slipped in
    return record
