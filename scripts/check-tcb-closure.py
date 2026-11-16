#!/usr/bin/env python3
"""Report the Tier-1 closure ratchet, or prove it rejects new `unsafe` in `libs/ostd`.

The ratchet itself lives in `cellos-sign` (`scripts/cellos_sign/tcb.py`) and runs
in `--check` and before every `--sign`; this tool prints its numbers for
evidence and review:

    python3 scripts/check-tcb-closure.py                     # per-crate totals
    python3 scripts/check-tcb-closure.py --negative-control

`--negative-control` runs the signer's policy check (`policy.check`, strict, as
`--sign` runs it) on the real tree, closure file and baseline three times, with
one in-memory addition each and nothing written:
  * a new file `libs/ostd/src/tcb_negative_control.rs` holding one `unsafe` block;
  * one `unsafe` block appended to `libs/ostd/src/syscall.rs` (a listed file);
  * one `#[no_mangle]` appended to `libs/ostd/src/lib.rs` (the attribute ceiling).
It exits 0 only if the unmodified tree passes and each addition fails with a
closure (`tcb`) violation naming that file.

Exit 0 when the ratchet passes (or the control behaves as designed), 1 otherwise.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from cellos_sign import allowlist, policy, tcb  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
CASES = (
    ("libs/ostd/src/tcb_negative_control.rs", "fn tcb_negative_control() { unsafe {} }",
     "no row"),
    ("libs/ostd/src/syscall.rs", "fn tcb_negative_control() { unsafe {} }",
     "exceed the ceiling"),
    ("libs/ostd/src/lib.rs", "#[no_mangle]\nfn tcb_negative_control() {}",
     "unsafe attribute site(s) exceed the ceiling"),
)


def run(extra: dict[str, str] | None = None) -> policy.Result:
    return policy.check(
        REPO, allowlist.load(REPO), strict=True,
        closure=tcb.load_closure(REPO), baseline=tcb.load_baseline(REPO), extra=extra,
    )


def negative_control() -> int:
    clean = run()
    if not clean.ok:
        print("tcb negative control: FAIL (the unmodified tree does not pass, so the "
              "control proves nothing):", file=sys.stderr)
        for v in clean.violations:
            print(f"  {v}", file=sys.stderr)
        return 1
    failed = False
    for path, code, expected in CASES:
        result = run({path: code})
        hits = [v for v in result.violations
                if v.layer == "tcb" and v.path == path and expected in v.detail]
        if hits:
            print(f"tcb negative control: OK — {path}: {hits[0].detail}")
        else:
            failed = True
            print(f"tcb negative control: FAIL — {path} + `{code}` was not rejected "
                  f"({[str(v) for v in result.violations]})", file=sys.stderr)
    return 1 if failed else 0


def report() -> int:
    result = run()
    r = result.tcb
    print(f"{'in-tree closure crate':24} {'files':>5} {'unsafe':>7} {'attrs':>6}")
    for name, counts in sorted(r.per_package.items()):
        if counts:
            files = sum(1 for p in r.per_file if p.startswith(
                tcb.load_closure(REPO).packages[name] + "/"))
            print(f"{name:24} {files:>5} {counts.unsafe:>7} {counts.attrs:>6}")
    print(f"{'closure':24} {len(r.per_file):>5} {r.closure.unsafe:>7} {r.closure.attrs:>6}")
    print(f"{'Tier-1 cells':24} {'':>5} {r.cells.unsafe:>7} {r.cells.attrs:>6}")
    print(f"{'Tier-1 total':24} {'':>5} {r.total.unsafe:>7} {r.total.attrs:>6}")
    with_unsafe = sum(1 for t in tcb.load_closure(REPO).thirdparty if t["unsafe"])
    print(f"third-party pins: {r.thirdparty_pins} name@version ({with_unsafe} with `unsafe`; "
          f"{r.thirdparty.unsafe} tokens, {r.thirdparty.attrs} attrs)")
    for v in result.violations:
        print(f"FAIL: {v}", file=sys.stderr)
    return 0 if result.ok else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--negative-control", action="store_true",
                        help="prove a new unlisted `unsafe` in libs/ostd fails the check")
    args = parser.parse_args()
    return negative_control() if args.negative_control else report()


if __name__ == "__main__":
    sys.exit(main())
