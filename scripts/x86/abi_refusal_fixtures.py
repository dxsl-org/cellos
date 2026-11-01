#!/usr/bin/env python3
"""Phase 06 signed CellManifest refusal fixtures and audit-ring evidence.

Private x86 test tooling (plan .agents/261030-1945-ginux-lessons-hardening,
phase 06). Nothing here ships: the fixtures live only in the image built by
`scripts/x86/make-abi-refusal-iso.sh` under its own cargo target directory.

Fixture contract (docs/abi-boundary-records.md, "CellManifest"):

* Every fixture is the same unsigned base cell ELF with exactly one
  `__ViCell_manifest` section added BEFORE signing. The normal signing route
  (`scripts/lib-sign-cells.sh` -> `cellos-sign --sign`, dev key, F1/F5 check)
  then signs it, so the signature covers the malformed record. Parser evidence
  is never produced by patching after signing.
* One fixture is deliberately damaged AFTER signing (`corrupt_signature`): it
  is signature-failure evidence only and carries a valid current record.
* Expected outcomes come from libs/api/src/abi/manifest_parse.rs and
  kernel/src/loader/{manifest_section,governed_spawn}.rs; the runner checks the
  kernel's own audit records, never a re-implementation of the parser.

The record builders mirror the frozen layout in libs/api/src/abi/manifest.rs.
They are byte producers for fixtures, not a second decoder.
"""

from __future__ import annotations

import json
import struct
import subprocess
from dataclasses import dataclass
from pathlib import Path

MANIFEST_MAGIC = 0x5649_4345  # libs/api/src/abi/manifest_flags.rs MANIFEST_MAGIC
MANIFEST_SECTION = "__ViCell_manifest"
SIG_SECTION = "__ViCell_sig"
TIER_LEGACY = 0xFF
FIRST_UNDEFINED_FLAG = 1 << 12  # MANIFEST_FLAGS_MASK covers bits 0..=11

ADMIT, FORMAT, SIGNATURE = "admit", "format-refused", "signature-refused"

# kernel/src/audit.rs AuditEvent discriminants used by the governed admission path.
EV_CELL_SPAWN = 7
EV_SPAWN_DENIED = 10
EV_MEASURE = 15
EV_SIG_VERIFIED = 21
EV_SIG_FAILED = 22
EVENT_NAMES = {
    1: "IpcSend", 2: "IpcRecv", 3: "FileOpen", 4: "FileWrite", 5: "NetTx", 6: "NetRx",
    7: "CellSpawn", 8: "CellFault", 9: "CellExit", 10: "CellSpawnDenied",
    11: "RtDeadlineMiss", 12: "RtCpuOverrun", 13: "CellHung", 14: "SyscallDenied",
    15: "CellMeasure", 16: "PolicyLoaded", 17: "PolicyInvalid", 18: "PolicyAbsent",
    19: "CapNarrowedByPolicy", 20: "CapRevoked", 21: "CellSignatureVerified",
    22: "CellSignatureFailed", 23: "PrivilegedCapGranted", 24: "PolicyMaintenanceBypass",
    25: "ThreadCapReached", 26: "PolicyNoEntryStripped", 27: "NetRightsGranted",
}
# Records emitted by one governed admission attempt (kernel/src/loader/governed_spawn.rs,
# kernel/src/task/launch.rs, kernel/src/measurement_log.rs). Everything else is noise.
ADMISSION_EVENTS = {EV_CELL_SPAWN, EV_SPAWN_DENIED, EV_MEASURE, 19, EV_SIG_VERIFIED,
                    EV_SIG_FAILED, 23, 24, 26, 27}
TERMINAL_EVENTS = {EV_CELL_SPAWN, EV_SPAWN_DENIED, EV_SIG_FAILED}

AUDIT_BUF_SIZE = 256 * 1024  # kernel/src/audit.rs BUF_SIZE
AUDIT_RING_BYTES = AUDIT_BUF_SIZE + 3 * 8  # buf + head/tail/dropped (AtomicUsize)


