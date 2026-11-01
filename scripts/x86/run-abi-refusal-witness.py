#!/usr/bin/env python3
"""Boot the Phase 06 private ABI refusal ISO and judge the kernel's own records.

Build first: `bash scripts/x86/make-abi-refusal-iso.sh`.

The image's init launches every fixture of scripts/x86/abi_refusal_fixtures.py
through its normal configured-cell path (ELF route, then path route), both of
which end in kernel `loader::spawn_gated` -> `governed_spawn::prepare_gated`.
The canonical kernel prints nothing that separates a malformed manifest from a
bad signature (both are PermissionDenied to the caller), so the evidence is the
kernel audit ring (kernel/src/audit.rs), read out of guest memory through the
QEMU monitor once init reports every fixture. Nothing in the guest is changed
to produce it.

Exit 0 only when every fixture matched its expected outcome; 1 otherwise; 2
when a prerequisite is missing (reported, never a pass).
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import abi_refusal_fixtures as fx  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
DEFAULT_DIR = REPO / "target" / "abi-refusal-06"
DONE_MARKER = "Init: supervising services"


def ring_address(kernel: Path) -> int:
    symbols = subprocess.run(["nm", str(kernel)], check=True, capture_output=True,
                             text=True).stdout
    matches = [line.split()[0] for line in symbols.splitlines()
               if line.endswith("5audit4RING") or "5audit4RING17h" in line]
    if len(matches) != 1:
        raise SystemExit(f"FAIL: expected one audit::RING symbol in {kernel}, found {matches}")
    return int(matches[0], 16)


def qmp(sock_path: Path, commands: list[dict], timeout: float = 30.0) -> list[dict]:
    """Run QMP commands in order on one session; return each command's reply."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as monitor:
        monitor.settimeout(timeout)
        monitor.connect(str(sock_path))
        stream = monitor.makefile("rwb")

        def reply() -> dict:
            while True:
                line = stream.readline()
                if not line:
                    return {"error": "QMP connection closed"}
                message = json.loads(line)
                if "return" in message or "error" in message:
                    return message

        stream.readline()  # greeting
        replies = []
        for command in [{"execute": "qmp_capabilities"}] + commands:
            stream.write(json.dumps(command).encode() + b"\n")
            stream.flush()
            replies.append(reply())
        return replies[1:]


