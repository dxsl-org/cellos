#!/usr/bin/env python3
"""Spec 21 anchor checker and generated status (rollout step 1: warn-only).

Resolves every `Anchor:` line under docs/specs/ against the tree, reports
findings, and regenerates docs/spec-status.generated.md.

Anchor grammar (Spec 21 §3):

    impl    <path>::<symbol>            file exists and declares <symbol>
    const   <path>::<NAME>=<value>      constant exists and its value matches
    test    <path>::<test_name>         test function exists in the file
    planned .agents/<plan-dir>[/NN]     plan dir exists; forbidden on a
                                        Ratified/Accepted/Definitive section
    absent  <symbol>                    symbol appears nowhere in
                                        kernel/, libs/, cells/
    design                              nothing to resolve (rationale only)

Several anchors may share one line, separated by " · ".

Structural pass (Phase 05, report-only for meaning, fail-only for structure):

    plan status   YAML-frontmatter `status:` of .agents/*/plan.md; an
                  unterminated frontmatter block or a duplicate, empty or
                  non-scalar `status:` is a structural error
    doc links     inline Markdown links in .agents/plan-portfolio.md,
                  README.md and docs/README.md; a local target that does not
                  exist is a structural error (external schemes and
                  anchor-only links are ignored, #fragments stripped)
    plan claims   every plan those docs link is listed with its frontmatter
                  status and the linking file:line. Linked prose may describe
                  scheduling, evidence or implementation state, so statuses
                  are never compared, equated or rewritten.

Modes:
  (default)          report findings, rewrite docs/spec-status.generated.md,
                     exit 0
  --check            do not write; exit 1 when the committed status file is
                     stale or a structural error exists
  --check-structure  do not write; exit 1 only on structural errors
  --strict           exit 1 on anchor violations and on coverage gaps in
                     Ratified/Accepted/Definitive sections (rollout step 3)

The script needs no toolchain and never compiles the tree, so it runs on every
push cheaply, alongside scripts/check-cell-va-layout.py.
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent.parent
SPECS_DIR = ROOT / "docs" / "specs"
STATUS_FILE = ROOT / "docs" / "spec-status.generated.md"
ABSENT_ROOTS = ("kernel", "libs", "cells")
PLAN_GLOB = ".agents/*/plan.md"
CLAIM_DOCS = (".agents/plan-portfolio.md", "README.md", "docs/README.md")

RATIFIED = ("ratified", "accepted", "definitive")
STATUS_WORDS = (
    "ratified",
    "accepted",
    "definitive",
    "draft",
    "proposed",
    "planned",
    "deprecated",
)

FENCE_RE = re.compile(r"^\s*(```|~~~)")
HEADING_RE = re.compile(r"^(#{2,4})\s+(.*?)\s*$")
ANCHOR_RE = re.compile(r"^\s*>?\s*\**\s*Anchor\**:\s*(.+?)\s*$")
STATUS_RE = re.compile(r"^\s*>?\s*\**\s*Status\**:\s*(.+?)\s*$")
KEYWORDS = ("status:", "anchor:")
FRONTMATTER_STATUS_RE = re.compile(r"^status\s*:(.*)$")
LINK_RE = re.compile(r"\]\(\s*(<[^>]*>|[^)\s]+)")
CODE_SPAN_RE = re.compile(r"(`+).*?\1")
SCHEME_RE = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*:")

# Status prose Spec 21 §3 forbids outside fenced blocks.
PROSE_PATTERNS = (
    ("✅", re.compile("✅")),
    ("COMPLETE", re.compile(r"\bCOMPLETE\b")),
    ("not implemented", re.compile(r"\bnot implemented\b", re.IGNORECASE)),
    ("LOC", re.compile(r"\bLOC\b")),
)

DECL_PATTERNS = (
    r"\bfn\s+{sym}\b",
    r"\bstruct\s+{sym}\b",
    r"\benum\s+{sym}\b",
    r"\btrait\s+{sym}\b",
    r"\bunion\s+{sym}\b",
    r"\btype\s+{sym}\b",
    r"\bconst\s+{sym}\b",
    r"\bstatic\s+{sym}\b",
    r"\bmod\s+{sym}\b",
    r"macro_rules!\s*{sym}\b",
    r"\b{sym}\s*:",
    r"\b{sym}\s*\(",
)

SUFFIX_RE = re.compile(r"(?<=\d)(u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)")


class Finding:
    def __init__(self, severity: str, spec: str, line: int, subject: str, detail: str):
        self.severity = severity  # violation | coverage | prose | structural
        self.spec = spec
        self.line = line
        self.subject = subject
        self.detail = detail


class Section:
    def __init__(self, spec: str, level: int, title: str, line: int):
        self.spec = spec
        self.level = level
        self.title = title
        self.line = line
        self.anchor: str | None = None
        self.anchor_line = 0
        self.status: str | None = None
        self.resolved: list[tuple[str, str]] = []  # (anchor, state)

    @property
    def ratified(self) -> bool:
        status = (self.status or "").lower()
        return any(word in status for word in RATIFIED)

    def anchor_text(self) -> str:
        return self.anchor if self.anchor else "—"


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="ignore")


def parse_spec(path: Path) -> tuple[list[Section], list[Finding]]:
    """Parse one spec into sections plus status-prose findings."""
    rel = str(path.relative_to(ROOT))
    lines = read_text(path).splitlines()
    findings: list[Finding] = []
    sections: list[Section] = []

    doc_status: str | None = None
    fenced = False
    current: Section | None = None
    body_window = 0  # non-empty lines inspected since the section heading

    preview = "\n".join(lines[:20])
    for raw in preview.splitlines():
        match = STATUS_RE.match(raw)
        if match:
            doc_status = match.group(1).lower()
            break

    for index, raw in enumerate(lines, start=1):
        if FENCE_RE.match(raw):
            fenced = not fenced
            continue

        heading = None if fenced else HEADING_RE.match(raw)
        if heading:
            level = len(heading.group(1))
            current = Section(rel, level, heading.group(2), index)
            sections.append(current)
            body_window = 0
            continue

        if fenced:
            continue

        for label, pattern in PROSE_PATTERNS:
            if pattern.search(raw):
                findings.append(
                    Finding("prose", rel, index, label, raw.strip()[:120])
                )
                break

        if current is None:
            continue

        stripped = raw.strip()
        if not stripped:
            continue
        body_window += 1
        if body_window > 8:
            continue

        if current.anchor is None:
            anchor = ANCHOR_RE.match(raw)
            if anchor:
                current.anchor = anchor.group(1)
                current.anchor_line = index
                continue
        if current.status is None:
            status = STATUS_RE.match(raw)
            if status:
                current.status = status.group(1).lower()
                continue

    for section in sections:
        if section.status is None:
            section.status = doc_status

    return sections, findings


def declared_symbol(src: str, symbol: str) -> bool:
    last = symbol.split("::")[-1]
    for pattern in DECL_PATTERNS:
        if re.search(pattern.format(sym=re.escape(last)), src):
            if "::" in symbol:
                owner = symbol.split("::")[-2]
                if not re.search(rf"\b{re.escape(owner)}\b", src):
                    continue
            return True
    return False


def eval_int(expr: str) -> int | None:
    """Evaluate the simple integer arithmetic a const initializer may use."""
    cleaned = SUFFIX_RE.sub("", expr).replace("_", "").strip()
    try:
        tree = ast.parse(cleaned, mode="eval")
    except SyntaxError:
        return None

    def walk(node: ast.AST) -> int | None:
        if isinstance(node, ast.Expression):
            return walk(node.body)
        if isinstance(node, ast.Constant) and isinstance(node.value, int):
            return node.value
        if isinstance(node, ast.BinOp):
            left, right = walk(node.left), walk(node.right)
            if left is None or right is None:
                return None
            if isinstance(node.op, ast.Add):
                return left + right
            if isinstance(node.op, ast.Sub):
                return left - right
            if isinstance(node.op, ast.Mult):
                return left * right
            if isinstance(node.op, ast.FloorDiv) and right:
                return left // right
            if isinstance(node.op, ast.LShift):
                return left << right
            return None
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
            inner = walk(node.operand)
            return None if inner is None else -inner
        return None

    return walk(tree)


def resolve_anchor(anchor: str) -> tuple[str, str]:
    """Resolve one anchor to (state, note)."""
    text = anchor.strip()
    if text == "design":
        return "OK", "design"

    kind, _, rest = text.partition(" ")
    rest = rest.strip()
    if not kind or not rest:
        return "MALFORMED", "anchor needs a kind and a target"

    if kind == "impl":
        path, _, symbol = rest.partition("::")
        if not symbol:
            return "MALFORMED", "impl needs <path>::<symbol>"
        target = ROOT / path
        if not target.is_file():
            return "MISSING-FILE", path
        if declared_symbol(read_text(target), symbol):
            return "OK", text
        return "MISSING-SYMBOL", symbol

    if kind == "test":
        path, _, name = rest.partition("::")
        if not name:
            return "MALFORMED", "test needs <path>::<test_name>"
        target = ROOT / path
        if not target.is_file():
            return "MISSING-FILE", path
        if re.search(rf"\bfn\s+{re.escape(name.split('::')[-1])}\b", read_text(target)):
            return "OK", text
        return "MISSING-TEST", name

    if kind == "const":
        path, _, tail = rest.partition("::")
        name, _, expected = tail.partition("=")
        if not name or not expected:
            return "MALFORMED", "const needs <path>::<NAME>=<value>"
        target = ROOT / path
        if not target.is_file():
            return "MISSING-FILE", path
        match = re.search(
            rf"\bconst\s+{re.escape(name.strip())}\s*:\s*[^=]+?=\s*([^;]+);",
            read_text(target),
        )
        if not match:
            return "MISSING-CONST", name.strip()
        actual = match.group(1).strip()
        want_int, got_int = eval_int(expected), eval_int(actual)
        if want_int is not None and got_int is not None:
            if want_int == got_int:
                return "OK", text
            return "DRIFTED", f"spec {want_int} vs tree {got_int}"
        if expected.strip() == actual:
            return "OK", text
        return "DRIFTED", f"spec {expected.strip()} vs tree {actual}"

    if kind == "planned":
        target = ROOT / rest
        if target.exists():
            return "OK", rest
        return "MISSING-PLAN", rest

    if kind == "absent":
        return "ABSENT", rest

    if kind == "design":
        return "OK", text

    return "MALFORMED", f"unknown anchor kind '{kind}'"


def scan_absent(symbols: set[str]) -> set[str]:
    """Return the subset of symbols that DO exist under kernel/, libs/, cells/."""
    pending = {symbol: re.compile(rf"\b{re.escape(symbol)}\b") for symbol in symbols}
    found: set[str] = set()
    if not pending:
        return found
    for root_name in ABSENT_ROOTS:
        root = ROOT / root_name
        if not root.is_dir():
            continue
        for path in root.rglob("*.rs"):
            if not pending:
                return found
            try:
                src = path.read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue
            for symbol in list(pending):
                if pending[symbol].search(src):
                    found.add(symbol)
                    del pending[symbol]
    return found


def collect() -> tuple[list[Section], list[Finding]]:
    specs = sorted(SPECS_DIR.glob("*.md"))
    sections: list[Section] = []
    findings: list[Finding] = []
    for spec in specs:
        spec_sections, spec_findings = parse_spec(spec)
        sections.extend(spec_sections)
        findings.extend(spec_findings)

    absent_symbols: set[str] = set()
    parsed: list[tuple[Section, list[str]]] = []
    for section in sections:
        anchors = (
            [part.strip() for part in section.anchor.split("·") if part.strip()]
            if section.anchor
            else []
        )
        parsed.append((section, anchors))
        for anchor in anchors:
            if anchor.startswith("absent "):
                absent_symbols.add(anchor.split(" ", 1)[1].strip())

    present = scan_absent(absent_symbols)

    for section, anchors in parsed:
        for anchor in anchors:
            state, note = resolve_anchor(anchor)
            if state == "ABSENT":
                if anchor.split(" ", 1)[1].strip() in present:
                    state, note = "ABSENT-BUT-PRESENT", anchor
                else:
                    state, note = "OK", anchor
            section.resolved.append((anchor, state if state == "OK" else f"{state}: {note}"))
            if state != "OK":
                findings.append(
                    Finding("violation", section.spec, section.anchor_line, anchor, note)
                )
            elif anchor.startswith("planned ") and section.ratified:
                findings.append(
                    Finding(
                        "violation",
                        section.spec,
                        section.anchor_line,
                        anchor,
                        "planned anchor on a Ratified/Accepted/Definitive section",
                    )
                )

    for section in sections:
        if section.anchor is None and section.ratified:
            findings.append(
                Finding(
                    "coverage",
                    section.spec,
                    section.line,
                    section.title,
                    f"Ratified/Accepted section without an Anchor line (status: {section.status})",
                )
            )

    return sections, findings


def frontmatter_scalar(raw: str) -> str | None:
    """One-line plain or quoted YAML scalar with its comment stripped, else None."""
    if raw and not raw[0].isspace():
        return None  # `status:x` is a plain scalar key, not a mapping entry
    value = raw.strip()
    if value[:1] in ("'", '"'):
        close = value.find(value[0], 1)
        if close < 0 or value[close + 1 :].strip()[:1] not in ("", "#"):
            return None
        return value[1:close].strip() or None
    value = re.split(r"\s#", value, maxsplit=1)[0].strip()
    if not value or value[0] in "[{|>&*!#%@`":
        return None
    return value


def plan_status(path: Path, rel: str) -> tuple[str | None, list[Finding]]:
    """A plan's frontmatter `status:` plus structural findings; None when absent."""
    lines = read_text(path).splitlines()
    if not lines or lines[0].strip() != "---":
        return None, []
    end = next((i for i in range(1, len(lines)) if lines[i].strip() == "---"), None)
    if end is None:
        return None, [
            Finding("structural", rel, 1, "frontmatter", "opening '---' is never closed")
        ]

    entries: list[tuple[int, str]] = []
    for index in range(1, end):
        match = FRONTMATTER_STATUS_RE.match(lines[index])
        if match:
            entries.append((index + 1, match.group(1)))

    findings: list[Finding] = []
    status: str | None = None
    for position, (line, raw) in enumerate(entries):
        if position:
            findings.append(
                Finding(
                    "structural",
                    rel,
                    line,
                    "status",
                    f"duplicate frontmatter status (first at line {entries[0][0]})",
                )
            )
        value = frontmatter_scalar(raw)
        if value is None:
            findings.append(
                Finding(
                    "structural",
                    rel,
                    line,
                    "status",
                    f"malformed frontmatter status '{raw.strip()}' (expected a one-line scalar)",
                )
            )
        elif not position:
            status = value
    return status, findings


