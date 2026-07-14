#!/usr/bin/env python3
"""Unsafe ratchet gate for cells/ (CI replacement for cargo-geiger).

Law 4 says Cells are `#![forbid(unsafe_code)]`. Reality: Driver Cells need
`unsafe` for MMIO/DMA, and a handful of FFI/runtime cells (mlibc, Lua, DOOM)
carry documented exemptions. cargo-geiger cannot analyse `-Z build-std`
bare-metal targets, so this gate enforces the real invariant statically:

  * Any .rs file under cells/ that contains the `unsafe` keyword MUST be in
    the ALLOWLIST below. A new file with unsafe fails CI — either remove the
    unsafe or add the file here with a justification comment in the same PR.
  * Allowlisted files that no longer contain unsafe are reported so the list
    can be tightened (warning only — the ratchet never loosens itself).

Detection strips `//` line comments and `/* ... */` block comments, then
matches the word `unsafe` (`unsafe_code` in lint attributes does not match).
"""

import re
import subprocess
import sys

# Files currently containing unsafe, grouped by exemption class.
# Snapshot taken 2026-07-14 — shrink this list, never grow it casually.
ALLOWLIST = {
    # Driver Cells — MMIO/DMA hardware access (Kernel Boundary Law moved
    # drivers out of the kernel; the hardware boundary is IOMMU, not LBI).
    "cells/drivers/e1000/src/controller.rs",
    "cells/drivers/nvme/src/controller.rs",
    "cells/drivers/nvme/src/dispatch.rs",
    "cells/drivers/nvme/src/queue.rs",
    "cells/drivers/virtio-blk/src/device.rs",
    "cells/drivers/virtio-gpu/src/cursor.rs",
    "cells/drivers/virtio-gpu/src/display.rs",
    "cells/drivers/virtio-net/src/device.rs",
    "cells/services/input/src/virtio_device.rs",
    # Hypervisor / Silo — EL2/VMX world switch and guest mailboxes.
    "cells/services/hypervisor/src/timer.rs",
    "cells/services/hypervisor/src/vmm.rs",
    "cells/services/silo/src/vmm.rs",
    "cells/guests/silo-guest/src/mailbox.rs",
    "cells/guests/silo-guest/src/main.rs",
    "cells/tests/hypervisor-test/src/main.rs",
    "cells/tests/silo-test/src/main.rs",
    # C FFI (Tier 1b) — extern "C" calls are inherently unsafe.
    "cells/demos/doom/src/main.rs",
    "cells/demos/tetris-c/src/main.rs",
    "cells/demos/tetris-lua/src/ffi.rs",
    "cells/demos/tetris-lua/src/main.rs",
    "cells/runtimes/lua/src/bindings_io.rs",
    "cells/runtimes/lua/src/bindings_net.rs",
    "cells/runtimes/lua/src/bindings_vfs.rs",
    "cells/runtimes/lua/src/ffi.rs",
    "cells/runtimes/lua/src/main.rs",
    "cells/runtimes/lua/src/repl_session.rs",
    "cells/tests/c-math-smoke/src/main.rs",
    "cells/tests/mlibc-smoke/src/main.rs",
    "cells/tests/posix-shim-test/src/main.rs",
    # Legacy / tracked tech debt — raw-pointer plumbing that predates the
    # ratchet. Do NOT add new entries of this class; refactor instead.
    "cells/demos/audio-demo/src/main.rs",
    "cells/demos/cfi-test/src/main.rs",
    "cells/services/compositor/src/surface_table.rs",
    "cells/services/net/src/handlers.rs",
    "cells/services/net/src/tls/socket.rs",
    "cells/services/net/src/tls/transport.rs",
    "cells/services/vfs/kernel-fs-legacy.rs",
    "cells/services/vfs/src/backend_redoxfs.rs",
    "cells/services/vfs/src/disk_redoxfs.rs",
    "cells/services/vfs/src/dispatch.rs",
    "cells/services/vfs/src/lfs_string_shim.rs",
    "cells/services/vfs/src/main.rs",
    "cells/tools/shell/src/cmd_fs.rs",
    "cells/tools/shell/src/commands.rs",
    "cells/tools/shell/src/config_client.rs",
    "cells/tools/shell/src/executor.rs",
    "cells/tools/shell/src/shell_test.rs",
    "cells/tools/wasm/src/main.rs",
}

UNSAFE_RE = re.compile(r"\bunsafe\b(?!_)")


def strip_comments(text: str) -> str:
    out = []
    in_block = 0
    for line in text.splitlines():
        i = 0
        buf = []
        while i < len(line):
            if in_block:
                end = line.find("*/", i)
                if end == -1:
                    i = len(line)
                else:
                    in_block -= 1
                    i = end + 2
                continue
            ls = line.find("//", i)
            bs = line.find("/*", i)
            if bs != -1 and (ls == -1 or bs < ls):
                buf.append(line[i:bs])
                in_block += 1
                i = bs + 2
            elif ls != -1:
                buf.append(line[i:ls])
                i = len(line)
            else:
                buf.append(line[i:])
                i = len(line)
        out.append("".join(buf))
    return "\n".join(out)


def main() -> int:
    files = subprocess.run(
        ["git", "ls-files", "cells/**/*.rs", "cells/*.rs"],
        capture_output=True, text=True, check=True,
    ).stdout.split()

    offenders, stale = [], []
    seen_unsafe = set()
    for path in files:
        with open(path, encoding="utf-8", errors="replace") as f:
            code = strip_comments(f.read())
        if UNSAFE_RE.search(code):
            seen_unsafe.add(path)
            if path not in ALLOWLIST:
                offenders.append(path)

    stale = sorted(ALLOWLIST - seen_unsafe)

    if stale:
        print("NOTE: allowlisted files no longer contain unsafe — tighten the ratchet:")
        for p in stale:
            print(f"  {p}")

    if offenders:
        print("FAIL: unsafe code outside the cells/ allowlist (Law 4 — Cells are")
        print("      #![forbid(unsafe_code)]). Remove the unsafe, or if this is a")
        print("      Driver-Cell MMIO/DMA necessity, add the file to ALLOWLIST in")
        print(f"      {__file__} with a justification:")
        for p in sorted(offenders):
            print(f"  {p}")
        return 1

    print(f"PASS: {len(files)} files scanned, unsafe confined to "
          f"{len(seen_unsafe)}/{len(ALLOWLIST)} allowlisted files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