def v1_record(flags: int = 0, pad: tuple[int, int] = (0, 0), version: int = 1) -> bytes:
    """The 8-byte v1 layout: magic u32, version u8, flags u8, two padding bytes."""
    return struct.pack("<IBBBB", MANIFEST_MAGIC, version, flags, *pad)


def v2_record(tier: int = TIER_LEGACY, flags: int = 0, cap_args_off: int = 0,
              reserved: int = 0, version: int = 2) -> bytes:
    """The 16-byte v2 layout: magic, version, tier, flags u16, cap_args_off, reserved."""
    return struct.pack("<IBBHII", MANIFEST_MAGIC, version, tier, flags, cap_args_off, reserved)


@dataclass(frozen=True)
class Fixture:
    name: str
    slot: str
    record: bytes
    expect: str
    why: str

    @property
    def path(self) -> str:
        return "/bin/" + self.slot


# Launch order matters: refusals sit between admitted controls so the evidence
# window is anchored on both sides, and the last fixture is an admitted control.
#
# `slot` is the install path. The syscall launch-edge gate runs before admission
# and authorizes init only for exact reviewed targets
# (kernel/src/loader/launch_profile/profiles.rs init_profile ->
# targets.rs reviewed_user_target_ceiling), so each fixture occupies a distinct
# reviewed LEGACY-ceiling slot of this private image. The slot name says nothing
# about the bytes: every fixture is the same echo ELF plus its own manifest.
FIXTURES: tuple[Fixture, ...] = (
    Fixture("abifx-v2", "echo", v2_record(), ADMIT, "current v2 record, TIER_LEGACY, no flags"),
    Fixture("abifx-badver", "cat", v2_record(version=3), FORMAT, "unknown version 3 (16 bytes)"),
    Fixture("abifx-v1", "ls", v1_record(), ADMIT, "supported older v1 record (8 bytes)"),
    Fixture("abifx-badflag", "ps", v2_record(flags=FIRST_UNDEFINED_FLAG), FORMAT,
            "unknown flag bit 12 outside MANIFEST_FLAGS_MASK"),
    Fixture("abifx-v1pad", "free", v1_record(pad=(1, 0)), FORMAT,
            "v1 reserved padding byte 6 non-zero"),
    Fixture("abifx-badrsv", "wx-test", v2_record(reserved=1), FORMAT, "v2 reserved u32 non-zero"),
    Fixture("abifx-capargs", "cfi-test", v2_record(cap_args_off=1), FORMAT,
            "v2 reserved cap_args_off non-zero"),
    Fixture("abifx-badtier", "futex-test", v2_record(tier=4), FORMAT,
            "protection class 4 (not 0..=3, not LEGACY)"),
    Fixture("abifx-v1len", "tls-test", v1_record() + bytes(8), FORMAT,
            "version 1 with a 16-byte record (length/version mismatch)"),
    Fixture("abifx-sigbad", "c-pthread", v2_record(), SIGNATURE,
            "current v2 record; signature byte flipped after signing"),
    Fixture("abifx-v2b", "pipe-peer", v2_record(), ADMIT, "current v2 record after the refusals"),
)


def build_unsigned(base: Path, out_dir: Path, objcopy: str) -> list[Path]:
    """Write one unsigned fixture per entry: base ELF + exactly one manifest section."""
    listing = subprocess.run(["readelf", "-S", "-W", str(base)], check=True,
                             capture_output=True, text=True).stdout
    if MANIFEST_SECTION in listing:
        raise SystemExit(f"base {base} already declares {MANIFEST_SECTION}; use a manifest-free cell")
    out_dir.mkdir(parents=True, exist_ok=True)
    built = []
    for fixture in FIXTURES:
        record = out_dir / (fixture.name + ".manifest")
        record.write_bytes(fixture.record)
        target = out_dir / fixture.name
        subprocess.run([
            objcopy, f"--remove-section={SIG_SECTION}",
            f"--add-section={MANIFEST_SECTION}={record}",
            f"--set-section-flags={MANIFEST_SECTION}=contents,readonly",
            str(base), str(target),
        ], check=True)
        record.unlink()
        built.append(target)
    return built


