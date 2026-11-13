#!/usr/bin/env python3
"""Disk layout single source of truth: `libs/abi/src/disk.rs`.

1. Reads the `pub const PART_*: u64` constants (refusing anything but plain
   integer literals), re-checks the contiguity the Rust `const _` asserts state,
   and keeps `scripts/disk-layout.json` equal to them (`--write` regenerates).
2. Checks every image/provisioning tool that still carries a literal copy: each
   named site below must exist and hold the canonical value. A partition move
   that misses one copy fails here instead of corrupting a neighbour partition.

Exit 0 when all match, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "libs/abi/src/disk.rs"
GENERATED = ROOT / "scripts/disk-layout.json"
CONST = re.compile(r"^pub const (PART_[A-Z0-9_]+): u64 = ([0-9_]+);", re.M)
NUM = r"([0-9][0-9_,]*)"

# (file, pattern with one numeric group, canonical key). `TOTAL` = end of P6.
SITES: list[tuple[str, str, str]] = [
    ("gen_disk.ps1", r"^\$disk_sectors = " + NUM, "TOTAL"),
    ("gen_disk.ps1", r"^\$CELLSTORE_BASE_LBA = " + NUM, "PART_CELLSTORE_BASE_LBA"),
    ("gen_disk.ps1", r"^\$CELLSTORE_SECTORS\s+= " + NUM, "PART_CELLSTORE_SECTORS"),
    ("gen_disk.ps1", r"mkfat32_inplace\.py\"[^\n]*\"disk_v3\.img\" " + NUM, "PART_FAT32_SECTORS"),
    ("gen_disk.ps1", r"mkfat32_inplace\.py\"[^\n]*\"disk_v3\.img\" [0-9]+ " + NUM, "PART_FAT32_BASE_LBA"),
    ("scripts/gen-disk-ci.sh", r"^DISK_SECTORS=" + NUM, "TOTAL"),
    ("scripts/gen-disk-ci.sh", r"^CELLSTORE_BASE_LBA=" + NUM, "PART_CELLSTORE_BASE_LBA"),
    ("scripts/gen-disk-ci.sh", r"^CELLSTORE_SECTORS=" + NUM, "PART_CELLSTORE_SECTORS"),
    ("scripts/gen-disk-ci.sh", r"\"\$NEW_DISK\" " + NUM + r" [0-9]+", "PART_FAT32_SECTORS"),
    ("scripts/gen-disk-ci.sh", r"\"\$NEW_DISK\" [0-9]+ " + NUM, "PART_FAT32_BASE_LBA"),
    ("scripts/gen-disk-ci.sh", r"BASE=" + NUM + r"; MAGIC=", "PART_CELLTBL_BASE_LBA"),
    ("scripts/gen-disk-aarch64-ci.sh", r"^DISK_SECTORS=" + NUM, "TOTAL"),
    ("scripts/gen-disk-aarch64-ci.sh", r"^CELLSTORE_BASE_LBA=" + NUM, "PART_CELLSTORE_BASE_LBA"),
    ("scripts/gen-disk-aarch64-ci.sh", r"^CELLSTORE_SECTORS=" + NUM, "PART_CELLSTORE_SECTORS"),
    ("scripts/gen-disk-aarch64-ci.sh", r"\"\$NEW_DISK\" " + NUM + r" [0-9]+", "PART_FAT32_SECTORS"),
    ("scripts/gen-disk-aarch64-ci.sh", r"\"\$NEW_DISK\" [0-9]+ " + NUM, "PART_FAT32_BASE_LBA"),
    ("scripts/gen-disk-aarch64-ci.sh", r"BASE=" + NUM + r"; MAGIC=", "PART_CELLTBL_BASE_LBA"),
    ("scripts/format-disk-arm.sh", r"^PART_FAT32_BASE_LBA=" + NUM, "PART_FAT32_BASE_LBA"),
    ("scripts/format-disk-arm.sh", r"^PART_FAT32_SECTORS=" + NUM, "PART_FAT32_SECTORS"),
    ("scripts/format-disk-arm.sh", r"^PART_SRV_BASE_LBA=" + NUM, "PART_SRV_BASE_LBA"),
    ("scripts/format-disk-arm.sh", r"^PART_SRV_SECTORS=" + NUM, "PART_SRV_SECTORS"),
    ("scripts/format-disk-arm.sh", r"^PART_CELLSTORE_BASE_LBA=" + NUM, "PART_CELLSTORE_BASE_LBA"),
    ("scripts/format-disk-arm.sh", r"^PART_CELLSTORE_SECTORS=" + NUM, "PART_CELLSTORE_SECTORS"),
    ("scripts/mksrv-img.sh", r"^PART_SRV_BASE_LBA=" + NUM, "PART_SRV_BASE_LBA"),
    ("scripts/mksrv-img.sh", r"^PART_SRV_SECTORS=" + NUM, "PART_SRV_SECTORS"),
    ("tools/cellos-fs-provision/provision.sh", r"for BASE in " + NUM + r" [0-9]+", "PART_LFS_BASE_LBA"),
    ("tools/cellos-fs-provision/provision.sh", r"for BASE in [0-9]+ " + NUM, "PART_SRV_BASE_LBA"),
    ("tools/cellos-fs-provision/provision.sh", r"--sectors " + NUM, "PART_LFS_SECTORS"),
    ("tools/cellos-fs-provision/provision.sh", r"--sectors " + NUM, "PART_SRV_SECTORS"),
    ("tools/mkfat32_inplace.py", r"^CELL_TABLE_BASE_LBA = " + NUM, "PART_CELLTBL_BASE_LBA"),
    ("tools/write-mbr.py", r"\(0x0C,\s+" + NUM + r",", "PART_FAT32_BASE_LBA"),
    ("tools/write-mbr.py", r"\(0x0C,\s+[0-9_]+,\s+" + NUM + r"\)", "PART_FAT32_SECTORS"),
    ("tools/write-mbr.py", r"\(0x7F,\s+" + NUM + r",", "PART_CELLTBL_BASE_LBA"),
    ("tools/write-mbr.py", r"\(0x7F,\s+[0-9_]+,\s+" + NUM + r"\)", "PART_CELLTBL_SECTORS"),
    ("tools/write-mbr.py", r"\(0x7D,\s+" + NUM + r",", "PART_SNAPSHOT_BASE_LBA"),
    ("tools/write-mbr.py", r"\(0x7D,\s+[0-9_]+,\s+" + NUM + r"\)", "PART_SNAPSHOT_SECTORS"),
    ("tools/write-mbr.py", r"\(0x7E,\s+" + NUM + r",", "PART_LFS_BASE_LBA"),
    ("tools/write-mbr.py", r"\(0x7E,\s+[0-9_]+,\s+" + NUM + r"\)", "PART_LFS_SECTORS"),
    ("tools/write-mbr.py", r"^Total disk: " + NUM + r" sectors", "TOTAL"),
    ("tools/write-rpi3-mbr.py", r"p1 = pack_entry\(0x0C, " + NUM + r", ", "PART_FAT32_BASE_LBA"),
    ("tools/write-rpi3-mbr.py", r"p1 = pack_entry\(0x0C, [0-9]+, " + NUM + r",", "PART_FAT32_SECTORS"),
    ("tools/write-rpi3-mbr.py", r"pack_entry\(0x7F, " + NUM + r", ", "PART_CELLTBL_BASE_LBA"),
    ("tools/write-rpi3-mbr.py", r"pack_entry\(0x7F, [0-9]+, " + NUM + r"\)", "PART_CELLTBL_SECTORS"),
    ("tools/write-rpi3-mbr.py", r"pack_entry\(0x7D, " + NUM + r", ", "PART_SNAPSHOT_BASE_LBA"),
    ("tools/write-rpi3-mbr.py", r"pack_entry\(0x7D, [0-9]+, " + NUM + r"\)", "PART_SNAPSHOT_SECTORS"),
    ("tools/write-rpi3-mbr.py", r"pack_entry\(0x7E, " + NUM + r", ", "PART_LFS_BASE_LBA"),
    ("tools/write-rpi3-mbr.py", r"pack_entry\(0x7E, [0-9]+, " + NUM + r"\)", "PART_LFS_SECTORS"),
]

ORDER = ["FAT32", "CELLTBL", "SNAPSHOT", "LFS", "SRV", "CELLSTORE"]


def canonical() -> dict[str, int]:
    found = {name: int(value.replace("_", "")) for name, value in CONST.findall(SOURCE.read_text())}
    expected = {f"PART_{p}_{s}" for p in ORDER for s in ("BASE_LBA", "SECTORS")}
    if set(found) != expected:
        raise SystemExit(f"FAIL {SOURCE}: constants {sorted(set(found) ^ expected)} missing or unexpected")
    for left, right in zip(ORDER, ORDER[1:]):
        if found[f"PART_{left}_BASE_LBA"] + found[f"PART_{left}_SECTORS"] != found[f"PART_{right}_BASE_LBA"]:
            raise SystemExit(f"FAIL {SOURCE}: {left} does not end where {right} starts")
    found["TOTAL"] = found["PART_CELLSTORE_BASE_LBA"] + found["PART_CELLSTORE_SECTORS"]
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--write", action="store_true", help="regenerate scripts/disk-layout.json")
    args = parser.parse_args()

    layout = canonical()
    rendered = json.dumps({"source": "libs/abi/src/disk.rs", "sector_bytes": 512, **layout},
                          indent=2) + "\n"
    errors: list[str] = []
    if args.write:
        GENERATED.write_text(rendered)
    elif not GENERATED.exists() or GENERATED.read_text() != rendered:
        errors.append(f"{GENERATED.relative_to(ROOT)} is stale — run with --write")

    for rel, pattern, key in SITES:
        text = (ROOT / rel).read_text(errors="replace")
        values = re.findall(pattern, text, re.M)
        if not values:
            errors.append(f"{rel}: site for {key} not found (pattern {pattern!r})")
        for value in values:
            number = int(value.replace("_", "").replace(",", ""))
            if number != layout[key]:
                errors.append(f"{rel}: {key} is {number}, canonical {layout[key]}")

    for error in errors:
        print(f"FAIL {error}")
    if errors:
        return 1
    print(f"disk layout: OK ({len(SITES)} literal sites match {SOURCE.relative_to(ROOT)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