def doc_links(path: Path) -> list[tuple[int, str]]:
    """Inline Markdown link targets outside fenced and inline code, as (line, target)."""
    links: list[tuple[int, str]] = []
    in_fence = False
    for number, line in enumerate(read_text(path).splitlines(), 1):
        if FENCE_RE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        for match in LINK_RE.finditer(CODE_SPAN_RE.sub("", line)):
            links.append((number, match.group(1).strip("<>").strip()))
    return links


def local_target(doc: Path, target: str, root: Path) -> Path | None:
    """Filesystem path a link names; None for external or anchor-only links."""
    if target.startswith(("#", "//")) or SCHEME_RE.match(target):
        return None
    local = unquote(target.split("#", 1)[0].split("?", 1)[0])
    if not local:
        return None
    if local.startswith("/"):
        return root / local.lstrip("/")
    return doc.parent / local


def linked_plan(target: Path, root: Path) -> str | None:
    """Repo-relative plan.md a link names directly or through its plan directory."""
    plan = (target / "plan.md" if target.is_dir() else target).resolve()
    agents = (root / ".agents").resolve()
    if plan.name != "plan.md" or plan.parent.parent != agents or not plan.is_file():
        return None
    return plan.relative_to(root.resolve()).as_posix()


def collect_structure(
    root: Path,
) -> tuple[list[Finding], list[tuple[str, str | None, list[str]]]]:
    """Structural plan-status/link findings plus (plan, status, linking sites) claims."""
    findings: list[Finding] = []
    statuses: dict[str, str | None] = {}
    for plan in sorted(root.glob(PLAN_GLOB)):
        rel = plan.relative_to(root).as_posix()
        statuses[rel], plan_findings = plan_status(plan, rel)
        findings.extend(plan_findings)

    linked: dict[str, list[str]] = {}
    for doc_rel in CLAIM_DOCS:
        doc = root / doc_rel
        if not doc.is_file():
            findings.append(Finding("structural", doc_rel, 0, doc_rel, "claim document missing"))
            continue
        for number, target in doc_links(doc):
            path = local_target(doc, target, root)
            if path is None:
                continue
            if not path.exists():
                findings.append(
                    Finding("structural", doc_rel, number, target, "link target does not exist")
                )
                continue
            plan = linked_plan(path, root)
            if plan is None:
                continue
            sites = linked.setdefault(plan, [])
            site = f"{doc_rel}:{number}"
            if site not in sites:
                sites.append(site)

    claims = [(plan, statuses.get(plan), sites) for plan, sites in sorted(linked.items())]
    return findings, claims