def section_range(signer, elf: bytes, name: str) -> tuple[int, int]:
    bits, shoff, shentsize, shnum, shstrndx = signer._section_table(elf)
    found = signer._find_section_range(elf, name, shoff, shentsize, shnum, shstrndx, bits)
    if found is None:
        raise SystemExit(f"missing unique {name} section")
    return found


def corrupt_signature(signer, path: Path) -> None:
    """Flip one signature bit after signing (signature-failure evidence only)."""
    elf = bytearray(path.read_bytes())
    offset, size = section_range(signer, bytes(elf), SIG_SECTION)
    if size != 64:
        raise SystemExit(f"{path}: {SIG_SECTION} is {size} bytes")
    elf[offset] ^= 0x01
    path.write_bytes(bytes(elf))


def autoload_toml() -> str:
    lines = ["# Phase 06 private ABI refusal image: fixtures only, launched in order.",
             "version = 1", ""]
    for fixture in FIXTURES:
        lines += ["[[cells]]", f'name = "{fixture.name}"', f'path = "{fixture.path}"',
                  'restart = "never"', ""]
    return "\n".join(lines)


# ── Audit ring (kernel/src/audit.rs) ─────────────────────────────────────────

def _records(buf: bytes, head: int, tail: int) -> list[tuple[int, bytes]] | None:
    """Parse whole records tail..head; None unless they end exactly at head."""
    if head < tail or head - tail > AUDIT_BUF_SIZE:
        return None
    mask = AUDIT_BUF_SIZE - 1
    out, at = [], tail
    while at != head:
        if head - at < 10:
            return None
        event = buf[(at + 8) & mask]
        length = buf[(at + 9) & mask]
        if event not in EVENT_NAMES or head - at < 10 + length:
            return None
        payload = bytes(buf[(at + 10 + i) & mask] for i in range(length))
        out.append((event, payload))
        at += 10 + length
    return out


def parse_ring(raw: bytes) -> tuple[list[tuple[int, bytes]], dict]:
    """Decode the dumped `AuditRing` static. Rust may order its fields either way,
    so both layouts and all role assignments are tried; exactly one distinct
    interpretation must parse (equal words, e.g. tail == dropped == 0, coincide)."""
    if len(raw) != AUDIT_RING_BYTES:
        raise ValueError(f"ring dump is {len(raw)} bytes, expected {AUDIT_RING_BYTES}")
    candidates = {}
    for buf_at, words_at in ((0, AUDIT_BUF_SIZE), (24, 0)):
        buf = raw[buf_at:buf_at + AUDIT_BUF_SIZE]
        words = struct.unpack_from("<3Q", raw, words_at)
        for h, t, d in ((0, 1, 2), (0, 2, 1), (1, 0, 2), (1, 2, 0), (2, 0, 1), (2, 1, 0)):
            records = _records(buf, words[h], words[t])
            if records:
                layout = {"buf_offset": buf_at, "head": words[h], "tail": words[t],
                          "dropped": words[d]}
                candidates[tuple(layout.values())] = (records, layout)
    if len(candidates) != 1:
        raise ValueError(f"audit ring layout ambiguous or unparsable ({len(candidates)} candidates)")
    return next(iter(candidates.values()))


def u32x2(payload: bytes) -> tuple[int, int] | None:
    return struct.unpack("<II", payload) if len(payload) == 8 else None


def attempts(records: list[tuple[int, bytes]]) -> list[list[tuple[int, bytes]]]:
    """Group admission records into attempts, each closed by a terminal record."""
    groups, current = [], []
    for event, payload in records:
        if event not in ADMISSION_EVENTS:
            continue
        current.append((event, payload))
        if event in TERMINAL_EVENTS:
            groups.append(current)
            current = []
    if current:
        groups.append(current)
    return groups


