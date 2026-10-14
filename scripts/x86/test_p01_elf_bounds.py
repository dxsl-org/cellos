"""Synthetic ELF boundary controls; these fixtures are not runtime evidence."""
import struct
import unittest

from p01_elf_bounds import (
    ELF_HEADER, PROGRAM_HEADER, U64_MAX, USER_VADDR_MAX, InvalidCertificate,
    checked_add, checked_mul, page_interval, parse_elf,
)


def elf(segments, elf_type=3, machine=62):
    """segments: (virtual address, memory size, flags), no file payload by default."""
    ident = b"\x7fELF\x02\x01\x01" + bytes(9)
    header = ELF_HEADER.pack(ident, elf_type, machine, 1, 0, 64, 0, 0,
                             64, 56, len(segments), 0, 0, 0)
    return header + b"".join(PROGRAM_HEADER.pack(1, flags, 0, vaddr, 0, 0, size, 1)
                              for vaddr, size, flags in segments)


class ElfBoundsTests(unittest.TestCase):
    def test_unaligned_interval_is_not_mem_size_ceil(self):
        bound = parse_elf(elf([(1, 4096, 4)]))
        self.assertEqual(bound["union_page_intervals_relative_to_base"], [[0, 8192]])
        self.assertEqual(bound["exclusive_segment_frames"], 2)
        self.assertEqual(bound["mapped_page_entries"], 2)
        self.assertEqual(bound["loader_reservation_expression_pages"], 1)
        self.assertEqual(bound["reservation_request_shortfall_entries"], 1)
        self.assertFalse(bound["reservation_request_covers_union_count"])
        self.assertFalse(bound["no_reallocation_proven"])

    def test_zero_size_loader_boundary_rule(self):
        self.assertEqual(page_interval(0, 0), (0, 0))
        self.assertEqual(page_interval(1, 0), (0, 4096))
        bound = parse_elf(elf([(0, 0, 4), (1, 0, 4)]))
        self.assertEqual(bound["interval_sum_pages"], 1)
        self.assertEqual(bound["union_pages"], 1)
        self.assertEqual(bound["loader_reservation_expression_pages"], 0)
        self.assertFalse(bound["reservation_request_covers_union_count"])

    def test_overlap_union_reuses_entries_and_writable_flags(self):
        bound = parse_elf(elf([(0, 8192, 4), (4097, 4096, 6), (4096, 4096, 4)]))
        self.assertEqual(bound["interval_sum_pages"], 5)
        self.assertEqual(bound["union_pages"], 3)
        self.assertEqual(bound["overlap_pages_in_interval_sum"], 2)
        self.assertEqual(bound["writable_page_entries"], 2)
        self.assertEqual(bound["writable_array_reservation_entries"], 3)
        self.assertEqual(bound["union_page_intervals_relative_to_base"], [[0, 12288]])
        self.assertTrue(bound["reservation_request_covers_union_count"])
        self.assertFalse(bound["no_reallocation_proven"])

    def test_adjacent_intervals_do_not_double_count(self):
        bound = parse_elf(elf([(0, 4096, 5), (4096, 4096, 6)]))
        self.assertEqual(bound["interval_sum_pages"], bound["union_pages"])
        self.assertEqual(bound["union_pages"], 2)

    def test_overlapping_permissions_fail_loader_wx_rule(self):
        for segments in ([(0, 4096, 7)], [(0, 1, 5), (1, 1, 6)]):
            with self.subTest(segments=segments), self.assertRaises(InvalidCertificate):
                parse_elf(elf(segments))

    def test_truncated_and_wrong_headers(self):
        valid = elf([(0, 4096, 4)])
        variants = [valid[:63], valid[:-1], elf([(0, 4096, 4)], elf_type=1),
                    elf([(0, 4096, 4)], machine=243)]
        for offset, value in ((0, 0), (4, 1), (5, 2), (6, 0)):
            modified = bytearray(valid)
            modified[offset] = value
            variants.append(modified)
        for offset, value in ((52, 63), (54, 55), (56, 0), (56, 0xFFFF)):
            modified = bytearray(valid)
            struct.pack_into("<H", modified, offset, value)
            variants.append(modified)
        for modified in variants:
            with self.subTest(header=bytes(modified[:64])), self.assertRaises(InvalidCertificate):
                parse_elf(modified)

    def test_table_offset_checked_before_unpacking(self):
        valid = elf([(0, 4096, 4)])
        for offset in (8, 65, 128, U64_MAX - 7):
            modified = bytearray(valid)
            struct.pack_into("<Q", modified, 32, offset)
            with self.subTest(offset=offset), self.assertRaises(InvalidCertificate):
                parse_elf(modified)

    def test_file_ranges_are_checked_even_with_no_payload(self):
        valid = elf([(0, 4096, 4)])
        variants = []
        for offset, filesz, memsz in ((len(valid), 1, 4096), (U64_MAX, 1, 4096),
                                    (0, 2, 1), (len(valid) + 1, 0, 4096)):
            modified = bytearray(valid)
            struct.pack_into("<Q", modified, 64 + 8, offset)
            struct.pack_into("<Q", modified, 64 + 32, filesz)
            struct.pack_into("<Q", modified, 64 + 40, memsz)
            variants.append(modified)
        for modified in variants:
            with self.subTest(phdr=bytes(modified[64:])), self.assertRaises(InvalidCertificate):
                parse_elf(modified)
        modified = bytearray(valid)
        struct.pack_into("<Q", modified, 64 + 8, len(valid))
        self.assertEqual(parse_elf(modified)["union_pages"], 1)

    def test_uint64_and_user_window_edges(self):
        self.assertEqual(checked_add(U64_MAX, 0, "test"), U64_MAX)
        self.assertEqual(checked_mul(U64_MAX, 1, "test"), U64_MAX)
        for fn, args in ((checked_add, (U64_MAX, 1, "test")),
                         (checked_mul, (U64_MAX, 2, "test")),
                         (page_interval, (U64_MAX, 0, 1)),
                         (page_interval, (U64_MAX - 4095, 8192)),
                         (page_interval, (USER_VADDR_MAX, 0)),
                         (page_interval, (USER_VADDR_MAX - 1, 2))):
            with self.subTest(fn=fn.__name__, args=args), self.assertRaises(InvalidCertificate):
                fn(*args)
        self.assertEqual(page_interval(USER_VADDR_MAX - 1, 1),
                         (USER_VADDR_MAX - 4096, USER_VADDR_MAX))

    def test_pie_highest_slot_is_checked_not_just_relative_va(self):
        # Fixed-zero can reach the user-window end; adding any PIE base cannot.
        self.assertEqual(parse_elf(elf([(USER_VADDR_MAX - 4096, 4096, 4)], elf_type=2))["union_pages"], 1)
        with self.assertRaises(InvalidCertificate):
            parse_elf(elf([(USER_VADDR_MAX - 4096, 4096, 4)]))


if __name__ == "__main__":
    unittest.main()
