#!/usr/bin/env python3
"""Check that every kernel source file is a listed, categorised resident.

Input: `kernel/residents.toml` (one entry per Rust file under `kernel/src`,
with its Spec 15 category). Fails when a file is unlisted, a listed file no
longer exists, a category is unknown, an owner is empty, or an `exception`
names no migration target. A new kernel file therefore needs a review decision
recorded in the manifest before it can land.

`--negative-control` runs the same check over the real tree plus one synthetic
unlisted file under `kernel/src/dev/` and exits 0 only if the check rejects it.
Exit 0 when consistent, 1 otherwise.
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "kernel" / "residents.toml"
SOURCE = ROOT / "kernel" / "src"
CATEGORIES = {"A", "B", "C", "D", "exception", "test"}
NEGATIVE_CONTROL = "kernel/src/dev/unlisted-negative-control.rs"


def problems(files: set[str], residents: dict[str, dict]) -> list[str]:
    out = []
    for path in sorted(files - residents.keys()):
        out.append(f"{path}: not listed in kernel/residents.toml")
    for path in sorted(residents.keys() - files):
        out.append(f"{path}: listed but no such file")
    for path, entry in sorted(residents.items()):
        category = entry.get("category")
        if category not in CATEGORIES:
            out.append(f"{path}: unknown category {category!r}")
        if not entry.get("owner"):
            out.append(f"{path}: empty owner")
        target = entry.get("target", "")
        if category == "exception" and not target:
            out.append(f"{path}: exception without a migration target")
        if category != "exception" and target:
            out.append(f"{path}: target given for a non-exception resident")
        unknown = entry.keys() - {"category", "owner", "target"}
        if unknown:
            out.append(f"{path}: unknown keys {sorted(unknown)}")
    return out


def main() -> int:
    residents = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))["residents"]
    files = {str(p.relative_to(ROOT)) for p in SOURCE.rglob("*.rs")}
    if "--negative-control" in sys.argv[1:]:
        found = problems(files | {NEGATIVE_CONTROL}, residents)
        if any(line.startswith(NEGATIVE_CONTROL) for line in found):
            print(f"kernel residents negative control: OK (unlisted {NEGATIVE_CONTROL} rejected)")
            return 0
        print("kernel residents negative control: FAIL (unlisted file accepted)", file=sys.stderr)
        return 1
    found = problems(files, residents)
    for line in found:
        print(f"kernel residents: {line}", file=sys.stderr)
    if found:
        return 1
    counts: dict[str, int] = {}
    for entry in residents.values():
        counts[entry["category"]] = counts.get(entry["category"], 0) + 1
    summary = ", ".join(f"{k}={counts[k]}" for k in sorted(counts))
    print(f"kernel residents: OK ({len(residents)} files; {summary})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
