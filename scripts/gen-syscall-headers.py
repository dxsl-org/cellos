#!/usr/bin/env python3
"""Generate the syscall-number blocks of the C and Zig headers from the table.

`libs/abi/abi/syscalls.toml` is the single source of syscall numbers
(architecture remediation Phase 08, Law-1 CP1). Each header keeps its own
structure; only the block between its BEGIN/END markers is generated, from the
header's own list of `(constant, ViSyscall name)` pairs below.

    python3 scripts/gen-syscall-headers.py          # rewrite the blocks
    python3 scripts/gen-syscall-headers.py --check  # CI: fail if a block drifted

Exit 0 when every block matches (or was rewritten), 1 on drift with --check.
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TABLE = ROOT / "libs/abi/abi/syscalls.toml"
BEGIN = "BEGIN generated syscall numbers (scripts/gen-syscall-headers.py)"
END = "END generated syscall numbers"

# (path, comment prefix, line template, [(constant, ViSyscall name)])
HEADERS = [
    (
        "libs/port-platform/include/cellos_syscall.h", "/*", "#define {const} {num}u",
        [("CELLOS_SYSCALL_LOG", "Log")],
    ),
    (
        "libs/zig-syscall/src/syscall.zig", "//", "pub const {const}: usize = {num};",
        [("SYS_EXIT", "Exit"), ("SYS_LOG", "Log"), ("SYS_WRITE", "Write"), ("SYS_READ", "Read"),
         ("SYS_OPEN", "Open"), ("SYS_CLOSE", "Close"), ("SYS_SEEK", "Seek"),
         ("SYS_GETTIME", "GetTime")],
    ),
    (
        "third_party/mlibc/sysdeps/vicell/include/vicell/syscall.h", "//", "#define {const} {num}",
        [("VI_SYS_EXIT", "Exit"), ("VI_SYS_SPAWN", "Spawn"), ("VI_SYS_SET_TLS_BASE", "SetTlsBase"),
         ("VI_SYS_FUTEX_WAIT", "FutexWait"), ("VI_SYS_FUTEX_WAKE", "FutexWake"),
         ("VI_SYS_YIELD", "Yield"), ("VI_SYS_GET_RANDOM", "GetRandom"), ("VI_SYS_MEM_INFO", "MemInfo"),
         ("VI_SYS_LOG", "Log"), ("VI_SYS_OPEN", "Open"), ("VI_SYS_READ", "Read"),
         ("VI_SYS_CLOSE", "Close"), ("VI_SYS_SEEK", "Seek"), ("VI_SYS_FILEOP", "FileOp"),
         ("VI_SYS_WRITE", "Write"), ("VI_SYS_GETTIME", "GetTime"), ("VI_SYS_FSTAT", "Fstat")],
    ),
]


def marker(prefix: str, text: str) -> str:
    return f"/* {text} */" if prefix == "/*" else f"{prefix} {text}"


def block(prefix: str, template: str, pairs: list[tuple[str, str]], numbers: dict[str, int]) -> list[str]:
    lines = [marker(prefix, BEGIN),
             marker(prefix, "Source: libs/abi/abi/syscalls.toml. Do not edit by hand.")]
    for const, name in pairs:
        if name not in numbers:
            sys.exit(f"gen-syscall-headers: {name} is not a ViSyscall row")
        lines.append(template.format(const=const, num=numbers[name]))
    lines.append(marker(prefix, END))
    return lines


def main() -> int:
    check = "--check" in sys.argv[1:]
    numbers = {r["name"]: r["number"] for r in tomllib.load(TABLE.open("rb"))["syscall"]}
    drift = []
    for rel, prefix, template, pairs in HEADERS:
        path = ROOT / rel
        lines = path.read_text().split("\n")
        begin = next((i for i, l in enumerate(lines) if BEGIN in l), None)
        end = next((i for i, l in enumerate(lines) if END in l), None)
        if begin is None or end is None or end < begin:
            sys.exit(f"gen-syscall-headers: {rel} has no BEGIN/END markers")
        new = lines[:begin] + block(prefix, template, pairs, numbers) + lines[end + 1:]
        if new != lines:
            drift.append(rel)
            if not check:
                path.write_text("\n".join(new))
    if check and drift:
        for rel in drift:
            print(f"FAIL {rel}: syscall numbers differ from libs/abi/abi/syscalls.toml; "
                  "run scripts/gen-syscall-headers.py")
        return 1
    print(f"syscall headers: {'OK' if check else 'written'} ({len(HEADERS)} headers)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
