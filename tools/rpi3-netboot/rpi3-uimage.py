#!/usr/bin/env python3
"""Build and verify the legacy ARM64 uImage consumed by U-Boot bootm."""

from __future__ import annotations

import argparse
import struct
import zlib
from pathlib import Path

MAGIC = 0x27051956
LOAD_ADDRESS = 0x00080000
HEADER = struct.Struct(">7I4B32s")
OS_LINUX = 5
ARCH_ARM64 = 22
TYPE_KERNEL = 2
COMP_NONE = 0


def create_uimage(payload: bytes, name: str = "Cellos") -> bytes:
    encoded_name = name.encode("ascii", "strict")[:31].ljust(32, b"\0")
    fields = (
        MAGIC,
        0,
        0,
        len(payload),
        LOAD_ADDRESS,
        LOAD_ADDRESS,
        zlib.crc32(payload) & 0xFFFFFFFF,
        OS_LINUX,
        ARCH_ARM64,
        TYPE_KERNEL,
        COMP_NONE,
        encoded_name,
    )
    header = HEADER.pack(*fields)
    header_crc = zlib.crc32(header) & 0xFFFFFFFF
    return HEADER.pack(MAGIC, header_crc, *fields[2:]) + payload


def verify_uimage(image: bytes) -> bytes:
    if len(image) < HEADER.size:
        raise ValueError("uImage is shorter than its header")
    fields = list(HEADER.unpack(image[: HEADER.size]))
    magic, expected_header_crc, _, size, load, entry, data_crc = fields[:7]
    if magic != MAGIC:
        raise ValueError("invalid uImage magic")
    fields[1] = 0
    if zlib.crc32(HEADER.pack(*fields)) & 0xFFFFFFFF != expected_header_crc:
        raise ValueError("uImage header CRC mismatch")
    payload = image[HEADER.size :]
    if len(payload) != size:
        raise ValueError("uImage payload size mismatch")
    if zlib.crc32(payload) & 0xFFFFFFFF != data_crc:
        raise ValueError("uImage payload CRC mismatch")
    if (load, entry) != (LOAD_ADDRESS, LOAD_ADDRESS):
        raise ValueError("uImage load or entry address is not 0x80000")
    if tuple(fields[7:11]) != (OS_LINUX, ARCH_ARM64, TYPE_KERNEL, COMP_NONE):
        raise ValueError("uImage is not an uncompressed ARM64 kernel")
    return payload


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify", type=Path)
    args = parser.parse_args()
    if args.verify:
        payload = verify_uimage(args.verify.read_bytes())
        print(f"PASS: {args.verify} payload={len(payload)} bytes")
        return
    if not args.input or not args.output:
        parser.error("--input and --output are required when not using --verify")
    payload = args.input.read_bytes()
    args.output.write_bytes(create_uimage(payload))
    verify_uimage(args.output.read_bytes())
    print(f"Wrote {args.output} payload={len(payload)} bytes")


if __name__ == "__main__":
    main()
