"""Offline known-answer tests for guest_view / stack_watermark / resident / memory_snapshot (no VM)."""
import json
import random
import struct
import sys
import unittest

sys.dont_write_bytecode = True
from guest_view import GuestReadError, GuestUnmappedError, GuestView  # noqa: E402
from memory_snapshot import LIMITS, collect, regions_from_observer_instance  # noqa: E402
from resident import Region, count_regions  # noqa: E402
from stack_watermark import (PATTERN, PrimeVerificationError, StackRangeError, leading_pattern_run,  # noqa: E402
                             measure, verify_primed)

PAGE = 4096


class FakeGuest:
    """Sparse physical memory + real 4-level x86-64 tables; read_physical mirrors the RSP client."""

    def __init__(self):
        self.pages = {}
        self.next_table = 0x100000
        self.cr3 = self._table()
        self.unreadable = set()
        self.reads = 0

    def _table(self):
        pa = self.next_table
        self.next_table += PAGE
        self.pages[pa] = bytearray(PAGE)
        return pa

    def _entry(self, table, idx, value=None):
        if value is not None:
            struct.pack_into('<Q', self.pages[table], idx * 8, value)
        return struct.unpack_from('<Q', self.pages[table], idx * 8)[0]

    def map(self, va, pa, flags=0x7, shift=12):
        table = self.cr3
        for level in (39, 30, 21, 12):
            idx = (va >> level) & 511
            if level == shift:
                self._entry(table, idx, (pa & ~((1 << shift) - 1)) | flags | (128 if shift != 12 else 0))
                return
            entry = self._entry(table, idx)
            if not entry & 1:
                entry = self._table() | 0x7
                self._entry(table, idx, entry)
            table = entry & 0x000ffffffffff000

    def map_identity_range(self, start, top, fill=None, skip=()):
        for va in range(start, top, PAGE):
            if va in skip:
                continue
            self.pages.setdefault(va, bytearray(PAGE))
            if fill is not None:
                self.pages[va][:] = bytes([fill]) * PAGE
            self.map(va, va)

    def write(self, va, data):
        for i, b in enumerate(data):
            self.pages[(va + i) & ~(PAGE - 1)][(va + i) & (PAGE - 1)] = b

    def read_physical(self, pa, length):
        self.reads += 1
        base = pa & ~(PAGE - 1)
        if base in self.unreadable:
            raise OSError(f'E01 unreadable {base:#x}')
        assert (pa & (PAGE - 1)) + length <= PAGE or base in self.pages
        return bytes(self.pages[base][pa & (PAGE - 1):(pa & (PAGE - 1)) + length])

    def view(self):
        return GuestView(self.cr3, self.read_physical)


START, PAGES = 0x400000, 4
TOP = START + PAGES * PAGE
USABLE = PAGES * PAGE


def primed_guest(**kw):
    g = FakeGuest()
    g.map_identity_range(START, TOP, fill=PATTERN, **kw)
    return g


def kernel_reference(raw):
    """Byte-for-byte transcription of test_hook_watermark_bytes' loop."""
    untouched = 0
    while untouched < len(raw):
        if raw[untouched] != PATTERN:
            break
        untouched += 1
    return len(raw) - untouched


