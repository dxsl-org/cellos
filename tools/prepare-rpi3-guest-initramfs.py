#!/usr/bin/env python3
"""Add the pinned Alpine modloop's ext4 drivers and Pi guest init to initramfs."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("repack_initramfs", ROOT / "repack-initramfs.py")
assert spec is not None and spec.loader is not None
repack = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = repack
spec.loader.exec_module(repack)

# Alpine v3.21.3 netboot kernel; a kernel/artifact upgrade must update this
# pairing rather than silently packaging modules for the wrong kernel.
VERSION = "6.12.13-0-virt"
MODULES = (
    "kernel/lib/crc16.ko",
    "kernel/fs/mbcache.ko",
    "kernel/fs/jbd2/jbd2.ko",
    "kernel/fs/ext4/ext4.ko",
)


def main() -> None:
    if len(sys.argv) != 5:
        sys.exit("usage: prepare-rpi3-guest-initramfs.py INITRAMFS MODLOOP INIT OUTPUT")
    source, modloop, init_script, output = map(Path, sys.argv[1:])
    entries = repack.read_archive(source)
    by_name = {entry.name: entry for entry in entries}
    prefix = f"lib/modules/{VERSION}/"
    dep_path = prefix + "modules.dep"
    if dep_path not in by_name or b"kernel/drivers/block/virtio_blk.ko:" not in by_name[dep_path].data:
        sys.exit(f"ERROR: Alpine initramfs does not contain expected {VERSION} modules")

    modloop_prefix = f"modules/{VERSION}/"
    dep_result = subprocess.run(
        ["unsquashfs", "-cat", str(modloop), modloop_prefix + "modules.dep"],
        check=True, capture_output=True,
    )
    modloop_deps = dict(line.split(b":", 1) for line in dep_result.stdout.splitlines() if b":" in line)
    next_inode = max(entry.fields[0] for entry in entries) + 1

    def add(path: str, data: bytes, mode: int) -> None:
        nonlocal next_inode
        if path in by_name:
            sys.exit(f"ERROR: unexpected duplicate guest initramfs path: {path}")
        fields = [next_inode, mode, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
        entry = repack.Entry(path, fields, data)
        entries.append(entry)
        by_name[path] = entry
        next_inode += 1

    deps = bytearray(by_name[dep_path].data)
    if deps and deps[-1] != 10:
        deps.extend(b"\n")
    for module in MODULES:
        name = (modloop_prefix + module)
        payload = subprocess.run(
            ["unsquashfs", "-cat", str(modloop), name],
            check=True, capture_output=True,
        ).stdout
        if not payload.startswith(b"\x7fELF") or module.encode() not in modloop_deps:
            sys.exit(f"ERROR: missing Alpine ext4 module or dependency: {name}")
        path = prefix + module
        parent = path.rsplit("/", 1)[0]
        if parent not in by_name:
            add(parent, b"", 0o040755)
        add(path, payload, 0o100644)
        deps.extend(module.encode() + b":" + modloop_deps[module.encode()] + b"\n")
    by_name[dep_path].data = bytes(deps)
    add("bin/pi-guest-init", init_script.read_bytes(), 0o100755)
    repack.write_archive(output, entries)


if __name__ == "__main__":
    main()
