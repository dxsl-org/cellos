#!/usr/bin/env python3
"""Replace or add files in a gzip-compressed newc initramfs."""

from __future__ import annotations

import argparse
import gzip
from dataclasses import dataclass
from pathlib import Path

HEADER_LEN = 110
TRAILER = "TRAILER!!!"


@dataclass
class Entry:
    name: str
    fields: list[int]
    data: bytes


def align4(value: int) -> int:
    return (value + 3) & ~3


def read_archive(path: Path) -> list[Entry]:
    raw = gzip.decompress(path.read_bytes())
    entries: list[Entry] = []
    offset = 0
    while offset + HEADER_LEN <= len(raw):
        header = raw[offset : offset + HEADER_LEN]
        if header[:6] != b"070701":
            raise ValueError(f"unsupported cpio magic at offset {offset}")
        fields = [
            int(header[6 + index * 8 : 14 + index * 8], 16)
            for index in range(13)
        ]
        offset += HEADER_LEN
        name_size = fields[11]
        name = raw[offset : offset + name_size - 1].decode("utf-8")
        offset = align4(offset + name_size)
        file_size = fields[6]
        data = raw[offset : offset + file_size]
        offset = align4(offset + file_size)
        if name == TRAILER:
            break
        entries.append(Entry(name, fields, data))
    return entries


def encoded(entry: Entry) -> bytes:
    name = entry.name.encode("utf-8") + b"\0"
    fields = entry.fields.copy()
    fields[6] = len(entry.data)
    fields[11] = len(name)
    fields[12] = 0
    header = b"070701" + b"".join(f"{value:08x}".encode() for value in fields)
    output = bytearray(header)
    output.extend(name)
    output.extend(b"\0" * (align4(len(output)) - len(output)))
    output.extend(entry.data)
    output.extend(b"\0" * (align4(len(output)) - len(output)))
    return bytes(output)


def write_archive(path: Path, entries: list[Entry]) -> None:
    output = bytearray()
    for entry in entries:
        output.extend(encoded(entry))
    trailer_fields = [0, 0o100644, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
    output.extend(encoded(Entry(TRAILER, trailer_fields, b"")))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(gzip.compress(bytes(output), compresslevel=9, mtime=0))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--add",
        action="append",
        nargs=3,
        metavar=("ARCHIVE_PATH", "HOST_FILE", "MODE"),
        default=[],
    )
    args = parser.parse_args()
    entries = read_archive(args.source)
    by_name = {entry.name: entry for entry in entries}
    next_inode = max((entry.fields[0] for entry in entries), default=0) + 1
    for archive_path, host_file, mode in args.add:
        name = archive_path.lstrip("/")
        data = Path(host_file).read_bytes()
        if name in by_name:
            entry = by_name[name]
            entry.data = data
            entry.fields[1] = int(mode, 8)
        else:
            fields = [next_inode, int(mode, 8), 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
            entry = Entry(name, fields, data)
            entries.append(entry)
            by_name[name] = entry
            next_inode += 1
    write_archive(args.output, entries)


if __name__ == "__main__":
    main()
