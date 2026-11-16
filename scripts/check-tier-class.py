#!/usr/bin/env python3
"""Report the allowlist-class × manifest-class join behind the tier-class gate.

The gate itself lives in `cellos-sign` (`scripts/cellos_sign/tier_class.py`) and
runs in `--check` and before every `--sign`; this tool only prints the join for
evidence and review:

    python3 scripts/check-tier-class.py            # crates with c-ffi / legacy-raw-ptr
    python3 scripts/check-tier-class.py --all      # every allowlisted crate
    python3 scripts/check-tier-class.py --negative-control

`--negative-control` builds throwaway git repositories outside this one, each
with one cell crate that holds `c-ffi` allowlist entries, and drives them
through the real `cellos-sign --sign` path with F5 reported verified and the
signing call replaced by a recorder. The target is a minimal ELF named after
the crate's binary, carrying the manifest the kernel would read. Three runs:
  * Tier-1 (tier-less) source, LEGACY binary: refused by the source gate;
  * FFI source, FFI binary: admitted, one target signed;
  * FFI source, binary without a manifest (what a `#[cfg]`-gated or never
    expanded `declare_manifest!` builds): refused by the binary gate.
It exits 0 only if all three behave so, nothing refused reaches the signer,
and each repository leaves an untracked scratch file with `unsafe` in the
crate that only a filesystem walk would see — so the admitted run proves the
`git ls-files` path CI uses is the one exercised. Nothing is written under
`cells/` of this repository.

Exit 0 when the gate passes (or the control behaves as designed), 1 otherwise.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import subprocess
import sys
import tempfile
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

from cellos_sign import allowlist, cli, elf_manifest, policy, tcb, toolchain  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
CONTROL_CRATE = "tier-class-negative-control"

_CONTROL_MAIN = """\
#![no_std]
#![no_main]

abi::declare_manifest!(block_io = false, network = false, spawn = false{tier});

extern "C" {{
    fn c_entry() -> i32;
}}

fn call() -> i32 {{
    unsafe {{ c_entry() }}
}}
"""

_CONTROL_ALLOWLIST = """\
version = 1

[[file]]
path = "cells/negative-control/src/main.rs"
class = "c-ffi"
reason = "negative control: calls a C function"
approver = "negative-control"
date = "{today}"

