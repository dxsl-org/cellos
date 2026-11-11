#!/usr/bin/env python3
"""Generate the canonical moving code-size metrics used by Cellos docs."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs" / "code-metrics.generated.md"
RESIDENTS = ROOT / "kernel" / "residents.toml"
CFG_TEST = re.compile(r"^\s*#\[cfg\(test\)\]\s*$")
INLINE_MOD = re.compile(r"^\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{")


@dataclass
class LexerState:
    block_depth: int = 0
    raw_hashes: int | None = None
    braces: int = 0  # `{` minus `}` outside comments, strings and char literals


def line_has_code(line: str, state: LexerState) -> bool:
    """Return whether a Rust physical line contains a token outside comments."""
    index = 0
    has_code = state.raw_hashes is not None

    while index < len(line):
        if state.raw_hashes is not None:
            marker = '"' + ('#' * state.raw_hashes)
            end = line.find(marker, index)
            if end < 0:
                return has_code or bool(line[index:].strip())
            has_code = True
            index = end + len(marker)
            state.raw_hashes = None
            continue

        if state.block_depth:
            if line.startswith("/*", index):
                state.block_depth += 1
                index += 2
            elif line.startswith("*/", index):
                state.block_depth -= 1
                index += 2
            else:
                index += 1
            continue

        if line.startswith("//", index):
            break
        if line.startswith("/*", index):
            state.block_depth = 1
            index += 2
            continue
        if line[index].isspace():
            index += 1
            continue

        raw = raw_string_hashes(line, index)
        if raw is not None:
            hashes, content_index = raw
            state.raw_hashes = hashes
            has_code = True
            index = content_index
            continue

        if line[index] == '"':
            has_code = True
            index = skip_quoted_string(line, index + 1)
            continue

        if line[index] == "'":
            end = char_literal_end(line, index)
            has_code = True
            index = end if end is not None else index + 1
            continue

        if line[index] == "{":
            state.braces += 1
        elif line[index] == "}":
            state.braces -= 1
        has_code = True
        index += 1

    return has_code


def char_literal_end(line: str, index: int) -> int | None:
    """End of a char literal starting at `index`, or None for a lifetime."""
    if line.startswith("\\", index + 1):
        end = line.find("'", index + 3)
        return end + 1 if end >= 0 else None
    if index + 2 < len(line) and line[index + 2] == "'":
        return index + 3
    return None


def raw_string_hashes(line: str, index: int) -> tuple[int, int] | None:
    if line[index] != "r":
        return None
    cursor = index + 1
    while cursor < len(line) and line[cursor] == "#":
        cursor += 1
    if cursor >= len(line) or line[cursor] != '"':
        return None
    return cursor - index - 1, cursor + 1


def skip_quoted_string(line: str, index: int) -> int:
    escaped = False
    while index < len(line):
        char = line[index]
        if char == '"' and not escaped:
            return index + 1
        if char == "\\" and not escaped:
            escaped = True
        else:
            escaped = False
        index += 1
    return index


def test_residents() -> set[str]:
    """Files `kernel/residents.toml` classifies as test, fixture or measurement code."""
    residents = tomllib.loads(RESIDENTS.read_text(encoding="utf-8"))["residents"]
    return {path for path, entry in residents.items() if entry["category"] == "test"}


TEST_RESIDENTS = test_residents()


def is_test_file(path: Path) -> bool:
    return str(path.relative_to(ROOT)) in TEST_RESIDENTS


def is_core_file(path: Path) -> bool:
    relative = path.relative_to(ROOT / "kernel" / "src")
    parts = relative.parts
    return not (
        is_test_file(path)
        or (parts and parts[0] in ("dev", "dev.rs"))
        or (parts and parts[0] == "hypervisor")
    )


def count_nloc(paths: list[Path]) -> int:
    """Count code lines, skipping inline `#[cfg(test)] mod … { … }` blocks."""
    total = 0
    for path in paths:
        state = LexerState()
        held = 0  # attribute lines after `#[cfg(test)]`, dropped if a module follows
        skip_depth = None  # brace depth that ends the skipped module
        depth = 0
        for line in path.read_text(encoding="utf-8").splitlines():
            before = state.braces
            code = line_has_code(line, state)
            if skip_depth is None:
                if CFG_TEST.match(line) or (held and line.lstrip().startswith("#[")):
                    held += 1
                elif held and INLINE_MOD.match(line):
                    held = 0
                    skip_depth = depth
                else:
                    total += held + int(code)
                    held = 0
            depth += state.braces - before
            if skip_depth is not None and depth <= skip_depth:
                skip_depth = None
        total += held
    return total


def render() -> str:
    source = ROOT / "kernel" / "src"
    files = sorted(source.rglob("*.rs"))
    production = [path for path in files if not is_test_file(path)]
    core = [path for path in files if is_core_file(path)]
    return f"""# Generated code metrics

<!-- Generated by scripts/generate-code-metrics.py; do not edit by hand. -->

| Metric | Definition | Value |
|---|---|---:|
| Kernel nLOC | Non-blank, non-comment Rust lines under `kernel/src`; excludes `test` residents of `kernel/residents.toml` and inline `#[cfg(test)]` modules | **{count_nloc(production):,}** |
| Core kernel nLOC | Kernel nLOC excluding `kernel/src/dev.rs`, `kernel/src/dev/**` and `kernel/src/hypervisor/**` | **{count_nloc(core):,}** |

The first row is the canonical moving kernel-size metric. The second is the boundary-migration
lens used by Spec 15. Neither number is a substitute for reviewing kernel responsibilities.
"""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    expected = render()
    if args.check:
        actual = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else ""
        if actual != expected:
            print(f"stale generated metrics: run {Path(__file__).name}", file=sys.stderr)
            return 1
        return 0
    OUTPUT.write_text(expected, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
