#!/usr/bin/env python3
"""Write a Raspberry Pi 3 MBR without changing the bootable FAT P1.

The legacy image keeps a second FAT partition; --tier3 uses Cellos' canonical
P2 cell table, P3 snapshot and P4 data layout instead.
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
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[2] != "--tier3"):
        sys.exit("Usage: python3 write-rpi3-mbr.py <disk_image> [--tier3]")
    img = sys.argv[1]
    tier3 = len(sys.argv) == 3

    p1 = pack_entry(0x0C, 2048, 524288, bootable=True)
    if tier3:
        p2 = pack_entry(0x7F, 526336, 33664)
        p3 = pack_entry(0x7D, 560000, 240000)
        p4 = pack_entry(0x7E, 800000, 131072)
    else:
        p2 = pack_entry(0x0C, 526336, 524288)
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
    print("  P1 (BOOT): type=0x0c start=2048 size=524288 (bootable)")
    print(f"  P2 (CELL): type={0x7F if tier3 else 0x0C:#04x} start=526336 size={33664 if tier3 else 524288}")
    if tier3:
        print("  P3 (SNAPSHOT): type=0x7d start=560000 size=240000")
        print("  P4 (DATA): type=0x7e start=800000 size=131072")

if __name__ == "__main__":
    main()
