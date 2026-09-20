#!/usr/bin/env python3
"""Write MBR partition table for Raspberry Pi 3 SD image.

Partition 1: FAT32 LBA (type 0x0C), start = 2048, size = 524288 (256 MiB), bootable
Partition 2: FAT32 LBA (type 0x0C), start = 526336, size = 524288 (256 MiB)
"""
import struct
import sys

def pack_entry(ptype: int, start: int, size: int, bootable: bool = False) -> bytes:
    status = 0x80 if bootable else 0x00
    return struct.pack(
        "<B3sB3sII",
        status,
        b"\xFF\xFF\xFF",
        ptype,
        b"\xFF\xFF\xFF",
        start,
        size,
    )

def main() -> None:
    if len(sys.argv) != 2:
        sys.exit("Usage: python3 write-rpi3-mbr.py <disk_image>")
    img = sys.argv[1]

    # P1: boot (bootable), P2: data/cells
    p1 = pack_entry(0x0C, 2048, 524288, bootable=True)
    p2 = pack_entry(0x0C, 526336, 524288, bootable=False)
    p3 = b"\x00" * 16
    p4 = b"\x00" * 16

    table = p1 + p2 + p3 + p4
    assert len(table) == 64

    with open(img, "r+b") as f:
        f.seek(446)
        f.write(table)
        f.seek(510)
        f.write(b"\x55\xAA")

    print("[write-rpi3-mbr] MBR written successfully:")
    print("  P1 (BOOT): type=0x0C start=2048 size=524288 (bootable)")
    print("  P2 (CELL): type=0x0C start=526336 size=524288")

if __name__ == "__main__":
    main()