def render_status(
    sections: list[Section],
    findings: list[Finding],
    claims: list[tuple[str, str | None, list[str]]],
) -> str:
    anchored = [s for s in sections if s.anchor]
    ratified = [s for s in sections if s.ratified]
    ratified_anchored = [s for s in ratified if s.anchor]
    violations = [f for f in findings if f.severity == "violation"]
    coverage = [f for f in findings if f.severity == "coverage"]
    prose = [f for f in findings if f.severity == "prose"]
    structural = [f for f in findings if f.severity == "structural"]

    out: list[str] = []
    out.append("# Generated spec anchor status")
    out.append("")
    out.append("<!-- Generated by scripts/check-spec-anchors.py; do not edit by hand. -->")
    out.append("")
    out.append(
        "Layer 3 of [Spec 21](specs/21-documentation-architecture.md). "
        "`Anchor:` lines in `docs/specs/` are resolved against the tree; this file "
        "is the status home specs link to instead of carrying status prose."
    )
    out.append("")
    out.append("| Metric | Value |")
    out.append("|---|---:|")
    out.append(f"| Sections scanned | {len(sections)} |")
    out.append(f"| Sections with an anchor | {len(anchored)} |")
    out.append(f"| Ratified/Accepted sections | {len(ratified)} |")
    out.append(f"| …of those, anchored | {len(ratified_anchored)} |")
    out.append(f"| Anchor violations | {len(violations)} |")
    out.append(f"| Coverage gaps (ratified, unanchored) | {len(coverage)} |")
    out.append(f"| Status-prose hits | {len(prose)} |")
    out.append(f"| Plans linked from status docs | {len(claims)} |")
    out.append(f"| Structural errors (plan frontmatter, doc links) | {len(structural)} |")
    out.append("")
    out.append("## Anchored sections")
    out.append("")
    out.append("| Spec | Section | Anchor | State |")
    out.append("|---|---|---|---|")
    for section in anchored:
        for anchor, state in section.resolved:
            out.append(
                f"| `{section.spec}` | {section.title} | `{anchor}` | {state} |"
            )
    if not anchored:
        out.append("| — | — | — | — |")
    out.append("")
    out.append("## Plan status claims")
    out.append("")
    out.append(
        "Plans linked from "
        + ", ".join(f"`{doc}`" for doc in CLAIM_DOCS)
        + " with each plan's own frontmatter `status:`. Report-only: the linking "
        "prose may describe scheduling, evidence or implementation state, so the "
        "checker neither equates these statuses nor rewrites either side."
    )
    out.append("")
    out.append("| Plan | Frontmatter status | Linked from |")
    out.append("|---|---|---|")
    for plan, status, sites in claims:
        shown = f"`{status}`".replace("|", "\\|") if status else "—"
        linked = ", ".join(f"`{site}`" for site in sites)
        out.append(f"| `{plan}` | {shown} | {linked} |")
    if not claims:
        out.append("| — | — | — |")
    out.append("")
    out.append("## Open findings")
    out.append("")
    if structural:
        out.append("### Structural errors (fail `--check`)")
        out.append("")
        for finding in structural:
            out.append(
                f"- `{finding.spec}:{finding.line}` — {finding.subject}: {finding.detail}"
            )
        out.append("")
    if violations:
        out.append("### Anchor violations")
        out.append("")
        for finding in violations:
            out.append(
                f"- `{finding.spec}:{finding.line}` — {finding.subject}: {finding.detail}"
            )
        out.append("")
    if coverage:
        out.append("### Coverage gaps (Spec 21 rollout step 2 backfill)")
        out.append("")
        for finding in coverage:
            out.append(f"- `{finding.spec}:{finding.line}` — {finding.subject}")
        out.append("")
    if prose:
        out.append("### Status prose in specs (Spec 21 §2 Layer 3)")
        out.append("")
        for finding in prose[:60]:
            out.append(
                f"- `{finding.spec}:{finding.line}` — `{finding.subject}` in: {finding.detail}"
            )
        if len(prose) > 60:
            out.append(f"- … {len(prose) - 60} more")
        out.append("")
    if not (violations or coverage or prose or structural):
        out.append("None.")
        out.append("")
    return "\n".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="do not write the status file; fail when it is stale or on structural errors",
    )
    parser.add_argument(
        "--check-structure",
        action="store_true",
        help="do not write the status file; fail only on structural errors",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="fail on anchor violations and ratified coverage gaps (rollout step 3)",
    )
    args = parser.parse_args()

    sections, findings = collect()
    structural, claims = collect_structure(ROOT)
    rendered = render_status(sections, findings + structural, claims)

    violations = [f for f in findings if f.severity == "violation"]
    coverage = [f for f in findings if f.severity == "coverage"]
    prose = [f for f in findings if f.severity == "prose"]

    print(
        "spec anchors: "
        f"{len([s for s in sections if s.anchor])}/{len(sections)} sections anchored, "
        f"{len(violations)} violation(s), {len(coverage)} coverage gap(s), "
        f"{len(prose)} status-prose hit(s)"
    )
    for finding in violations:
        print(f"  violation {finding.spec}:{finding.line} — {finding.subject}: {finding.detail}")
    print(
        f"plan structure: {len(claims)} linked plan(s), "
        f"{len(structural)} structural error(s)"
    )
    for finding in structural:
        print(f"  structural {finding.spec}:{finding.line} — {finding.subject}: {finding.detail}")

    stale = False
    if STATUS_FILE.is_file():
        stale = read_text(STATUS_FILE) != rendered
    else:
        stale = True

    failed = False
    if args.check or args.check_structure:
        if args.check:
            if stale:
                print(
                    f"FAIL: {STATUS_FILE.relative_to(ROOT)} is stale — "
                    "run python3 scripts/check-spec-anchors.py and commit the result"
                )
                failed = True
            else:
                print(f"OK: {STATUS_FILE.relative_to(ROOT)} is current")
        if structural:
            print(f"FAIL: {len(structural)} structural error(s) in plan frontmatter or doc links")
            failed = True
        else:
            print("OK: plan frontmatter and status-doc links are structurally valid")
    else:
        STATUS_FILE.write_text(rendered, encoding="utf-8")
        print(f"wrote {STATUS_FILE.relative_to(ROOT)} ({len(rendered.splitlines())} lines)")

    if args.strict and (violations or coverage):
        print("FAIL: strict mode (Spec 21 rollout step 3)")
        failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
