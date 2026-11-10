#!/usr/bin/env python3
"""Keep scripts/lanes.toml complete and honest.

Every tracked lane script (`scripts/{qemu,run}-*`, `scripts/build-*-ci.sh`,
`scripts/x86/{qemu,run}-*`) and every `tests/integration/tests/*.rs` target must
have a row (test names as `cargo metadata` reports them). A `pr` row must be named by `.github/workflows/ci.yml`; a `nightly`
row by any workflow; `manual`/`retired` rows need a `reason`. Rows for paths or
tests that no longer exist fail. Exit 0 when consistent, 1 otherwise.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LANE_PATTERNS = (
    re.compile(r"^scripts/(qemu|run)-[^/]+$"),
    re.compile(r"^scripts/build-[^/]+-ci\.sh$"),
    re.compile(r"^scripts/x86/(qemu|run)-[^/]+$"),
)
TIERS = {"pr", "nightly", "manual", "retired"}


def tracked_lane_scripts() -> set[str]:
    out = subprocess.run(["git", "ls-files", "scripts"], cwd=ROOT, check=True,
                         capture_output=True, text=True).stdout.split()
    return {p for p in out if any(rx.match(p) for rx in LANE_PATTERNS)}


def integration_tests() -> set[str]:
    """Test target names as cargo sees them ([[test]] names plus auto-discovered files)."""
    out = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"],
                         cwd=ROOT / "tests/integration", check=True, capture_output=True, text=True).stdout
    return {t["name"] for p in json.loads(out)["packages"] for t in p["targets"] if "test" in t["kind"]}


def names_test(text: str, name: str) -> bool:
    return re.search(r"--test(?:[\s\\]+|=)" + re.escape(name) + r"(?![\w-])", text) is not None


def names_script(text: str, path: str) -> bool:
    return path in text or re.search(r"(?<![\w-])" + re.escape(Path(path).name) + r"(?![\w-])", text) is not None


def main() -> int:
    manifest = tomllib.loads((ROOT / "scripts/lanes.toml").read_text())
    workflows = {p.name: p.read_text() for p in (ROOT / ".github/workflows").glob("*.yml")}
    ci = workflows.get("ci.yml", "")
    every = "".join(workflows.values())
    errors: list[str] = []

    def check_row(kind: str, key: str, row: dict, named) -> None:
        tier = row.get("tier")
        if tier not in TIERS:
            errors.append(f"{kind} {key}: tier must be one of {sorted(TIERS)}")
        elif tier == "pr" and not named(ci):
            errors.append(f"{kind} {key}: tier pr but not named by ci.yml")
        elif tier == "nightly" and not named(every):
            errors.append(f"{kind} {key}: tier nightly but not named by any workflow")
        elif tier in {"manual", "retired"} and not row.get("reason"):
            errors.append(f"{kind} {key}: tier {tier} needs a reason")

    lanes = {row["path"]: row for row in manifest.get("lane", [])}
    for path in sorted(tracked_lane_scripts() - lanes.keys()):
        errors.append(f"lane {path}: missing from scripts/lanes.toml")
    for path, row in lanes.items():
        if not (ROOT / path).exists():
            errors.append(f"lane {path}: listed but does not exist")
            continue
        check_row("lane", path, row, lambda text, p=path: names_script(text, p))

    tests = {row["name"]: row for row in manifest.get("test", [])}
    for name in sorted(integration_tests() - tests.keys()):
        errors.append(f"test {name}: missing from scripts/lanes.toml")
    targets = integration_tests()
    for name, row in tests.items():
        if name not in targets:
            errors.append(f"test {name}: listed but is not a tests/integration test target")
            continue
        check_row("test", name, row, lambda text, n=name: names_test(text, n))

    for error in errors:
        print(f"FAIL {error}")
    if errors:
        return 1
    print(f"lanes: OK ({len(lanes)} scripts, {len(tests)} integration targets)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