def fixture_lines(serial: str) -> dict[str, str]:
    seen = {}
    for fixture in fx.FIXTURES:
        for outcome in ("launched.", "spawn/registration failed."):
            line = f"Init: {fixture.name} {outcome}"
            if line in serial:
                seen[fixture.name] = line
    return seen


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dir", type=Path, default=DEFAULT_DIR,
                        help="CARGO_TARGET_DIR used by make-abi-refusal-iso.sh")
    parser.add_argument("--timeout", type=float, default=240.0)
    parser.add_argument("--out", type=Path, help="evidence directory (default: a temp dir)")
    args = parser.parse_args()

    iso = args.dir / "vicell-x86-abi-refusal.iso"
    kernel = args.dir / "x86_64-unknown-none" / "release" / "cellos-kernel"
    manifest = args.dir / "abifx" / "fixtures.json"
    qemu = shutil.which("qemu-system-x86_64")
    missing = [str(p) for p in (iso, kernel, manifest) if not p.is_file()]
    if missing or not qemu:
        print(f"ABI-FX: UNAVAILABLE missing={missing} qemu={qemu}", file=sys.stderr)
        return 2
    fixtures = json.loads(manifest.read_text())
    ring_va = ring_address(kernel)
    out = args.out or Path(tempfile.mkdtemp(prefix="abi-refusal-"))
    out.mkdir(parents=True, exist_ok=True)
    serial_log, sock, dump = out / "serial.log", out / "monitor.sock", out / "audit-ring.bin"
    disk = out / "sata.img"
    with open(disk, "wb") as blank:
        blank.truncate(16 * 1024 * 1024)
    for stale in (serial_log, sock, dump):
        stale.unlink(missing_ok=True)

    command = [
        qemu, "-machine", "q35", "-cpu", "qemu64,+pdpe1gb", "-m", "256M",
        "-display", "none", "-cdrom", str(iso), "-boot", "d", "-no-reboot",
        "-drive", f"file={disk},format=raw,if=none,id=sata0", "-device", "ide-hd,drive=sata0",
        "-qmp", f"unix:{sock},server,nowait", "-serial", f"file:{serial_log}",
    ]
    print("ABI-FX: qemu " + " ".join(command[1:]))
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + args.timeout
        serial = ""
        while time.monotonic() < deadline and child.poll() is None:
            serial = serial_log.read_text(errors="replace") if serial_log.exists() else ""
            if len(fixture_lines(serial)) == len(fx.FIXTURES) and DONE_MARKER in serial:
                break
            time.sleep(1.0)
        if child.poll() is not None:
            print(f"ABI-FX: FAIL qemu exited early: {child.stderr.read().decode(errors='replace')}")
            return 1
        time.sleep(2.0)  # let the last control finish its own exit records
        serial = serial_log.read_text(errors="replace")
        seen = fixture_lines(serial)
        # Pause the guest, then copy the kernel's `audit::RING` static through
        # the current vCPU's page tables (the kernel half is mapped in every root).
        replies = qmp(sock, [
            {"execute": "stop"},
            {"execute": "memsave", "arguments": {
                # QMP `val` is an int64: pass the higher-half address in two's complement.
                "val": ring_va - (1 << 64) if ring_va >= 1 << 63 else ring_va,
                "size": fx.AUDIT_RING_BYTES, "filename": str(dump)}},
        ])
        if any("error" in r for r in replies) or not dump.is_file() \
                or dump.stat().st_size != fx.AUDIT_RING_BYTES:
            print(f"ABI-FX: FAIL memsave produced no ring dump; QMP replies: {replies}")
            return 1
    finally:
        try:
            qmp(sock, [{"execute": "quit"}], timeout=5.0)
        except OSError:
            pass
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()

    print(f"ABI-FX: evidence dir {out}")
    for line in serial.splitlines():
        if "[signing] cell trust posture" in line or "[policy]" in line:
            print("ABI-FX serial: " + line.strip())
    for fixture in fx.FIXTURES:
        entry = fixtures[fixture.name]
        print(f"ABI-FX fixture {fixture.name}: manifest={entry['manifest_hex']} "
              f"host-signature={'valid' if entry['host_signature_valid'] else 'INVALID'} "
              f"init=\"{seen.get(fixture.name, '<missing>')}\"")
    if len(seen) != len(fx.FIXTURES):
        print(f"ABI-FX: FAIL init reported {len(seen)}/{len(fx.FIXTURES)} fixtures before "
              f"{args.timeout:.0f}s")
        return 1
    edge_denials = [line.strip() for line in serial.splitlines()
                    if "DENY launch edge" in line
                    and any(f"target={f.path} " in line for f in fx.FIXTURES)]
    if edge_denials:
        print("ABI-FX: FAIL fixtures stopped at the launch-edge gate, before admission:")
        for line in edge_denials:
            print("ABI-FX serial: " + line)
        return 1
    records, layout = fx.parse_ring(dump.read_bytes())
    print(f"ABI-FX: audit ring @0x{ring_va:x} records={len(records)} layout={layout}")
    if layout["dropped"]:
        print("ABI-FX: FAIL audit ring evicted records; evidence window incomplete")
        return 1
    ok, lines, summary = fx.evaluate(records)
    for line in lines:
        print(line)
    # A refused fixture must be reported as failed by init. An admitted echo
    # control may exit before init arms its exit watch, which init also reports
    # as "spawn/registration failed."; its publication and run are judged from
    # the kernel records above (CellSpawn + its own CellExit), not this line.
    init_ok = all(seen[f.name].endswith("spawn/registration failed.")
                  for f in fx.FIXTURES if f.expect != fx.ADMIT)
    print(f"ABI-FX: init refusal lines {'match' if init_ok else 'MISMATCH'}; summary={summary}")
    (out / "evidence.json").write_text(json.dumps(
        {"pass": ok and init_ok, "lines": lines, "summary": summary, "layout": layout,
         "ring_va": hex(ring_va)}, indent=2) + "\n")
    print(f"ABI-FX: {'PASS' if ok and init_ok else 'FAIL'}")
    return 0 if ok and init_ok else 1


if __name__ == "__main__":
    sys.exit(main())
