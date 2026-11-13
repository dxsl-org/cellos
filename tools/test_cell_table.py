"""P2 cell-table writers: entry bound, P2 end bound, and layout parity.

Run: python3 -m unittest tools/test_cell_table.py
"""
import importlib.util
import re
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parent
SECTOR = 512


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), TOOLS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def rust_const(path, name):
    text = (ROOT / path).read_text(encoding="utf-8")
    match = re.search(rf"pub const {name}: \w+ = ([0-9_]+);", text)
    if match is None:
        raise AssertionError(f"{name} not found in {path}")
    return int(match.group(1).replace("_", ""))


WRITE = load("write-cell-table")
ADD = load("add-cell-to-disk")
MBR = load("write-mbr")
BASE = rust_const("libs/abi/src/disk.rs", "PART_CELLTBL_BASE_LBA")
SECTORS = rust_const("libs/abi/src/disk.rs", "PART_CELLTBL_SECTORS")
MAX_ENTRIES = rust_const("kernel/src/loader/disk_layout.rs", "MAX_CELL_ENTRIES")


def read_table(image):
    with open(image, "rb") as f:
        f.seek(BASE * SECTOR)
        magic, count = struct.unpack_from("<QI", f.read(SECTOR))
        entries = []
        for _ in range(count):
            entry = f.read(SECTOR)
            path = entry[:64].split(b"\0", 1)[0].decode()
            entries.append((path, *struct.unpack_from("<QQ", entry, 64)))
    return magic, entries


class CellTableTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        # Sparse image ending exactly at P2's end: the writers never need to pad
        # it, and a blob past P2 would have to grow it.
        self.image = self.dir / "disk.img"
        with open(self.image, "wb") as f:
            f.truncate((BASE + SECTORS) * SECTOR)

    def tearDown(self):
        self.tmp.cleanup()

    def cells(self, count, size=700):
        rows = []
        for i in range(count):
            elf = self.dir / f"cell{i}"
            elf.write_bytes(bytes([i % 251]) * size)
            rows.append(f"/bin/cell{i}={elf}")
        return rows

    def run_tool(self, tool, *args):
        return subprocess.run(
            [sys.executable, str(TOOLS / f"{tool}.py"), str(self.image), *args],
            capture_output=True, text=True,
        )

    def assert_inside_p2(self, entries):
        first_data = BASE + 1 + len(entries)
        for path, lba, size in entries:
            end = lba + (size + SECTOR - 1) // SECTOR
            self.assertGreaterEqual(lba, first_data, path)
            self.assertLessEqual(end, BASE + SECTORS, path)

    def test_constants_match_the_rust_layout(self):
        for module in (WRITE, ADD):
            self.assertEqual(module.MAX_CELL_ENTRIES, MAX_ENTRIES)
            self.assertEqual(module.CELL_TABLE_BASE_LBA, BASE)
            self.assertEqual(module.CELL_TABLE_SECTORS, SECTORS)
        self.assertIn((0x7F, BASE, SECTORS), MBR.PARTITIONS)
        snapshot = rust_const("libs/abi/src/disk.rs", "PART_SNAPSHOT_BASE_LBA")
        self.assertEqual(snapshot, BASE + SECTORS)
        self.assertIn(
            (0x7D, snapshot, rust_const("libs/abi/src/disk.rs", "PART_SNAPSHOT_SECTORS")),
            MBR.PARTITIONS,
        )

    def test_write_accepts_65_entries(self):
        result = self.run_tool("write-cell-table", *self.cells(65))
        self.assertEqual(result.returncode, 0, result.stderr)
        magic, entries = read_table(self.image)
        self.assertEqual(magic, WRITE.CELL_TABLE_MAGIC)
        self.assertEqual(len(entries), 65)
        self.assert_inside_p2(entries)

    def test_write_refuses_more_than_the_bound(self):
        result = self.run_tool("write-cell-table", *self.cells(MAX_ENTRIES + 1, size=1))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("too many cells", result.stderr)

    def test_write_refuses_to_run_past_p2(self):
        big = self.dir / "big"
        with open(big, "wb") as f:
            f.truncate(SECTORS * SECTOR)  # header + entry push it past P2's end
        before = self.image.stat().st_size
        result = self.run_tool("write-cell-table", f"/bin/big={big}")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("P2 holds", result.stderr)
        self.assertEqual(self.image.stat().st_size, before, "nothing written past P2")
        self.assertEqual(read_table(self.image)[0], 0, "no table written")

    def test_add_grows_a_full_table_to_65(self):
        self.assertEqual(self.run_tool("write-cell-table", *self.cells(64)).returncode, 0)
        script = self.dir / "smoke.lua"
        script.write_text("print(1)\n", encoding="utf-8")
        result = self.run_tool("add-cell-to-disk", f"/bin/smoke.lua={script}")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        _, entries = read_table(self.image)
        self.assertEqual(len(entries), 65)
        self.assertEqual(entries[-1][0], "/bin/smoke.lua")
        self.assert_inside_p2(entries)

    def test_add_refuses_to_run_past_p2(self):
        self.assertEqual(self.run_tool("write-cell-table", *self.cells(2)).returncode, 0)
        big = self.dir / "big"
        with open(big, "wb") as f:
            f.truncate(SECTORS * SECTOR)
        result = self.run_tool("add-cell-to-disk", f"/bin/big={big}")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("P2 holds", result.stdout)
        self.assertEqual(len(read_table(self.image)[1]), 2, "table left as it was")


if __name__ == "__main__":
    unittest.main()
