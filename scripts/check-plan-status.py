#!/usr/bin/env python3
"""Refuse plan/phase files that claim completion while acceptance boxes are unchecked.

A `plan.md` or `phase-*.md` under `.agents/` whose frontmatter `status:` is
`complete`/`completed` must have no `- [ ]` item inside a section whose heading starts with `Todo` or
contains `Success Criteria` / `Acceptance Criteria`. This is the guard that keeps a plan header from
claiming a result its own checklist does not record (architecture review C-2).

Enforcement scope: plan directories named `YYMMDD-HHMM-*` at or after the
cutoff are enforced; older directories are reported only, so ~200 historical
plans do not block CI. `--enforce-all` enforces everything.

Exit status: 0 when no enforced file violates, 1 otherwise.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

CUTOFF = "261110-1703"
CHECKED_SECTIONS = re.compile(r"^#{2,4}\s+(?:todo\b|.*\b(?:success criteria|acceptance criteria)\b)", re.I)
ANY_HEADING = re.compile(r"^#{1,4}\s+")
UNCHECKED = re.compile(r"^\s*[-*]\s+\[ \]")
STATUS = re.compile(r"^status:\s*(.+?)\s*$", re.M)
DIR_STAMP = re.compile(r"^(\d{6}-\d{4})")


def frontmatter_status(text: str) -> str | None:
    if not text.startswith("---\n"):
        return None
    end = text.find("\n---", 4)
    if end < 0:
        return None
    match = STATUS.search(text[4:end])
    return match.group(1).strip().strip("\"'").lower() if match else None


def unchecked_items(text: str) -> list[int]:
    lines: list[int] = []
    in_section = False
    for number, line in enumerate(text.splitlines(), 1):
        if ANY_HEADING.match(line):
            in_section = bool(CHECKED_SECTIONS.match(line))
            continue
        if in_section and UNCHECKED.match(line):
            lines.append(number)
    return lines


def is_enforced(path: Path, root: Path, enforce_all: bool) -> bool:
    if enforce_all:
        return True
    rel = path.relative_to(root)
    stamp = DIR_STAMP.match(rel.parts[0]) if len(rel.parts) > 1 else None
    return bool(stamp) and stamp.group(1) >= CUTOFF


def candidates(root: Path):
    for path in sorted(root.rglob("*.md")):
        rel = path.relative_to(root)
        if rel.parts[0] == "archive":
            continue
        if path.name == "plan.md" or path.name.startswith("phase-"):
            yield path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent / ".agents")
    parser.add_argument("--enforce-all", action="store_true")
    parser.add_argument("--quiet-legacy", action="store_true", help="print only the legacy count")
    args = parser.parse_args()

    root = args.root.resolve()
    failures: list[str] = []
    legacy: list[str] = []
    for path in candidates(root):
        text = path.read_text(encoding="utf-8", errors="replace")
        if frontmatter_status(text) not in {"complete", "completed"}:
            continue
        lines = unchecked_items(text)
        if not lines:
            continue
        entry = f"{path.relative_to(root)}: status complete with unchecked item(s) at line(s) {', '.join(map(str, lines))}"
        (failures if is_enforced(path, root, args.enforce_all) else legacy).append(entry)

    if legacy:
        print(f"report-only (plans before {CUTOFF}): {len(legacy)} file(s)")
        if not args.quiet_legacy:
            for entry in legacy:
                print(f"  {entry}")
    for entry in failures:
        print(f"FAIL {entry}")
    if failures:
        return 1
    print("plan status: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
