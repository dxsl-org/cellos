#!/usr/bin/env python3
"""Give the volatile Pi guest a usable shell by linking busybox's applets.

The stock Alpine `initramfs-virt` ships four executables (`busybox`, `kmod`,
`modprobe`, `sh`) and no applet symlinks, because it exists to fetch packages
over the network and mount a modloop - not to be a userspace. Booting it with
`rdinit=/bin/sh` therefore lands in a shell where `ls`, `cat` and `ps` are "not
found" even though the applets are compiled into the binary sitting right there
in `/bin`. This tool adds the symlinks (a few hundred bytes of headers, no new
payload) after verifying every name it links is actually in the busybox applet
table, so it can never create a dangling link.

The kernel's default init environment already carries
`PATH=/sbin:/usr/sbin:/bin:/usr/bin`, so links in `bin/` and `sbin/` are found
without any profile script.

It also adds the volatile profile's `/init`: the stock image has no `/proc`,
`/sys` or `/dev`, which is why `ps` and `free` have nothing to read. The script
mounts them and hands over to the shell, and it is where the app-launch profile
will exec its app instead.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
import posixpath
import sys

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("repack_initramfs", ROOT / "repack-initramfs.py")
assert spec is not None and spec.loader is not None
repack = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = repack
spec.loader.exec_module(repack)

BUSYBOX = "bin/busybox"

# Applets the guest shell is expected to have. Every name is checked against the
# busybox applet table before it is linked, so a missing applet fails the build
# instead of producing a link that resolves to "applet not found".
BIN_APPLETS = (
    "ash", "sh", "ls", "cat", "echo", "ps", "top", "uname", "dmesg", "free",
    "df", "du", "date", "sleep", "mkdir", "rmdir", "rm", "cp", "mv", "ln",
    "chmod", "stat", "touch", "find", "grep", "sed", "head", "tail", "wc",
    "cut", "sort", "uniq", "tr", "xargs", "basename", "dirname", "mktemp",
    "env", "printenv", "id", "whoami", "kill", "hostname", "hexdump", "od",
    "sync", "true", "false", "more", "less", "vi", "tar", "gzip", "ping",
    "nslookup", "wget", "poweroff", "reboot", "clear", "seq", "which",
    "udhcpc", "nc", "traceroute", "arp",
)
SBIN_APPLETS = (
    "mount", "umount", "ifconfig", "ip", "route", "netstat", "switch_root",
    "mdev", "modprobe", "insmod", "rmmod", "lsmod", "sysctl", "hwclock",
)


# The volatile profile's first process. `exec` keeps the shell as PID 1 so the
# console keeps working when the shell exits, and the prompt stays the stock
# `~ #` the boot gate matches.
INIT_SCRIPT = b"""#!/bin/sh
mkdir -p /proc /sys /dev /tmp
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev 2>/dev/null
# The guest DTB declares virtio-mmio devices (console, blk, net, gpu) and the
# initramfs carries their modules, but bypassing Alpine's init means nothing
# loads them: the guest came up with only `lo`. Load the transport and the NIC;
# the volatile profile has no disk to mount and no display userspace yet.
modprobe virtio_mmio 2>/dev/null
modprobe virtio_net 2>/dev/null
exec /bin/sh
"""


def applet_table(busybox: bytes) -> set[str]:
    """Applet names appear NUL-delimited in busybox's `applet_names` table."""
    names: set[str] = set()
    for field in busybox.split(b"\0"):
        if 1 <= len(field) <= 32 and all(33 <= byte <= 126 for byte in field):
            names.add(field.decode("ascii"))
    return names


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit("usage: prepare-rpi3-shell-initramfs.py INITRAMFS OUTPUT")
    source, output = map(Path, sys.argv[1:])

    entries = repack.read_archive(source)
    by_name = {entry.name: entry for entry in entries}
    if BUSYBOX not in by_name:
        sys.exit(f"ERROR: {source} has no {BUSYBOX}")

    table = applet_table(by_name[BUSYBOX].data)
    wanted = {f"bin/{name}": name for name in BIN_APPLETS}
    wanted.update({f"sbin/{name}": name for name in SBIN_APPLETS})
    missing = sorted(applet for applet in wanted.values() if applet not in table)
    if missing:
        sys.exit(f"ERROR: busybox in {source} has no applet: {' '.join(missing)}")

    next_inode = max(entry.fields[0] for entry in entries) + 1
    added = 0
    for path, applet in sorted(wanted.items()):
        if path in by_name:
            continue
        if path.rsplit("/", 1)[0] not in by_name:
            sys.exit(f"ERROR: {source} has no directory for {path}")
        # newc symlink: mode 0o120777, nlink 1, target in the payload. The
        # target is relative to the link's own directory, so a link in /sbin
        # points at ../bin/busybox and not at a /sbin/busybox that does not
        # exist (which is how the first cut made `mount: not found`).
        target = posixpath.relpath(BUSYBOX, path.rsplit("/", 1)[0])
        fields = [next_inode, 0o120777, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
        entry = repack.Entry(path, fields, target.encode())
        entries.append(entry)
        by_name[path] = entry
        next_inode += 1
        added += 1

    init = by_name.get("init")
    if init is None:
        fields = [next_inode, 0o100755, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
        entries.append(repack.Entry("init", fields, INIT_SCRIPT))
    else:
        init.data = INIT_SCRIPT
        init.fields[1] = 0o100755

    repack.write_archive(output, entries)
    print(f"added {added} applet symlink(s) to {output} ({source.stat().st_size} -> {output.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
