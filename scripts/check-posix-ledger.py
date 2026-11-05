#!/usr/bin/env python3
"""POSIX ledger checker: docs/posix-ledger.md against the adapter sources.

Discovers the two optional POSIX adapter surfaces (ADR-0018) from source and
cross-checks them against the ledger tables. Discovery finds drift; it never
proves POSIX behaviour, so a behavioural status needs a behavioural anchor.

Discovery:

    Tier A  libs/api/src/services/posix/**.rs
            `#[no_mangle]` items that are `extern "C" fn` or `static`, plus
            identifiers passed to a `macro_rules!` macro whose body emits a
            `#[no_mangle]` function (the C99 math bridge). A symbol defined
            once per target architecture (setjmp/longjmp) counts once.
    Tier B  third_party/mlibc/sysdeps/vicell/generic/*.cpp
            top-level `Sysdeps<Tag>::operator()` definitions (mlibc sysdeps),
            named by their tag, e.g. `Open`.

Ledger tables are recognised by their exact header row:

    Tier A  | profile | kind | entry_point | backing | source | semantic_subset
            | unsupported_result | status | evidence |
    Tier B  | profile | kind | sysdep | consumer_visible_symbol | backing | source
            | semantic_subset | unsupported_result | status | evidence |

Closed vocabularies:

    kind    libc-symbol, libc-symbol-family, cxx-abi-symbol, c-global (Tier A);
            mlibc-sysdep (Tier B)
    status  implemented, partial, refused, unverified

Anchors (`source` and `evidence` cells, separated by ";", backticks optional,
`none` for an empty evidence cell):

    <path>:<N>[-<M>]   lines N..M exist in <path>
    <path>::<name>     <path> defines function <name>

A `source` anchor range must mention every symbol of its row. An
`implemented`, `partial` or `refused` row needs at least one behavioural
evidence anchor: `<path>::<name>` naming a Rust `#[test]` or Python `test*`
function, or `<path>:<N>[-<M>]` under docs/evidence/.

Exit 0 when the ledger and sources agree, 1 with `path:line:` diagnostics
otherwise. The script needs no toolchain and never compiles the tree.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

LEDGER = Path("docs/posix-ledger.md")
TIER_A_DIR = Path("libs/api/src/services/posix")
TIER_B_DIR = Path("third_party/mlibc/sysdeps/vicell/generic")
EVIDENCE_DIR = "docs/evidence/"

TIER_A_COLUMNS = (
    "profile", "kind", "entry_point", "backing", "source", "semantic_subset",
    "unsupported_result", "status", "evidence",
)
TIER_B_COLUMNS = (
    "profile", "kind", "sysdep", "consumer_visible_symbol", "backing", "source",
    "semantic_subset", "unsupported_result", "status", "evidence",
)
TIER_A_KINDS = ("libc-symbol", "libc-symbol-family", "cxx-abi-symbol", "c-global")
TIER_B_KINDS = ("mlibc-sysdep",)
STATUSES = ("implemented", "partial", "refused", "unverified")
BEHAVIOURAL = ("implemented", "partial", "refused")

NO_MANGLE_RE = re.compile(r"^\s*#\[(?:unsafe\()?no_mangle\)?\]\s*$")
ATTR_OR_COMMENT_RE = re.compile(r"^\s*(#\[|//)")
FN_RE = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?extern\s+"C"\s+fn\s+(\$?\w+)'
)
STATIC_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+(?:mut\s+)?(\$?\w+)")
MACRO_DEF_RE = re.compile(r"^\s*macro_rules!\s+(\w+)")
SYSDEP_RE = re.compile(r"^[A-Za-z_][\w:<>*&\s]*?\bSysdeps<(\w+)>::operator\(\)\s*\(")
SYMBOL_RE = re.compile(r"^[A-Za-z_]\w*$")
LINE_ANCHOR_RE = re.compile(r"^(?P<path>[^\s:]+):(?P<start>\d+)(?:-(?P<end>\d+))?$")
FN_ANCHOR_RE = re.compile(r"^(?P<path>[^\s:]+)::(?P<name>\w+)$")


class Export:
    def __init__(self, name: str, kind: str, path: str, line: int):
        self.name = name
        self.kind = kind  # "fn" or "static"
        self.path = path
        self.line = line


class Row:
    def __init__(self, tier: str, line: int, cells: dict[str, str]):
        self.tier = tier
        self.line = line
        self.cells = cells
        self.symbols: list[str] = []


class Checker:
    def __init__(self, repo: Path):
        self.repo = repo
        self.errors: list[str] = []
        self._lines: dict[str, list[str] | None] = {}

    def error(self, where: str, line: int, message: str) -> None:
        self.errors.append(f"{where}:{line}: {message}")

    def lines(self, rel: str) -> list[str] | None:
        if rel not in self._lines:
            path = self.repo / rel
            self._lines[rel] = (
                path.read_text(encoding="utf-8", errors="ignore").splitlines()
                if path.is_file()
                else None
            )
        return self._lines[rel]

    # ------------------------------------------------------------------ discovery

    def discover_tier_a(self) -> dict[str, Export]:
        root = self.repo / TIER_A_DIR
        if not root.is_dir():
            self.error(TIER_A_DIR.as_posix(), 0, "Tier A source directory missing")
            return {}
        exports: dict[str, Export] = {}
        exporting_macros: set[str] = set()
        files = sorted(root.rglob("*.rs"))
        for path in files:
            rel = path.relative_to(self.repo).as_posix()
            src = self.lines(rel) or []
            macro = None
            for idx, text in enumerate(src):
                found = MACRO_DEF_RE.match(text)
                if found:
                    macro = found.group(1)
                    continue
                if not NO_MANGLE_RE.match(text):
                    continue
                item = idx + 1
                while item < len(src) and ATTR_OR_COMMENT_RE.match(src[item]):
                    item += 1
                text_item = src[item] if item < len(src) else ""
                fn = FN_RE.match(text_item)
                static = STATIC_RE.match(text_item)
                name, kind = (fn.group(1), "fn") if fn else (
                    (static.group(1), "static") if static else (None, None)
                )
                if name is None:
                    self.error(rel, item + 1, "#[no_mangle] item is neither extern \"C\" fn nor static")
                    continue
                if name.startswith("$"):
                    if macro is None:
                        self.error(rel, item + 1, "#[no_mangle] macro template outside macro_rules!")
                    else:
                        exporting_macros.add(macro)
                    continue
                exports.setdefault(name, Export(name, kind, rel, item + 1))
        if exporting_macros:
            invoke = re.compile(
                r"^\s*(" + "|".join(sorted(map(re.escape, exporting_macros))) + r")!\s*\(\s*(\w+)"
            )
            for path in files:
                rel = path.relative_to(self.repo).as_posix()
                for idx, text in enumerate(self.lines(rel) or []):
                    found = invoke.match(text)
                    if found:
                        name = found.group(2)
                        exports.setdefault(name, Export(name, "fn", rel, idx + 1))
        return exports

    def discover_tier_b(self) -> dict[str, Export]:
        root = self.repo / TIER_B_DIR
        files = sorted(root.glob("*.cpp")) if root.is_dir() else []
        if not files:
            self.error(TIER_B_DIR.as_posix(), 0, "Tier B sysdep sources missing")
            return {}
        exports: dict[str, Export] = {}
        for path in files:
            rel = path.relative_to(self.repo).as_posix()
            for idx, text in enumerate(self.lines(rel) or []):
                if text[:1].isspace() or text.rstrip().endswith(";"):
                    continue
                found = SYSDEP_RE.match(text)
                if found:
                    name = found.group(1)
                    if name in exports:
                        self.error(rel, idx + 1, f"Tier B sysdep '{name}' defined twice")
                    exports.setdefault(name, Export(name, "fn", rel, idx + 1))
        return exports

    # -------------------------------------------------------------------- ledger

    def parse_ledger(self) -> list[Row]:
        rel = LEDGER.as_posix()
        src = self.lines(rel)
        if src is None:
            self.error(rel, 0, "ledger missing")
            return []
        rows: list[Row] = []
        tier: str | None = None
        columns: tuple[str, ...] = ()
        expect_separator = False
        for idx, text in enumerate(src):
            line = idx + 1
            stripped = text.strip()
            if not stripped.startswith("|"):
                tier = None
                continue
            cells = [c.strip() for c in stripped.strip("|").split("|")]
            if tier is None:
                if tuple(cells) == TIER_A_COLUMNS:
                    tier, columns = "A", TIER_A_COLUMNS
                elif tuple(cells) == TIER_B_COLUMNS:
                    tier, columns = "B", TIER_B_COLUMNS
                else:
                    tier = "other"
                expect_separator = tier != "other"
                continue
            if tier == "other":
                continue
            if expect_separator:
                expect_separator = False
                if not all(re.fullmatch(r":?-{3,}:?", c) for c in cells):
                    self.error(rel, line, "table header must be followed by a separator row")
                continue
            if len(cells) != len(columns):
                self.error(rel, line, f"malformed row: {len(cells)} columns, expected {len(columns)}")
                continue
            rows.append(Row(tier, line, dict(zip(columns, cells))))
        return rows

    def check_row(self, row: Row) -> None:
        rel = LEDGER.as_posix()
        cells = row.cells
        profile = "tier-a" if row.tier == "A" else "tier-b"
        kinds = TIER_A_KINDS if row.tier == "A" else TIER_B_KINDS
        if cells["profile"] != profile:
            self.error(rel, row.line, f"profile '{cells['profile']}' must be '{profile}'")
        kind = cells["kind"]
        if kind not in kinds:
            self.error(rel, row.line, f"kind '{kind}' not in {{{', '.join(kinds)}}}")
        status = cells["status"]
        if status not in STATUSES:
            self.error(rel, row.line, f"status '{status}' not in {{{', '.join(STATUSES)}}}")

        names_cell = cells["entry_point"] if row.tier == "A" else cells["sysdep"]
        names = [n.strip().strip("`").strip() for n in names_cell.split(",")]
        if not all(SYMBOL_RE.match(n) for n in names):
            self.error(rel, row.line, f"malformed symbol list '{names_cell}'")
            names = [n for n in names if SYMBOL_RE.match(n)]
        if kind == "libc-symbol-family" and len(names) < 2:
            self.error(rel, row.line, "libc-symbol-family row must list at least two symbols")
        if kind != "libc-symbol-family" and len(names) != 1:
            self.error(rel, row.line, f"{kind} row must name exactly one symbol")
        row.symbols = names

        source = self.anchors(row.line, cells["source"], "source")
        if not source:
            self.error(rel, row.line, "source anchor required")
        else:
            text = "\n".join(source)
            for name in names:
                if not re.search(rf"\b{re.escape(name)}\b", text):
                    self.error(rel, row.line, f"source anchor range does not mention '{name}'")

        behavioural = self.evidence(row.line, cells["evidence"])
        if status in BEHAVIOURAL and not behavioural:
            self.error(
                rel, row.line,
                f"status '{status}' needs a behavioural evidence anchor (test fn or {EVIDENCE_DIR} path:line)",
            )

    def resolve(self, line: int, anchor: str, column: str) -> tuple[list[str], bool] | None:
        """Resolve one anchor to (cited text, behavioural) or None when invalid."""
        rel = LEDGER.as_posix()
        by_line = LINE_ANCHOR_RE.match(anchor)
        by_fn = FN_ANCHOR_RE.match(anchor)
        if not by_line and not by_fn:
            self.error(rel, line, f"{column} anchor '{anchor}' is not <path>:<N>[-<M>] or <path>::<fn>")
            return None
        path = (by_line or by_fn).group("path")
        src = self.lines(path)
        if src is None:
            self.error(rel, line, f"{column} anchor path '{path}' does not exist")
            return None
        if by_line:
            start = int(by_line.group("start"))
            end = int(by_line.group("end") or start)
            if not 1 <= start <= end <= len(src):
                self.error(rel, line, f"{column} anchor '{anchor}' outside {path} (1-{len(src)})")
                return None
            return src[start - 1:end], path.startswith(EVIDENCE_DIR)
        name = by_fn.group("name")
        for idx, text in enumerate(src):
            if re.match(rf"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+{name}\b", text):
                attrs = idx - 1
                is_test = False
                while attrs >= 0 and src[attrs].strip().startswith(("#[", "///", "//")):
                    is_test = is_test or src[attrs].strip().startswith(("#[test", "#[tokio::test"))
                    attrs -= 1
                return [text], is_test
            if re.match(rf"^\s*(?:async\s+)?def\s+{name}\b", text):
                return [text], name.startswith("test")
        self.error(rel, line, f"{column} anchor '{anchor}': {path} defines no function '{name}'")
        return None

    def anchors(self, line: int, cell: str, column: str) -> list[str]:
        cited: list[str] = []
        for anchor in split_anchors(cell):
            resolved = self.resolve(line, anchor, column)
            if resolved:
                cited.extend(resolved[0])
        return cited

    def evidence(self, line: int, cell: str) -> bool:
        if cell.strip().strip("`") in ("", "none"):
            return False
        behavioural = False
        for anchor in split_anchors(cell):
            resolved = self.resolve(line, anchor, "evidence")
            behavioural = behavioural or bool(resolved and resolved[1])
        return behavioural

    # --------------------------------------------------------------------- check

    def run(self) -> tuple[dict[str, Export], dict[str, Export], list[Row]]:
        tier_a = self.discover_tier_a()
        tier_b = self.discover_tier_b()
        rows = self.parse_ledger()
        rel = LEDGER.as_posix()
        listed: dict[str, dict[str, int]] = {"A": {}, "B": {}}
        for row in rows:
            self.check_row(row)
            exports = tier_a if row.tier == "A" else tier_b
            for name in row.symbols:
                if name in listed[row.tier]:
                    self.error(rel, row.line, f"'{name}' already listed at line {listed[row.tier][name]}")
                    continue
                listed[row.tier][name] = row.line
                export = exports.get(name)
                if export is None:
                    self.error(rel, row.line, f"'{name}' is not exported by Tier {row.tier} source")
                elif row.tier == "A" and (export.kind == "static") != (row.cells["kind"] == "c-global"):
                    self.error(
                        rel, row.line,
                        f"'{name}' is a {'static' if export.kind == 'static' else 'function'}; "
                        f"kind '{row.cells['kind']}' disagrees",
                    )
        for tier, exports in (("A", tier_a), ("B", tier_b)):
            for export in exports.values():
                if export.name not in listed[tier]:
                    self.error(export.path, export.line, f"Tier {tier} export '{export.name}' missing from {rel}")
        return tier_a, tier_b, rows


def split_anchors(cell: str) -> list[str]:
    return [a.strip().strip("`").strip() for a in cell.split(";") if a.strip().strip("`").strip()]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parent.parent,
                        help="repository root (default: this checkout)")
    parser.add_argument("--list", action="store_true", help="print discovered exports with their source lines")
    args = parser.parse_args(argv)

    checker = Checker(args.repo.resolve())
    tier_a, tier_b, rows = checker.run()
    if args.list:
        for tier, exports in (("A", tier_a), ("B", tier_b)):
            for export in sorted(exports.values(), key=lambda e: (e.path, e.line)):
                print(f"tier-{tier.lower()} {export.kind:6} {export.name} {export.path}:{export.line}")

    statuses = Counter((row.tier, row.cells["status"]) for row in rows)
    summary = ", ".join(
        f"Tier {tier} {status}={statuses[(tier, status)]}"
        for tier in ("A", "B") for status in STATUSES if statuses[(tier, status)]
    )
    print(
        f"posix ledger: Tier A {len(tier_a)} exports "
        f"({sum(e.kind == 'static' for e in tier_a.values())} globals), "
        f"Tier B {len(tier_b)} sysdeps, {len(rows)} rows ({summary or 'none'})"
    )
    for message in checker.errors:
        print(f"  {message}")
    if checker.errors:
        print(f"FAIL: {len(checker.errors)} ledger violation(s)")
        return 1
    print("OK: ledger matches discovered exports")
    return 0


if __name__ == "__main__":
    sys.exit(main())
