import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from elf_manifest import InspectionError, inspect_elf_bytes

NAMES = b"\0.shstrtab\0__ViCell_manifest\0"
MAGIC = (0x56494345).to_bytes(4, "little")


def manifest(version=2, size=16, protection=1, flags=0x0FFF):
    record = bytearray(size)
    record[: min(4, size)] = MAGIC[: min(4, size)]
    if size > 4:
        record[4] = version
    if version == 1 and size > 5:
        record[5] = flags & 0xFF
    elif version == 2 and size >= 8:
        record[5] = protection
        record[6:8] = flags.to_bytes(2, "little")
    return bytes(record)


def put(data, offset, value, width):
    data[offset : offset + width] = value.to_bytes(width, "little")


def elf64(record=None, duplicate=False):
    shoff, record_offset = 128, 96
    count = 2 + int(record is not None) + int(duplicate)
    data = bytearray(shoff + count * 64)
    data[:7] = b"\x7fELF\x02\x01\x01"
    put(data, 20, 1, 4)
    put(data, 24, 0x401000, 8)
    put(data, 40, shoff, 8)
    put(data, 52, 64, 2)
    put(data, 58, 64, 2)
    put(data, 60, count, 2)
    put(data, 62, 1, 2)
    data[64 : 64 + len(NAMES)] = NAMES
    section64(data, shoff, 1, 1, 3, 64, len(NAMES))
    if record is not None:
        data[record_offset : record_offset + len(record)] = record
        section64(data, shoff, 2, 11, 1, record_offset, len(record))
        if duplicate:
            section64(data, shoff, 3, 11, 1, record_offset, len(record))
    return bytes(data)


def section64(data, shoff, index, name, kind, offset, size):
    base = shoff + index * 64
    put(data, base, name, 4)
    put(data, base + 4, kind, 4)
    put(data, base + 24, offset, 8)
    put(data, base + 32, size, 8)


def elf32(record):
    shoff, record_offset = 104, 84
    data = bytearray(shoff + 3 * 40)
    data[:7] = b"\x7fELF\x01\x01\x01"
    put(data, 20, 1, 4)
    put(data, 24, 0x401000, 4)
    put(data, 32, shoff, 4)
    put(data, 40, 52, 2)
    put(data, 46, 40, 2)
    put(data, 48, 3, 2)
    put(data, 50, 1, 2)
    data[52 : 52 + len(NAMES)] = NAMES
    data[record_offset : record_offset + len(record)] = record
    for index, name, kind, offset, size in [
        (1, 1, 3, 52, len(NAMES)),
        (2, 11, 1, record_offset, len(record)),
    ]:
        base = shoff + index * 40
        put(data, base, name, 4)
        put(data, base + 4, kind, 4)
        put(data, base + 16, offset, 4)
        put(data, base + 20, size, 4)
    return bytes(data)


class ElfInspectorTests(unittest.TestCase):
    def run_cli(self, data):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "cell.elf"
            path.write_bytes(data)
            before = path.read_bytes()
            result = subprocess.run(
                [sys.executable, str(TOOLS / "check_elf.py"), str(path)],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(path.read_bytes(), before, "inspector mutated its input")
            return result

    def test_v2_labels_are_separate_and_honest(self):
        result = self.run_cli(elf64(manifest(flags=(1 << 0) | (1 << 10))))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Execution tier: unknown", result.stdout)
        self.assertIn("Runtime profile: unknown", result.stdout)
        self.assertIn("Protection class: standard", result.stdout)
        self.assertIn("Capabilities: block-io, i2c", result.stdout)
        self.assertIn("Evidence: manifest v2 section", result.stdout)
        self.assertNotIn("Tier 2", result.stdout)

    def test_v1_and_absence_remain_explicit(self):
        old = self.run_cli(elf64(manifest(version=1, size=8, flags=0x81)))
        self.assertEqual(old.returncode, 0, old.stderr)
        self.assertIn("Protection class: legacy", old.stdout)
        self.assertIn("block-io, partition-lfs", old.stdout)
        absent = self.run_cli(elf64())
        self.assertEqual(absent.returncode, 0, absent.stderr)
        self.assertIn("manifest absent; legacy loader policy applies", absent.stdout)
        self.assertIn("Capabilities: not asserted", absent.stdout)

    def test_usage_error_is_nonzero(self):
        result = subprocess.run(
            [sys.executable, str(TOOLS / "check_elf.py")],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertTrue(result.stderr.startswith("usage:"))

    def test_supported_elf_classes(self):
        self.assertEqual(inspect_elf_bytes(elf32(manifest())).elf_class, 32)
        self.assertEqual(inspect_elf_bytes(elf64(manifest())).elf_class, 64)

    def test_malformed_manifest_and_metadata_are_nonzero(self):
        cases = [
            elf64(manifest(size=15)),
            elf64(manifest(size=17)),
            elf64(manifest(version=3)),
            elf64(manifest(protection=4)),
            elf64(manifest(flags=0x1000)),
            elf64(manifest(), duplicate=True),
            b"not an elf",
        ]
        reserved = bytearray(manifest())
        reserved[12] = 1
        cases.append(elf64(bytes(reserved)))
        reserved_v1 = bytearray(manifest(version=1, size=8))
        reserved_v1[7] = 1
        cases.append(elf64(bytes(reserved_v1)))
        bad_table = bytearray(elf64(manifest()))
        put(bad_table, 40, (1 << 64) - 1, 8)
        cases.append(bytes(bad_table))
        for case in cases:
            result = self.run_cli(case)
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(result.stderr.startswith("error:"), result.stderr)

    def test_bounded_mutation_and_truncation_corpus_never_escapes(self):
        base = elf64(manifest())
        corpus = [base[:length] for length in range(len(base) + 1)]
        for index in range(0, len(base), 7):
            changed = bytearray(base)
            changed[index] ^= 0xA5
            corpus.append(bytes(changed))
        for candidate in corpus:
            try:
                inspect_elf_bytes(candidate)
            except InspectionError:
                pass


if __name__ == "__main__":
    unittest.main()
