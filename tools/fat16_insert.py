#!/usr/bin/env python3
"""fat16_insert.py — Insert (or overwrite) a small file into an existing FAT16
image IN-PLACE, without disturbing existing files.

Unlike mkfat16.py (a formatter that writes an empty FS), this preserves the
directory + data region and just adds one root-directory file. Reads all geometry
from the BPB; updates every FAT copy; idempotent (re-inserting the same name
frees the old chain first). Single root-dir level only (no subdirectories).

Usage:
    python tools/fat16_insert.py <image> <src_file> <DOSNAME.EXT>
    e.g. python tools/fat16_insert.py kernel/src/embedded/kernel_fs.img POLICY.BIN POLICY.BIN
"""
import struct
import sys

FREE = 0x0000
EOC = 0xFFFF
DIR_ENTRY = 32


def u16(d, o):
    return struct.unpack_from("<H", d, o)[0]


def u32(d, o):
    return struct.unpack_from("<I", d, o)[0]


def dos_name(name):
    if "." in name:
        base, ext = name.split(".", 1)
    else:
        base, ext = name, ""
    if len(base) > 8 or len(ext) > 3:
        sys.exit(f"name {name!r} is not 8.3")
    return (base.ljust(8)[:8] + ext.ljust(3)[:3]).upper().encode("ascii")


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    img_path, src_path, name = sys.argv[1], sys.argv[2], sys.argv[3]
    d = bytearray(open(img_path, "rb").read())
    src = open(src_path, "rb").read()

    bps = u16(d, 0x0B)
    spc = d[0x0D]
    rsvd = u16(d, 0x0E)
    nfat = d[0x10]
    rootent = u16(d, 0x11)
    spf = u16(d, 0x16)
    ts = u16(d, 0x13) or u32(d, 0x20)
    if bps == 0 or spc == 0:
        sys.exit("not a FAT image (zero bps/spc)")

    clus_bytes = spc * bps
    fat_start = rsvd * bps  # byte offset of FAT #0
    root_start = (rsvd + nfat * spf) * bps
    root_sectors = (rootent * DIR_ENTRY + bps - 1) // bps
    data_start = root_start + root_sectors * bps  # byte offset of cluster 2
    data_sectors = ts - (rsvd + nfat * spf) - root_sectors
    max_cluster = data_sectors // spc + 1  # highest valid cluster index (2..=max)

    def fat_off(fat_i, clus):
        return (rsvd + fat_i * spf) * bps + clus * 2

    def fat_get(clus):
        return u16(d, fat_off(0, clus))

    def fat_set(clus, val):
        for fi in range(nfat):
            struct.pack_into("<H", d, fat_off(fi, clus), val)

    def clus_data_off(clus):
        return data_start + (clus - 2) * clus_bytes

    name83 = dos_name(name)

    # Locate an existing root entry with this name (overwrite) or a free slot.
    overwrite_off = None
    free_off = None
    for i in range(rootent):
        off = root_start + i * DIR_ENTRY
        first = d[off]
        if first in (0x00, 0xE5):
            if free_off is None:
                free_off = off
            if first == 0x00:
                break  # no entries beyond this point
            continue
        if d[off:off + 11] == name83:
            overwrite_off = off
            break

    # Free the old cluster chain if overwriting.
    if overwrite_off is not None:
        old = u16(d, overwrite_off + 0x1A)
        while 2 <= old <= max_cluster and fat_get(old) not in (FREE,):
            nxt = fat_get(old)
            fat_set(old, FREE)
            if nxt >= 0xFFF8 or nxt == 0 or nxt < 2:
                break
            old = nxt

    # Allocate the needed clusters.
    need = max(1, (len(src) + clus_bytes - 1) // clus_bytes)
    chain = []
    for c in range(2, max_cluster + 1):
        if fat_get(c) == FREE and c * DIR_ENTRY:  # any free
            chain.append(c)
            if len(chain) == need:
                break
    if len(chain) < need:
        sys.exit(f"not enough free clusters (need {need}, found {len(chain)})")

    # Write data + chain FATs.
    for idx, c in enumerate(chain):
        seg = src[idx * clus_bytes:(idx + 1) * clus_bytes]
        base = clus_data_off(c)
        d[base:base + len(seg)] = seg
        # zero the tail of the last cluster
        if len(seg) < clus_bytes:
            d[base + len(seg):base + clus_bytes] = b"\x00" * (clus_bytes - len(seg))
        fat_set(c, chain[idx + 1] if idx + 1 < len(chain) else EOC)

    # Write the directory entry.
    if overwrite_off is not None:
        off = overwrite_off
    elif free_off is not None:
        off = free_off
    else:
        sys.exit("root directory full")
    entry = bytearray(DIR_ENTRY)
    entry[0:11] = name83
    entry[0x0B] = 0x20  # attr: archive
    struct.pack_into("<H", entry, 0x1A, chain[0])  # first cluster low (FAT16: hi=0)
    struct.pack_into("<I", entry, 0x1C, len(src))  # file size
    d[off:off + DIR_ENTRY] = entry

    open(img_path, "wb").write(d)
    print(f"inserted {name} ({len(src)} B, {need} cluster(s) @ {chain}) into {img_path}", file=sys.stderr)


if __name__ == "__main__":
    main()
