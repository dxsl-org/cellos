#!/usr/bin/env python3
"""Every workspace crate that contains `#[test]` must be listed in
scripts/host-test-crates.txt; `ci` rows must be named with `-p <crate>` in
.github/workflows/ci.yml, other rows need a reason. Stops new test suites from
silently never running (architecture review: 46+ crates had unrun tests).
Exit 0 when consistent, 1 otherwise.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STATUSES = {"ci", "untested", "not-host-testable"}


def crates_with_tests() -> set[str]:
    meta = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"],
                                     cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    found = set()
    for package in meta["packages"]:
        crate_dir = Path(package["manifest_path"]).parent
        for source in crate_dir.rglob("*.rs"):
            if "target" in source.relative_to(crate_dir).parts:
                continue
            if "#[test]" in source.read_text(errors="ignore"):
                found.add(package["name"])
                break
    return found


def main() -> int:
    rows: dict[str, tuple[str, str]] = {}
    errors: list[str] = []
    for number, line in enumerate((ROOT / "scripts/host-test-crates.txt").read_text().splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        name, status, *reason = line.split()
        if status not in STATUSES:
            errors.append(f"line {number}: unknown status {status!r}")
        rows[name] = (status, " ".join(reason))

    ci_named = set(re.findall(r"-p\s+([\w-]+)", (ROOT / ".github/workflows/ci.yml").read_text()))
    present = crates_with_tests()
    for name in sorted(present - rows.keys()):
        errors.append(f"{name}: contains #[test] but is missing from scripts/host-test-crates.txt")
    for name, (status, reason) in sorted(rows.items()):
        if name not in present:
            errors.append(f"{name}: listed but no workspace crate with #[test] has that name")
        elif status == "ci" and name not in ci_named:
            errors.append(f"{name}: status ci but ci.yml never runs `-p {name}`")
        elif status != "ci" and not reason:
            errors.append(f"{name}: status {status} needs a reason")

    for error in errors:
        print(f"FAIL {error}")
    if errors:
        return 1
    run = sum(1 for status, _ in rows.values() if status == "ci")
    print(f"host test crates: OK ({run} run by ci.yml, {len(rows) - run} listed with a reason)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
