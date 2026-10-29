#!/usr/bin/env python3
"""Gate G-S1a: the cellbox ceiling must equal the shell ceiling it replaces.

Extracts the `declare_manifest!(...)` key/value pairs and the
`declare_syscalls![...]` identifier list from a candidate `main.rs` and from a
baseline (default: `git show HEAD:cells/tools/shell/src/main.rs`) and fails when
the two sets differ in either direction (an added *or* a removed entry).

Usage:
    scripts/cellbox-ceiling-diff.py [CANDIDATE_MAIN_RS]
        [--baseline-ref REF] [--baseline-path PATH] [--baseline-file FILE]

CANDIDATE_MAIN_RS defaults to cells/tools/cellbox/src/main.rs. Exit status:
0 identical, 1 differ, 2 usage/extraction error.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CANDIDATE = "cells/tools/cellbox/src/main.rs"
DEFAULT_BASELINE_PATH = "cells/tools/shell/src/main.rs"


class ExtractionError(Exception):
    pass


def strip_comments(text: str) -> str:
    """Drop // and /* */ comments (string literals are not used in the ceiling)."""
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def macro_body(text: str, name: str, opener: str, closer: str) -> str:
    match = re.search(r"\b(?:api::)?" + re.escape(name) + r"\s*!\s*" + re.escape(opener), text)
    if not match:
        raise ExtractionError(f"no {name}!{opener}...{closer} invocation found")
    start = match.end()
    depth = 1
    for index in range(start, len(text)):
        if text[index] == opener:
            depth += 1
        elif text[index] == closer:
            depth -= 1
            if depth == 0:
                return text[start:index]
    raise ExtractionError(f"unterminated {name}! invocation")


def extract(source: str) -> tuple[dict[str, str], set[str]]:
    clean = strip_comments(source)
    manifest_body = macro_body(clean, "declare_manifest", "(", ")")
    manifest: dict[str, str] = {}
    for item in filter(None, (part.strip() for part in manifest_body.split(","))):
        key, separator, value = item.partition("=")
        if not separator or not key.strip():
            raise ExtractionError(f"malformed declare_manifest entry: {item!r}")
        key = key.strip()
        if key in manifest:
            raise ExtractionError(f"duplicate declare_manifest key: {key}")
        manifest[key] = re.sub(r"\s+", "", value)
    syscalls_body = macro_body(clean, "declare_syscalls", "[", "]")
    names = [part.strip() for part in syscalls_body.split(",") if part.strip()]
    for name in names:
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
            raise ExtractionError(f"malformed declare_syscalls entry: {name!r}")
    if len(set(names)) != len(names):
        raise ExtractionError("duplicate declare_syscalls entry")
    return manifest, set(names)


def read_baseline(args: argparse.Namespace) -> tuple[str, str]:
    if args.baseline_file:
        return Path(args.baseline_file).read_text(encoding="utf-8"), args.baseline_file
    spec = f"{args.baseline_ref}:{args.baseline_path}"
    try:
        out = subprocess.run(
            ["git", "show", spec], cwd=ROOT, check=True, capture_output=True, text=True
        ).stdout
    except subprocess.CalledProcessError as error:
        raise ExtractionError(f"git show {spec} failed: {error.stderr.strip()}") from error
    return out, f"git {spec}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("candidate", nargs="?", default=DEFAULT_CANDIDATE)
    parser.add_argument("--baseline-ref", default="HEAD")
    parser.add_argument("--allow-added", action="append", default=["Write", "GetRandom"], help="syscalls added by an approved delta (default: Write, Phase 02 CP2 closure; GetRandom, Phase 09 D79-9 as revised)")
    parser.add_argument("--baseline-path", default=DEFAULT_BASELINE_PATH)
    parser.add_argument("--baseline-file", help="read the baseline from a file instead of git")
    args = parser.parse_args()

    candidate_path = Path(args.candidate)
    if not candidate_path.is_absolute():
        candidate_path = ROOT / candidate_path
    try:
        candidate_text = candidate_path.read_text(encoding="utf-8")
        baseline_text, baseline_name = read_baseline(args)
        cand_manifest, cand_syscalls = extract(candidate_text)
        base_manifest, base_syscalls = extract(baseline_text)
    except (OSError, ExtractionError) as error:
        print(f"ceiling-diff: error: {error}", file=sys.stderr)
        return 2

    problems: list[str] = []
    for name in sorted(cand_syscalls - base_syscalls):
        (problems.append(f"syscall ADDED vs baseline: {name}") if name not in args.allow_added else print(f"note: {name} added vs baseline by an approved delta"))
    for name in sorted(base_syscalls - cand_syscalls):
        problems.append(f"syscall REMOVED vs baseline: {name}")
    for key in sorted(set(cand_manifest) | set(base_manifest)):
        old, new = base_manifest.get(key), cand_manifest.get(key)
        if old != new:
            problems.append(f"manifest {key}: baseline={old} candidate={new}")

    print(f"baseline : {baseline_name} ({len(base_syscalls)} syscalls, {len(base_manifest)} manifest keys)")
    print(f"candidate: {candidate_path} ({len(cand_syscalls)} syscalls, {len(cand_manifest)} manifest keys)")
    if problems:
        print("G-S1a FAIL: ceiling differs from the baseline")
        for problem in problems:
            print(" -", problem)
        return 1
    print("G-S1a PASS: declare_manifest and declare_syscalls sets are identical")
    return 0


if __name__ == "__main__":
    sys.exit(main())
