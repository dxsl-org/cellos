#!/usr/bin/env python3
"""Add or update a cell ELF in the Cellos cell bootstrap table on a disk image."""
import struct
import sys
import os

SECTOR_SIZE = 512
CELL_TABLE_BASE_LBA = 526_336
CELL_TABLE_MAGIC = 0x5649_4F53_5F43_454C
CELL_PATH_LEN = 64
MAX_CELL_ENTRIES = 64

def pack_header(count: int) -> bytes:
    return struct.pack("<QI", CELL_TABLE_MAGIC, count) + b"\x00" * 500

def pack_entry(path: str, data_lba: int, data_size: int) -> bytes:
    path_bytes = path.encode("utf-8")
    return path_bytes + b"\x00" * (CELL_PATH_LEN - len(path_bytes)) + struct.pack("<QQ", data_lba, data_size) + b"\x00" * 432

def sectors_for(size: int) -> int:
    return (size + SECTOR_SIZE - 1) // SECTOR_SIZE

def main():
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <disk.img> <path1>=<elf1> ...")
        sys.exit(1)

    disk_img = sys.argv[1]
    additions = sys.argv[2:]

    # 1. Read existing cells
    existing_cells = {}
    with open(disk_img, "rb") as f:
        f.seek(CELL_TABLE_BASE_LBA * SECTOR_SIZE)
        hdr = f.read(512)
        magic, count = struct.unpack_from("<QI", hdr)
        if magic == CELL_TABLE_MAGIC:
            for _ in range(count):
                entry = f.read(512)
                path = entry[:CELL_PATH_LEN].split(b"\x00", 1)[0].decode("utf-8")
                data_lba, data_size = struct.unpack_from("<QQ", entry, CELL_PATH_LEN)
                pos = f.tell()
                f.seek(data_lba * SECTOR_SIZE)
                data = f.read(data_size)
                f.seek(pos)
                existing_cells[path] = data

    # 2. Add or update cells
    for item in additions:
        path, elf_path = item.split("=", 1)
        with open(elf_path, "rb") as ef:
            existing_cells[path] = ef.read()
        print(f"Adding/Updating {path} from {elf_path} ({len(existing_cells[path])} bytes)")

    if len(existing_cells) > MAX_CELL_ENTRIES:
        print(f"ERROR: too many cells ({len(existing_cells)} > {MAX_CELL_ENTRIES})")
        sys.exit(1)

    # 3. Pack and write back
    cell_list = list(existing_cells.items())
    data_start_lba = CELL_TABLE_BASE_LBA + 1 + len(cell_list)
    current_lba = data_start_lba

    packed_entries = []
    data_blobs = []
    for cell_path, data in cell_list:
        packed_entries.append(pack_entry(cell_path, current_lba, len(data)))
        data_blobs.append((current_lba, data))
        current_lba += sectors_for(len(data))

    total_sectors = current_lba * SECTOR_SIZE
    with open(disk_img, "r+b") as f:
        f.seek(0, 2)
        if f.tell() < total_sectors:
            f.write(b"\x00" * (total_sectors - f.tell()))

        # Write header
        f.seek(CELL_TABLE_BASE_LBA * SECTOR_SIZE)
        f.write(pack_header(len(cell_list)))

        # Write entries
        for entry in packed_entries:
            f.write(entry)

        # Write data
        for lba, data in data_blobs:
            f.seek(lba * SECTOR_SIZE)
            f.write(data)
            remainder = len(data) % SECTOR_SIZE
            if remainder:
                f.write(b"\x00" * (SECTOR_SIZE - remainder))

    print(f"Successfully updated {disk_img} with {len(cell_list)} cells.")

if __name__ == "__main__":
    main()
