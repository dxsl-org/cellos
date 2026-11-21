#!/usr/bin/env python3
"""Fail if a staged x86_64 cell contains an instruction that UMIP refuses at CPL 3.

The kernel sets CR4.UMIP on every CPU that has it (hal/arch/x86/src/x86_64.rs).
With it set, SGDT, SIDT, SLDT, SMSW and STR raise #GP in user mode. The trap
handler then terminates the cell (`vi_terminate_on_user_trap_fault`,
hal/arch/x86/src/x86_64/idt/dispatch.rs). Isolation holds, but the cell dies the
first time it reaches the instruction, which may be long after boot. This check
finds the instruction at build time instead.

Usage: check-x86-umip-mnemonics.py PATH...

Each PATH is either an x86-64 ELF or a FAT kernel_fs.img. Every x86-64 ELF
inside an image is checked; other image files (configs, policy) are skipped.
The disassembler is `$OBJDUMP` (default `objdump`, which must understand
x86-64).

Before it reads any input, the check disassembles a fixed byte string that holds
each refused instruction between harmless ones. It must report exactly those
five. If it does not, this objdump's output is not understood and nothing below
could fail, so the run stops with exit 2.

Exit 0: no refused instruction. 1: at least one found, or an input was no x86-64
ELF / FAT image. 2: the self-check failed, or no ELF was given.
"""

from __future__ import annotations

import importlib.util
import os
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OBJDUMP = os.environ.get("OBJDUMP", "objdump")
EM_X86_64 = 62

REFUSED = re.compile(r"(sgdt|sidt|sldt|smsw|str)[wlq]?")
# Tokens objdump prints before the mnemonic (AT&T syntax).
PREFIXES = frozenset({
    "lock", "rep", "repz", "repe", "repnz", "repne", "data16", "data32", "addr16",
    "addr32", "cs", "ds", "es", "fs", "gs", "ss", "notrack", "bnd", "xacquire",
    "xrelease",
})
INSN = re.compile(r"^\s*([0-9a-f]+):\t(.*)$")
SYMBOL = re.compile(r"^[0-9a-f]+ <(.+)>:$")

# mov %rsp,%rbp; sgdt (%rax); nop; sidt (%rax); sldt %eax; ret; smsw %eax; str %eax; ret
CONTROL_BYTES = bytes.fromhex("4889e5 0f0100 90 0f0108 0f00c0 c3 0f01e0 0f00c8 c3".replace(" ", ""))
CONTROL_EXPECTED = ["sgdt", "sidt", "sldt", "smsw", "str"]


def mnemonic(text: str) -> str:
    for token in text.split():
        if token in PREFIXES or token.startswith(("rex", "{")):
            continue
        return token
    return ""


def refused_in(listing: str) -> list[tuple[str, str, str]]:
    """(symbol, address, instruction) of every refused instruction in `objdump -d` output."""
    hits = []
    symbol = "?"
    for line in listing.splitlines():
        if found := SYMBOL.match(line):
            symbol = found.group(1)
            continue
        found = INSN.match(line)
        if found and REFUSED.fullmatch(mnemonic(found.group(2))):
            hits.append((symbol, found.group(1), found.group(2).strip()))
    return hits


def objdump(args: list[str]) -> str:
    result = subprocess.run([OBJDUMP, "--no-show-raw-insn", *args],
                            capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"{OBJDUMP} {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout


def self_check() -> str | None:
    """None when the control bytes yield exactly the five refused instructions."""
    with tempfile.TemporaryDirectory(prefix="umip-control-") as tmp:
        blob = Path(tmp) / "control.bin"
        blob.write_bytes(CONTROL_BYTES)
        try:
            listing = objdump(["-D", "-b", "binary", "-m", "i386:x86-64", str(blob)])
        except (OSError, RuntimeError) as exc:
            return str(exc)
    found = [REFUSED.fullmatch(mnemonic(insn)).group(1) for _, _, insn in refused_in(listing)]
    if found != CONTROL_EXPECTED:
        return f"control bytes gave {found}, expected {CONTROL_EXPECTED}"
    return None


def is_x86_64_elf(data: bytes) -> bool:
    return (data[:4] == b"\x7fELF" and len(data) >= 20
            and int.from_bytes(data[18:20], "little") == EM_X86_64)


def image_elves(image: Path) -> tuple[dict[str, bytes], list[str]]:
    """The x86-64 ELFs in a FAT image by path, and the paths of foreign ELFs."""
    spec = importlib.util.spec_from_file_location(
        "cellos_boot_config_reader", REPO / "scripts" / "generate-boot-config.py")
    reader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(reader)
    elves, foreign = {}, []
    for path, data in sorted(reader.image_files(image).items()):
        data = bytes(data)
        if is_x86_64_elf(data):
            elves[f"{image}:{path}"] = data
        elif data[:4] == b"\x7fELF":
            foreign.append(f"{image}:{path}")
    return elves, foreign


def main(argv: list[str]) -> int:
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0 if argv else 2
    problem = self_check()
    if problem:
        print(f"FAIL: UMIP self-check: {problem}", file=sys.stderr)
        return 2

    elves: dict[str, bytes] = {}
    errors: list[str] = []
    for arg in argv:
        path = Path(arg)
        try:
            head = path.read_bytes()[:20]
        except OSError as exc:
            errors.append(f"{arg}: {exc.strerror or exc}")
            continue
        if is_x86_64_elf(head):
            elves[arg] = path.read_bytes()
            continue
        if head[:4] == b"\x7fELF":
            errors.append(f"{arg}: an ELF, but not x86-64")
            continue
        try:
            found, foreign = image_elves(path)
        except (ValueError, IndexError, struct.error) as exc:
            errors.append(f"{arg}: neither an x86-64 ELF nor a FAT image ({exc})")
            continue
        elves.update(found)
        errors.extend(f"{name}: an ELF, but not x86-64" for name in foreign)
    if not elves and not errors:
        print("FAIL: no x86-64 ELF among the inputs; nothing was checked", file=sys.stderr)
        return 2

    hits = 0
    with tempfile.TemporaryDirectory(prefix="umip-") as tmp:
        for index, (name, data) in enumerate(sorted(elves.items())):
            elf = Path(tmp) / str(index)
            elf.write_bytes(data)
            try:
                found = refused_in(objdump(["-d", str(elf)]))
            except (OSError, RuntimeError) as exc:
                errors.append(f"{name}: {exc}")
                continue
            for symbol, address, insn in found:
                print(f"FAIL: {name}: {insn} at {address} in <{symbol}> "
                      "(#GP under UMIP, kills the cell)", file=sys.stderr)
            hits += len(found)
    for error in errors:
        print(f"FAIL: {error}", file=sys.stderr)
    if hits or errors:
        return 1
    print(f"OK: no SGDT/SIDT/SLDT/SMSW/STR in {len(elves)} x86-64 ELF(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