class Watermark(unittest.TestCase):
    def test_depth_zero_untouched(self):
        r = measure(primed_guest().view(), START, TOP)
        self.assertEqual((r['depth_bytes'], r['untouched_prefix_bytes'], r['status'], r['depth_pages']), (0, USABLE, 'untouched', 0))

    def test_partial_known_answer(self):
        g = primed_guest()
        g.write(TOP - 300, b'\x01' * 300)          # deepest byte at top-300
        r = measure(g.view(), START, TOP)
        self.assertEqual((r['depth_bytes'], r['status'], r['depth_pages']), (300, 'partial', 1))
        self.assertFalse(r['exact'])
        self.assertEqual(r['prime_evidence'], 'in-band-prefix')

    def test_full_usable_requires_proof_to_be_valid(self):
        g = primed_guest()
        proof = verify_primed(g.view(), START, TOP)  # primed pre-run, then fully consumed
        g.write(START, b'\x00')
        r = measure(g.view(), START, TOP, primed_proof=proof)
        self.assertEqual((r['depth_bytes'], r['status'], r['exact'], r['depth_valid']), (USABLE, 'saturated-proven', True, True))
        r = measure(g.view(), START, TOP)
        self.assertEqual((r['depth_bytes'], r['status'], r['exact'], r['depth_valid']), (USABLE, 'saturated-unproven', False, False))

    def test_pattern_colliding_write_is_invisible_lower_bound(self):
        g = primed_guest()
        g.write(TOP - 4000, bytes([PATTERN]) * 3000)  # deepest stores equal the pattern
        g.write(TOP - 1000, b'\x7f')                   # shallowest visible store
        r = measure(g.view(), START, TOP)
        self.assertEqual(r['depth_bytes'], 1000)       # true depth was 4000; invisible by design
        g.write(TOP - 1000, bytes([PATTERN]))          # now every store collides
        self.assertEqual(measure(g.view(), START, TOP)['depth_bytes'], 0)

    def test_zero_valued_store_is_visible_only_because_zero_is_not_the_pattern(self):
        g = primed_guest()
        g.write(TOP - 8, b'\x00' * 8)
        self.assertEqual(measure(g.view(), START, TOP)['depth_bytes'], 8)

    def test_page_boundary_off_by_one(self):
        for k in range(1, PAGES):
            g = primed_guest()
            g.write(START + k * PAGE - 1, b'\x01')    # last byte of page k-1
            self.assertEqual(measure(g.view(), START, TOP)['depth_bytes'], USABLE - (k * PAGE - 1), k)
            g = primed_guest()
            g.write(START + k * PAGE, b'\x01')        # first byte of page k
            self.assertEqual(measure(g.view(), START, TOP)['depth_bytes'], USABLE - k * PAGE, k)
        g = primed_guest()
        g.write(TOP - 1, b'\x01')                     # topmost byte only
        self.assertEqual(measure(g.view(), START, TOP)['depth_bytes'], 1)
        g = primed_guest()
        g.write(START, b'\x01')                       # lowest byte
        self.assertEqual(measure(g.view(), START, TOP)['depth_bytes'], USABLE)

    def test_matches_kernel_loop_on_random_images(self):
        rng = random.Random(0xA5)
        for _ in range(200):
            g = primed_guest()
            n = rng.randrange(0, 4)
            for _ in range(n):
                g.write(START + rng.randrange(USABLE), bytes([rng.choice([0, 1, PATTERN, 0xFF])]))
            raw = b''.join(bytes(g.pages[va]) for va in range(START, TOP, PAGE))
            got = measure(g.view(), START, TOP, primed_proof={'start': START, 'limit': TOP, 'bytes_checked': USABLE, 'pattern': PATTERN})
            self.assertEqual(got['depth_bytes'], kernel_reference(raw))
        self.assertEqual(leading_pattern_run(b''), 0)

    def test_scan_stops_at_first_nonpattern_page_but_all_pages_must_be_mapped(self):
        g = primed_guest(skip={START + 3 * PAGE})
        g.write(START, b'\x01')
        with self.assertRaises(GuestUnmappedError) as cm:
            measure(g.view(), START, TOP)
        self.assertEqual(cm.exception.va, START + 3 * PAGE)

    def test_unmapped_first_and_middle_page(self):
        for hole in (START, START + 2 * PAGE):
            with self.assertRaises(GuestUnmappedError):
                measure(primed_guest(skip={hole}).view(), START, TOP)

    def test_unreadable_page_fails_closed(self):
        g = primed_guest()
        g.unreadable.add(START + PAGE)
        g.write(START + 2 * PAGE, b'\x01')            # scan must read page 1 to find the prefix end
        with self.assertRaises(GuestReadError):
            measure(g.view(), START, TOP)

    def test_unreadable_page_table_fails_closed(self):
        g = primed_guest()
        g.unreadable.add(g.cr3)
        with self.assertRaises(GuestReadError):
            measure(g.view(), START, TOP)

    def test_short_physical_reply_fails_closed(self):
        g = primed_guest()
        view = GuestView(g.cr3, lambda pa, n: g.read_physical(pa, n)[:-1])
        with self.assertRaises(GuestReadError):
            measure(view, START, TOP)

    def test_off_range_and_malformed_inputs(self):
        v = primed_guest().view()
        for args in [(START + 1, TOP), (START, TOP + 1), (TOP, START), (START, START), (START, (1 << 64) + PAGE)]:
            with self.assertRaises(StackRangeError, msg=args):
                measure(v, *args)
        for sp in (START - 1, TOP + 1):
            with self.assertRaises(StackRangeError):
                measure(v, START, TOP, sp=sp)
        self.assertEqual(measure(v, START, TOP, sp=TOP)['sp_depth_bytes'], 0)
        r = measure(v, START, TOP, sp=START)
        self.assertEqual((r['sp_depth_bytes'], r['watermark_covers_sp']), (USABLE, False))

    def test_guard_pages_below_range_are_not_read(self):
        g = primed_guest()                            # START-PAGE, START-2*PAGE unmapped like kernel guards
        self.assertEqual(measure(g.view(), START, TOP)['depth_bytes'], 0)

    def test_primed_proof_checks(self):
        g = primed_guest()
        proof = verify_primed(g.view(), START, TOP - PAGE)
        self.assertEqual(proof['bytes_checked'], (PAGES - 1) * PAGE)
        g.write(START + PAGE + 7, b'\x00')
        with self.assertRaises(PrimeVerificationError) as cm:
            verify_primed(g.view(), START, TOP)
        self.assertEqual(cm.exception.va, START + PAGE + 7)
        with self.assertRaises(StackRangeError):
            measure(g.view(), START, TOP, primed_proof={'start': START + PAGE, 'limit': TOP, 'bytes_checked': 1, 'pattern': PATTERN})
        with self.assertRaises(StackRangeError):
            verify_primed(g.view(), START, START)

    def test_pattern_constant_matches_kernel_source(self):
        from pathlib import Path
        src = Path('/home/dmin/cellos/kernel/src/task/stack.rs').read_text()
        self.assertIn(f'const STACK_WATERMARK_PATTERN: u8 = 0x{PATTERN:X};', src)


