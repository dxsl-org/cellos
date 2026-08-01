"""Command line for `cellos-sign`. See the package docstring for the threat model.

Exit codes: 0 ok · 1 policy violation (F1 or F5) · 2 usage/config error ·
3 signing refused or signature invalid.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import os
import sys
from pathlib import Path

from . import allowlist as allowlist_mod
from . import policy, signing, toolchain

EXIT_OK, EXIT_POLICY, EXIT_USAGE, EXIT_SIGNING = 0, 1, 2, 3


def _repo_root() -> Path:
    """The repo root, derived from this file's location (scripts/cellos_sign/)."""
    return Path(__file__).resolve().parent.parent.parent


def _report(result: policy.Result, status: toolchain.ToolchainStatus, quiet: bool) -> None:
    if result.stale_entries:
        print("NOTE: allowlist entries overdue for re-review (past `review_by`, or "
              "older than `max_age_days` when they set none):")
        for line in result.stale_entries:
            print(f"  {line}")
    for key in result.unused_file_entries:
        print(f"NOTE: [[file]] {key} no longer contains unsafe — tighten the allowlist")
    for key in result.unused_crate_entries:
        print(f"NOTE: [[crate]] {key} is no longer needed — tighten the allowlist")

    if status.skipped:
        print(f"SKIP: F5 — {status.detail}")
    elif not status.ok:
        print(f"FAIL: F5 — {status.detail}")
    elif not quiet:
        print(f"OK:   F5 — {status.detail}")

    if result.violations:
        print("FAIL: F1 — Cells must be #![forbid(unsafe_code)] (Spec 16 §6). Either")
        print("      remove the unsafe / add the attribute, or add a reviewed entry to")
        print("      scripts/unsafe-allowlist.toml with reason, approver and date:")
        for violation in sorted(result.violations, key=lambda v: (v.layer, v.path)):
            print(f"  {violation}")
    elif not quiet:
        print(f"OK:   F1 — {result.crates_scanned} crates and {result.files_scanned} files "
              f"scanned; unsafe confined to {len(result.unsafe_files)} allowlisted files")


def run_check(args: argparse.Namespace) -> int:
    """F1 + F5 with no signing. Pure source parsing: no build, no network."""
    repo = args.repo
    try:
        allow = allowlist_mod.load(repo, args.allowlist)
    except allowlist_mod.AllowlistError as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return EXIT_USAGE

    result = policy.check(repo, allow, today=_dt.date.today())
    status = toolchain.check(repo)
    _report(result, status, args.quiet)

    if not result.ok:
        return EXIT_POLICY
    if not status.ok:
        return EXIT_POLICY
    if status.skipped and args.strict:
        print("FAIL: --strict requires F5 to be verified, but it was skipped",
              file=sys.stderr)
        return EXIT_POLICY
    return EXIT_OK


def run_sign(args: argparse.Namespace) -> int:
    """Check, then sign. There is no path to the signing call that skips the check.

    Strictness is imposed here rather than left to the caller: the signature is
    defined as attesting that the pipeline enforced F1 *and* F5, so a host that
    cannot verify F5 must refuse to sign instead of printing `SKIP` and signing
    anyway. A build script cannot forget a flag it does not have to pass.
    """
    args.strict = True
    check_code = run_check(args)
    if check_code != EXIT_OK:
        sys.stdout.flush()  # keep the refusal below the findings it refers to
        print("REFUSED: not signing — the F1/F5 check above did not pass.", file=sys.stderr)
        return check_code
    try:
        signing.sign_and_verify(
            args.repo, [Path(p) for p in args.targets], args.objcopy, args.seed_hex
        )
    except signing.SigningRefused as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        return EXIT_SIGNING
    except (OSError, AssertionError) as exc:
        print(f"FAIL: signing failed — {exc}", file=sys.stderr)
        return EXIT_SIGNING
    key = "prod" if args.seed_hex else "dev"
    print(f"OK:   signed {len(args.targets)} cell(s) with the {key} key after a passing F1/F5 check")
    return EXIT_OK


def build_parser() -> argparse.ArgumentParser:
    from . import __doc__ as package_doc

    parser = argparse.ArgumentParser(
        prog="cellos-sign",
        description=package_doc,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--repo", type=Path, default=_repo_root(),
                        help="repository root (default: inferred from this script)")
    parser.add_argument("--allowlist", default=allowlist_mod.DEFAULT_PATH,
                        help=f"allowlist path, repo-relative (default: {allowlist_mod.DEFAULT_PATH})")
    parser.add_argument("--quiet", action="store_true",
                        help="print only warnings and failures")
    parser.add_argument("--strict", action="store_true",
                        help="treat an unverifiable F5 check as a failure "
                             "(always on for --sign)")
    parser.add_argument("--check", action="store_true",
                        help="run the F1 + F5 check and exit without signing")
    parser.add_argument("--sign", dest="targets", nargs="+", metavar="ELF",
                        help="check, then sign these ELFs in place and re-verify")
    parser.add_argument("--objcopy", default=os.environ.get("OBJCOPY", "objcopy"),
                        help="cross objcopy for the target architecture ($OBJCOPY)")
    parser.add_argument("--seed-hex", default=None,
                        help="32-byte hex seed for a production key (CI only; "
                             "default: the reproducible dev key)")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if bool(args.check) == bool(args.targets):
        print("error: pass exactly one of --check or --sign ELF...", file=sys.stderr)
        return EXIT_USAGE
    return run_check(args) if args.check else run_sign(args)