def classify(attempt: list[tuple[int, bytes]]) -> tuple[str, int | None]:
    """Map one attempt to an outcome using only the kernel's own records."""
    events = [event for event, _ in attempt]
    last_event, last_payload = attempt[-1]
    if events == [EV_SPAWN_DENIED] and u32x2(last_payload) == (0, 1):
        # The pre-signature manifest branch (governed_spawn.rs) is the only (0, 1)
        # denial that is not preceded by CellSignatureVerified for a signed ELF.
        return FORMAT, None
    if events == [EV_SIG_FAILED] and u32x2(last_payload) == (0, 0):
        return SIGNATURE, None
    if (last_event == EV_CELL_SPAWN and events[0] == EV_SIG_VERIFIED
            and EV_SPAWN_DENIED not in events and EV_SIG_FAILED not in events):
        tid = u32x2(last_payload)[0]
        measured = [u32x2(p)[0] for e, p in attempt if e == EV_MEASURE]
        if measured in ([], [tid]):
            return ADMIT, tid
    return "unexpected:" + ",".join(EVENT_NAMES.get(e, str(e)) for e in events), None


def evaluate(records: list[tuple[int, bytes]]) -> tuple[bool, list[str], dict]:
    """Match the trailing attempts against FIXTURES in launch order.

    Init tries the VFS ELF route, then the path route, after a refusal
    (cells/tools/init/src/service_table.rs spawn_path), so each refused fixture
    yields `r` identical attempts with r in {1, 2}; r is read from the sigbad
    fixture (its kind is unique) and must hold for every refusal.
    """
    groups = [(classify(a), a) for a in attempts(records)]
    lines: list[str] = []
    if not groups or groups[-1][0][0] != ADMIT:
        return False, ["ABI-FX: FAIL last admission attempt is not the tail control"], {}
    sig_run = 0
    for (kind, _), _ in reversed(groups[:-1]):
        if kind != SIGNATURE:
            break
        sig_run += 1
    if sig_run not in (1, 2):
        return False, [f"ABI-FX: FAIL signature-refusal run length {sig_run}"], {}
    needed = sum(1 if f.expect == ADMIT else sig_run for f in FIXTURES)
    if len(groups) < needed:
        return False, [f"ABI-FX: FAIL only {len(groups)} attempts, need {needed}"], {}
    window = groups[-needed:]
    exits = {u32x2(p)[0]: u32x2(p)[1] for e, p in records if e == 9 and u32x2(p)}
    ok, at, tids = True, 0, {}
    for fixture in FIXTURES:
        count = 1 if fixture.expect == ADMIT else sig_run
        observed = window[at:at + count]
        at += count
        kinds = {kind for (kind, _), _ in observed}
        verdict = kinds == {fixture.expect}
        detail = "; ".join(
            "+".join(f"{EVENT_NAMES[e]}{u32x2(p) if u32x2(p) else ''}" for e, p in attempt)
            for _, attempt in observed)
        ran = ""
        if fixture.expect == ADMIT and verdict:
            tid = observed[0][0][1]
            tids[fixture.name] = tid
            # A published control must have run: its own CellExit(tid, code) record.
            verdict = tid in exits
            ran = f" exit(tid={tid})={exits.get(tid, '<none>')}"
        ok &= verdict
        lines.append(f"ABI-FX {fixture.name} {fixture.path}: {'PASS' if verdict else 'FAIL'} "
                     f"expect={fixture.expect} observed={sorted(kinds)} "
                     f"attempts={count} records=[{detail}]{ran}")
    return ok, lines, {"routes_per_refusal": sig_run, "admitted_tids": tids}


def write_manifest(out: Path, entries: dict) -> None:
    out.write_text(json.dumps(entries, indent=2, sort_keys=True) + "\n")