class Resident(unittest.TestCase):
    def test_fully_and_partially_present_region(self):
        g = FakeGuest()
        g.map_identity_range(0x800000, 0x800000 + 8 * PAGE, skip={0x800000 + 2 * PAGE, 0x800000 + 3 * PAGE, 0x800000 + 7 * PAGE})
        out = count_regions(g.view(), [Region('seg', 'elf-segment', 0x800000, 8 * PAGE)])
        r = out['regions'][0]
        self.assertEqual((r['pages'], r['present_pages'], r['not_present_pages'], r['mapped_backed_bytes'], r['not_present_bytes'], r['fully_mapped']),
                         (8, 5, 3, 5 * PAGE, 3 * PAGE, False))
        self.assertEqual(r['not_present_ranges'], [[0x802000, 0x804000], [0x807000, 0x808000]])
        self.assertEqual(out['total']['distinct_frame_bytes'], 5 * PAGE)

    def test_unaligned_region_rounds_outward(self):
        g = FakeGuest()
        g.map_identity_range(0x800000, 0x800000 + 3 * PAGE)
        r = count_regions(g.view(), [Region('s', 'elf-segment', 0x800000 + 4095, 2)])['regions'][0]   # straddles page 0/1
        self.assertEqual((r['pages'], r['mapped_backed_bytes'], r['page_span']), (2, 2 * PAGE, [0x800000, 0x802000]))
        r = count_regions(g.view(), [Region('s', 'elf-segment', 0x800000, 0)])['regions'][0]
        self.assertEqual((r['pages'], r['mapped_backed_bytes']), (0, 0))

    def test_region_off_by_one_at_page_boundary(self):
        g = FakeGuest()
        g.map_identity_range(0x800000, 0x802000)
        for size, pages in ((PAGE, 1), (PAGE + 1, 2), (2 * PAGE, 2)):
            r = count_regions(g.view(), [Region('s', 'elf-segment', 0x800000, size)])['regions'][0]
            self.assertEqual((r['pages'], r['not_present_pages']), (pages, 0), size)
        r = count_regions(g.view(), [Region('s', 'elf-segment', 0x800000, 2 * PAGE + 1)])['regions'][0]
        self.assertEqual((r['pages'], r['present_pages'], r['not_present_ranges']), (3, 2, [[0x802000, 0x803000]]))

    def test_overlap_shared_frame_and_heap_split(self):
        g = FakeGuest()
        g.map_identity_range(0x800000, 0x804000)
        g.map(0x900000, 0x801000)                     # alias: same frame, second VA
        regs = [Region('elf', 'elf-segment', 0x800000, 4 * PAGE), Region('heap', 'heap-arena', 0x802000, 2 * PAGE),
                Region('alias', 'user-stack', 0x900000, PAGE)]
        out = count_regions(g.view(), regs)
        by = {r['label']: r for r in out['regions']}
        self.assertEqual(by['heap']['overlaps'], {'elf': 2})
        t = out['total']
        self.assertEqual(t['sum_of_region_mapped_backed_bytes'], (4 + 2 + 1) * PAGE)
        self.assertEqual(t['distinct_frame_bytes'], 4 * PAGE)                       # alias shares a frame
        self.assertEqual(t['distinct_frame_bytes_excluding_heap_arena'], 4 * PAGE)
        with self.assertRaises(ValueError):
            count_regions(g.view(), [regs[0], regs[0]])

    def test_large_page_leaf_and_flags(self):
        g = FakeGuest()
        g.map(0x40000000, 0x40000000, flags=0x67, shift=21)      # present|rw|user|accessed|dirty 2 MiB leaf
        info = g.view().page_info(0x40000000 + 5 * PAGE)
        self.assertEqual((info['present'], info['leaf_shift'], info['pa']), (True, 21, 0x40000000 + 5 * PAGE))
        self.assertTrue(all(info['flags'][k] for k in ('present', 'writable', 'user', 'accessed', 'dirty')))
        r = count_regions(g.view(), [Region('h', 'heap-arena', 0x40000000, 4 * PAGE)])['regions'][0]
        self.assertEqual((r['mapped_backed_bytes'], r['leaf_page_sizes']), (4 * PAGE, {str(1 << 21): 4}))

    def test_unreadable_table_fails_closed_and_no_rss_key(self):
        g = FakeGuest()
        g.map_identity_range(0x800000, 0x801000)
        out = count_regions(g.view(), [Region('s', 'elf-segment', 0x800000, PAGE)])
        self.assertFalse([k for r in out['regions'] for k in r if 'rss' in k.lower()] + [k for k in out['total'] if 'rss' in k.lower()])
        self.assertIn('NOT host RSS', out['scope'])
        g.unreadable.add(g.cr3)
        with self.assertRaises(GuestReadError):
            count_regions(g.view(), [Region('s', 'elf-segment', 0x800000, PAGE)])

    def test_bad_region_kind(self):
        with self.assertRaises(ValueError):
            Region('x', 'rss', 0, 1)