[[crate]]
name = "{crate}"
class = "c-ffi"
reason = "negative control: owns the C call"
approver = "negative-control"
date = "{today}"
"""


def write_control_tree(root: Path, tier: str, today: str, manifest: bytes | None) -> Path:
    """One c-ffi cell crate in a fresh git repository; returns its binary.

    `tier` is the extra `declare_manifest!` argument and `manifest` the record
    in the binary (None: no `__ViCell_manifest` section).
    """

    def put(rel: str, text: str) -> None:
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    put("cells/negative-control/Cargo.toml",
        f'[package]\nname = "{CONTROL_CRATE}"\nversion = "0.1.0"\n')
    put("cells/negative-control/src/main.rs", _CONTROL_MAIN.format(tier=tier))
    put(allowlist.DEFAULT_PATH, _CONTROL_ALLOWLIST.format(today=today, crate=CONTROL_CRATE))
    # The closure ratchet sees the crate as a Tier-1 root only in the Tier-1
    # variant; give each variant the closure and baseline it would generate so
    # the only difference between the runs is the manifest class.
    tier1 = not tier
    roots = f'"{CONTROL_CRATE}"' if tier1 else ""
    put(tcb.CLOSURE_PATH,
        f"version = 1\ntargets = []\ntier1_roots = [{roots}]\n"
        "package = []\nthirdparty = []\nunresolved = []\n")
    put(tcb.BASELINE_PATH,
        f"version = 1\nsite = []\n[total]\nunsafe = {1 if tier1 else 0}\nattrs = 0\n")
    for command in (["init", "-q"], ["add", "-A"]):
        subprocess.run(["git", "-C", str(root), *command], check=True, capture_output=True)
    # Untracked after `git add`: only a filesystem walk would scan it.
    put("cells/negative-control/src/scratch.rs", "fn scratch() { unsafe {} }\n")
    binary = root / "build" / CONTROL_CRATE
    binary.parent.mkdir()
    binary.write_bytes(elf_manifest.minimal_elf(manifest))
    return binary


def run_sign(root: Path, target: Path) -> tuple[int, list, str]:
    """`cellos-sign --sign` on `root` with F5 verified and signing recorded."""
    signed: list = []
    out = io.StringIO()
    verified = toolchain.ToolchainStatus(ok=True, skipped=False, detail="negative control")
    with mock.patch.object(cli.toolchain, "check", return_value=verified), \
         mock.patch.object(cli.signing, "sign_and_verify",
                           side_effect=lambda *a, **k: signed.append(a)), \
         contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
        code = cli.main(["--repo", str(root), "--sign", str(target)])
    return code, signed, out.getvalue()


def negative_control() -> int:
    import datetime as _dt

    today = _dt.date.today().isoformat()
    ffi_tier = ", tier = abi::manifest::PROTECTION_CLASS_FFI"
    legacy = elf_manifest.manifest_record(0xFF)
    ffi = elf_manifest.manifest_record(2)
    cases = (
        ("Tier-1 source, LEGACY binary", "", legacy, False,
         (f"[tier-class] {CONTROL_CRATE}", "allowlist class `c-ffi`")),
        ("FFI source, FFI binary", ffi_tier, ffi, True, ()),
        ("FFI source, binary without a manifest", ffi_tier, None, False,
         ("FAIL: tier class (binary)", f"binary of `{CONTROL_CRATE}` carries manifest ABSENT")),
    )
    failures = []
    with tempfile.TemporaryDirectory(prefix="tier-class-nc-") as tmp:
        for index, (label, tier, manifest, admit, expected) in enumerate(cases):
            root = Path(tmp) / str(index)
            target = write_control_tree(root, tier, today, manifest)
            code, signed, output = run_sign(root, target)
            ok = (code == cli.EXIT_OK and len(signed) == 1) if admit else (
                code == cli.EXIT_POLICY and not signed and all(e in output for e in expected))
            if not ok:
                failures.append(f"  {label}: exit {code}, signed {len(signed)}\n{output}")
    if not failures:
        print("tier-class negative control: OK (c-ffi entry + Tier-1 source refused; the same "
              "crate with PROTECTION_CLASS_FFI admitted through git ls-files; an FFI source "
              "whose binary carries no manifest refused by the binary gate; nothing refused "
              "was signed)")
        return 0
    print("tier-class negative control: FAIL", file=sys.stderr)
    print("\n".join(failures), file=sys.stderr)
    return 1


def report(show_all: bool) -> int:
    allow = allowlist.load(REPO)
    result = policy.check(REPO, allow)
    outcome = result.tier
    rows = sorted(outcome.records.values(), key=lambda r: (r.tier, r.crate))
    rows = [r for r in rows if r.classes and (show_all or r.triggers)]
    print(f"{'crate':28} {'allowlist classes':34} {'manifest':18} tier  verdict")
    failing = {crate for crate, _, _ in outcome.mismatches}
    for r in rows:
        if not r.triggers:
            verdict = "not gated"
        elif r.tier == 2:
            verdict = "ok"
        elif r.crate in failing:
            verdict = "REFUSED"
        else:
            verdict = "tier exception"
        print(f"{r.crate:28} {','.join(sorted(r.classes)):34} {r.manifest:18} {r.tier:<5} {verdict}")
    print()
    for x in sorted(allow.tier_exceptions.values(), key=lambda x: x.label):
        print(f"[[tier_exception]] {x.label}: approver {x.entry.approver}, "
              f"date {x.entry.date}, review_by {x.entry.review_by}")
    for label in outcome.unused_exceptions:
        print(f"NOTE: [[tier_exception]] {label} covers no mismatch")
    for crate, _, detail in outcome.mismatches:
        print(f"FAIL: {crate} — {detail}", file=sys.stderr)
    return 1 if outcome.mismatches else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--all", action="store_true", help="list every allowlisted crate")
    parser.add_argument("--negative-control", action="store_true",
                        help="prove the gate refuses a c-ffi crate with a Tier-1 manifest")
    args = parser.parse_args()
    return negative_control() if args.negative_control else report(args.all)


if __name__ == "__main__":
    sys.exit(main())