PROV = {'kernel_build': 'test-hooks measurement kernel (disposable, not the promoted candidate)', 'kernel_features': ['test-hooks'], 'profile': 'desktop'}


def full_guest():
    g = FakeGuest()
    g.map_identity_range(0x500000, 0x500000 + 2 * PAGE)                       # text-ish seg
    g.map_identity_range(0x700000, 0x700000 + 16 * PAGE, fill=PATTERN)        # user stack
    g.map_identity_range(0x300000, 0x300000 + 4 * PAGE, fill=PATTERN)         # kernel stack
    g.map_identity_range(0xA00000, 0xA00000 + 8 * PAGE)                       # heap arena
    return g


class Collector(unittest.TestCase):
    def regions(self, g):
        inst = {'base': 0x400000, 'stack_bottom': 0x700000, 'stack_top': 0x700000 + 16 * PAGE}
        return regions_from_observer_instance(inst, [{'va': 0x100000, 'memsz': 2 * PAGE, 'filesz': 100, 'flags': 5}],
                                              kernel_stack=(0x300000, 4 * PAGE), heap_arena=(0xA00000, 8 * PAGE), heap_used_bytes=1234)

    def test_full_record_known_answers(self):
        g = full_guest()
        g.write(0x700000 + 16 * PAGE - 5000, b'\x01' * 10)   # user depth 5000
        g.write(0x300000 + 4 * PAGE - 200, b'\x02')          # kernel depth 200
        proofs = {'user-stack': {'start': 0x700000, 'limit': 0x700000 + 15 * PAGE, 'bytes_checked': 15 * PAGE, 'pattern': PATTERN},
                  'kernel-stack': {'start': 0x300000, 'limit': 0x300000 + 3 * PAGE, 'bytes_checked': 3 * PAGE, 'pattern': PATTERN}}
        rec = collect(g.view(), self.regions(g), phase='complete', instance=1, provenance=PROV, primed_proofs=proofs,
                      stack_sp={'user-stack': 0x700000 + 16 * PAGE - 64}, now_ns=7)
        self.assertEqual(json.loads(json.dumps(rec)), rec)
        self.assertTrue(rec['ok'] and rec['all_stack_depths_exact'])
        self.assertEqual(rec['stack_depth_bytes'], {'user': 5000, 'kernel': 200})
        by = {r['label']: r for r in rec['resident']['regions']}
        self.assertEqual((by['elf-load-0']['mapped_backed_bytes'], by['user-stack']['mapped_backed_bytes'], by['kernel-stack']['mapped_backed_bytes'],
                          by['heap-arena']['mapped_backed_bytes']), (2 * PAGE, 16 * PAGE, 4 * PAGE, 8 * PAGE))
        self.assertEqual(by['elf-load-0']['not_present_pages'], 0)
        self.assertEqual(rec['heap_arena']['extra']['allocator_used_bytes'], 1234)
        self.assertEqual(rec['limits'], list(LIMITS))
        self.assertTrue(any('lower bound' in s.lower() for s in rec['limits']) and any('0xA5' in s for s in rec['limits']))
        self.assertTrue(any('QEMU/TCG' in s for s in rec['limits']))

    def test_failures_retained_not_skipped(self):
        g = full_guest()
        g.unreadable.add(0x300000 + PAGE)
        g.write(0x300000 + 2 * PAGE, b'\x01')
        rec = collect(g.view(), self.regions(g), phase='p', provenance=PROV)
        self.assertFalse(rec['ok'])
        self.assertEqual([e['where'] for e in rec['errors']], ['kernel-stack'])
        kernel = next(s for s in rec['stacks'] if s['label'] == 'kernel-stack')
        self.assertEqual((kernel['status'], kernel['depth_valid']), ('error', False))
        self.assertEqual(rec['stack_depth_bytes'], {'user': 0})
        self.assertTrue(rec['resident'] is not None)

    def test_unmapped_stack_page_and_unproven_saturation_not_ok(self):
        g = full_guest()
        g.write(0x700000, b'\x00')                                         # user stack fully consumed, no proof
        rec = collect(g.view(), self.regions(g), phase='p', provenance=PROV)
        self.assertFalse(rec['ok'])
        self.assertNotIn('user', rec['stack_depth_bytes'])
        self.assertFalse(rec['all_stack_depths_exact'])
        g2 = FakeGuest()
        g2.map_identity_range(0x700000, 0x700000 + 16 * PAGE, fill=PATTERN, skip={0x700000 + 5 * PAGE})
        rec = collect(g2.view(), [Region('user-stack', 'user-stack', 0x700000, 16 * PAGE)], phase='p', provenance=PROV)
        self.assertEqual(rec['errors'][0]['type'], 'GuestUnmappedError')
        self.assertEqual(rec['resident']['regions'][0]['not_present_ranges'], [[0x705000, 0x706000]])   # flagged in resident too

    def test_resident_failure_retained(self):
        g = full_guest()
        g.unreadable.add(g.cr3)
        rec = collect(g.view(), self.regions(g), phase='p', provenance=PROV)
        self.assertFalse(rec['ok'])
        self.assertIsNone(rec['resident'])
        self.assertEqual(rec['errors'][0]['where'], 'resident')

    def test_provenance_required_and_test_hooks_labelled(self):
        g = full_guest()
        for bad in (None, {}, {'kernel_build': 'x', 'profile': 'p'}, dict(PROV, kernel_features=['other']), dict(PROV, profile='')):
            with self.assertRaises(ValueError):
                collect(g.view(), self.regions(g), phase='p', provenance=bad)

    def test_regions_from_observer_instance_shape(self):
        inst = {'base': 0x400000, 'stack_bottom': 0x700000, 'stack_top': 0x710000}
        regs = regions_from_observer_instance(inst, [{'va': 0x1000, 'memsz': 5000, 'filesz': 10, 'flags': 6}, {'va': 0x3000, 'memsz': 1, 'filesz': 1, 'flags': 5}])
        self.assertEqual([(r.label, r.kind, r.va, r.size) for r in regs],
                         [('elf-load-0', 'elf-segment', 0x401000, 5000), ('elf-load-1', 'elf-segment', 0x403000, 1), ('user-stack', 'user-stack', 0x700000, 0x10000)])


if __name__ == '__main__':
    unittest.main()
