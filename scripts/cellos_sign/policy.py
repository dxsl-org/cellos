"""The F1 policy check: attribute and token layers, tier class, Tier-1 closure.

This is the *only* implementation of the "no unsafe in cells/" rule in the repo.
It replaced `scripts/check-cells-unsafe-ratchet.py`, whose token rule and 49-entry
Python set were absorbed into `scripts/unsafe-allowlist.toml` verbatim; keeping
two scanners would have let CI enforce two rules that drift apart.

Layers, and why both are needed:
  * attribute — `#![forbid(unsafe_code)]` on every crate root makes rustc the
    enforcer for code the token scan cannot judge on its own: it fails the
    *build* rather than a script, and it cannot be silenced by an
    `#[allow(unsafe_code)]` further down the crate;
  * token — a raw keyword scan over every tracked `.rs` file catches unsafe in
    files that are *not* in the module graph, which rustc never compiles and
    therefore never lints, and catches a crate whose attribute was removed in
    the same commit as the unsafe was added.

Neither layer subsumes the other, so a violation in either fails the check.

What `forbid` does NOT cover, so that nobody plans around a guarantee that is
not there (both verified against the pinned toolchain):
  * `unsafe` produced by expanding a macro defined in *another* crate compiles
    cleanly inside a forbidding crate — the lint is checked against the macro's
    definition site. `ostd::cell_main!` relies on exactly this;
  * `forbid` is per-crate, so it says nothing about a dependency, path or
    registry.

The boundary this check therefore enforces: **Cells are forbid-clean, and the
code they link is held to reviewed ceilings.** `libs/ostd` and the other
in-tree crates of the Tier-1 dependency closure are not Cells — their job is
the `unsafe` a Cell must not write — so they are not in CELL_ROOTS. Instead the
closure layer (`tcb.py`) holds every one of their files to a per-file ceiling
of `unsafe` tokens and unsafe attributes, plus a combined total over the Tier-1
cells and the closure. Third-party crates are pinned by name, version and
full source string in the generated closure file, whose drift CI checks.

The tier-class layer (`tier_class.py`) refuses a crate whose allowlist entries
include `c-ffi` or `legacy-raw-ptr` unless its manifest class puts it in Tier 2
or a reviewed `[[tier_exception]]` names it; before signing, the CLI repeats
that rule on each target ELF's own manifest (`tier_class.elf_gate`).

Strictness. `strict` makes F5-unverifiable, stale allowlist entries and the
tightening findings (a ceiling above the tree, an unused tier exception, a
closure file that misses or over-lists a Tier-1 root) fatal. `signing`
(`--sign`, which is always strict) keeps the stale entries fatal but reports
the tightening findings as NOTEs: they never admit more `unsafe` than the
reviewed ceilings already do, and CI's `--check --strict` fails on them.
"""

from __future__ import annotations

import datetime as _dt
from dataclasses import dataclass, field
from pathlib import Path

from . import scan, tcb, tier_class
from .allowlist import Allowlist

# Directories whose crates are Cells subject to F1.
CELL_ROOTS = ["cells"]
# Classes a file declaring an `extern { … }` block may carry: `c-ffi`, which the
# tier-class gate reads, or a deliberate test.
EXTERN_BLOCK_CLASSES = frozenset({"c-ffi", "test-harness", "test-violation"})


@dataclass(frozen=True)
class Violation:
    """One F1 failure, addressed to the developer who must fix it."""

    layer: str  # "attribute" | "token" | "tier-class" | "tcb" | "stale" (strict only)
    crate: str
    path: str
    detail: str

    def __str__(self) -> str:
        return f"[{self.layer}] {self.crate} :: {self.path} — {self.detail}"


@dataclass
class Result:
    violations: list[Violation] = field(default_factory=list)
    crates_scanned: int = 0
    files_scanned: int = 0
    unsafe_files: list[str] = field(default_factory=list)
    unused_file_entries: list[str] = field(default_factory=list)
    unused_crate_entries: list[str] = field(default_factory=list)
    stale_entries: list[str] = field(default_factory=list)
    tier: tier_class.Outcome | None = None
    tcb: tcb.Report | None = None
    # NOTEs from the closure layer that are violations only under --check --strict.
    tcb_notes: list[str] = field(default_factory=list)
    crates: list[scan.Crate] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.violations


def _crate_of(path: str, owners: list[tuple[str, str]]) -> str:
    """Name the crate owning `path`; owners are (dir, name) longest-first."""
    for directory, name in owners:
        if path == directory or path.startswith(directory + "/"):
            return name
    return "<no crate>"


def check(
    repo: Path,
    allow: Allowlist,
    today: _dt.date | None = None,
    strict: bool = False,
    closure: tcb.Closure | None = None,
    baseline: tcb.Baseline | None = None,
    extra: dict[str, str] | None = None,
    signing: bool = False,
) -> Result:
    """Run both F1 layers, the tier-class gate and the Tier-1 closure ratchet.

    Pure source parsing — no build, no network. The closure layer runs when
    `closure` and `baseline` are given; the CLI always loads and passes them
    (`tcb.load_closure` / `tcb.load_baseline`), so a signing run cannot skip
    it. `extra` appends code to repo-relative closure files without writing
    them (negative controls only). `signing` demotes the tightening findings
    to NOTEs (module docstring).
    """
    tighten = strict and not signing
    today = today or _dt.date.today()
    repo = repo.resolve()
    result = Result()

    # Both layers scan exactly the git-tracked set. Not a convenience: CI checks
    # out only tracked files, so a filesystem walk would enforce a stricter rule
    # locally than in CI — a half-written crate would fail a developer's build
    # before it was ever committed, and the two verdicts would diverge.
    sources = scan.tracked_sources(repo, CELL_ROOTS)
    tracked = {(repo / rel).resolve() for rel in sources}

    crates = [
        crate
        for crate in scan.discover_crates(repo, CELL_ROOTS, closure.bins if closure else None)
        if any(root.resolve() in tracked for root in crate.roots)
    ]
    result.crates = crates
    result.crates_scanned = len(crates)
    owners = sorted(
        ((c.directory.relative_to(repo).as_posix(), c.name) for c in crates),
        key=lambda pair: len(pair[0]),
        reverse=True,
    )

    # ── Layer 1: attribute ────────────────────────────────────────────────────
    used_crate_entries: set[str] = set()
    for crate in crates:
        if crate.name in allow.crates:
            used_crate_entries.add(crate.name)
            continue
        for root in crate.roots:
            if root.resolve() not in tracked:
                continue  # untracked root: not in the CI checkout, not our verdict
            if not scan.has_forbid(scan.read_code(root)):
                result.violations.append(
                    Violation(
                        layer="attribute",
                        crate=crate.name,
                        path=root.relative_to(repo).as_posix(),
                        detail="crate root lacks #![forbid(unsafe_code)] and the crate "
                        "is not in unsafe-allowlist.toml",
                    )
                )

    # ── Layer 2: token ────────────────────────────────────────────────────────
    result.files_scanned = len(sources)
    seen_unsafe: set[str] = set()
    cell_counts: dict[str, tcb.Counts] = {}
    for rel in sources:
        posix = rel.as_posix()
        absolute = repo / rel
        if not absolute.is_file():
            continue  # tracked but deleted in the working tree
        code = scan.read_code(absolute)
        counts = tcb.count(code)
        if counts:
            cell_counts[posix] = counts
        # A C import must carry the class the tier gate reads (or a test class).
        if scan.has_extern_block(code):
            entry = allow.files.get(posix)
            if entry is None or entry.klass not in EXTERN_BLOCK_CLASSES:
                found = f"class `{entry.klass}`" if entry else "no [[file]] entry"
                result.violations.append(Violation(
                    layer="tier-class", crate=_crate_of(posix, owners), path=posix,
                    detail=f"declares an `extern {{ … }}` block (a C-ABI import) but has "
                    f"{found}; it needs class `c-ffi` (or `test-harness` / "
                    "`test-violation` for a deliberate test)",
                ))
        if counts.unsafe == 0:
            continue
        seen_unsafe.add(posix)
        if posix not in allow.files:
            result.violations.append(
                Violation(
                    layer="token",
                    crate=_crate_of(posix, owners),
                    path=posix,
                    detail="contains the `unsafe` keyword and is not in "
                    "unsafe-allowlist.toml",
                )
            )
    result.unsafe_files = sorted(seen_unsafe)

    # ── Layer 3: tier class ───────────────────────────────────────────────────
    # A crate with a `c-ffi` or `legacy-raw-ptr` entry must declare a Tier-2
    # manifest class, or carry a reviewed [[tier_exception]] (tier_class.py).
    tier = tier_class.evaluate(repo, crates, tracked, allow, lambda p: _crate_of(p, owners))
    result.tier = tier
    for crate_name, _klass, detail in tier.mismatches:
        result.violations.append(
            Violation(
                layer="tier-class",
                crate=crate_name,
                path=tier.records[crate_name].directory,
                detail=detail,
            )
        )
    for crate_name, directories in sorted(tier.duplicates.items()):
        result.violations.append(Violation(
            layer="tier-class", crate=crate_name, path=", ".join(directories),
            detail="several crates under cells/ share this package name; the tier-class "
            "join and the ELF gate are keyed by it — rename one",
        ))
    if tighten:
        for label in tier.unused_exceptions:
            result.violations.append(
                Violation(
                    layer="tier-class",
                    crate=label.split(" ", 1)[0],
                    path="[[tier_exception]]",
                    detail=f"{label} covers no mismatch — delete or correct it",
                )
            )

    # ── Layer 4: Tier-1 closure ───────────────────────────────────────────────
    # The code Tier-1 cells link from outside cells/ is held to per-file
    # ceilings, and the Tier-1 total (cells plus closure) to one more (tcb.py).
    if closure is not None and baseline is not None:
        tier1 = {name: r.directory for name, r in tier.records.items() if r.tier == 1}
        cells_total = tcb.Counts()
        for posix, counts in cell_counts.items():
            owner = tier.records.get(_crate_of(posix, owners))
            if owner is None or owner.tier == 1:
                cells_total = cells_total + counts
        report = tcb.check(repo, closure, baseline, tier1, cells_total, extra)
        result.tcb = report
        for finding in report.findings:
            if finding.strict_only:
                result.tcb_notes.append(f"{finding.crate} :: {finding.path} — {finding.detail}")
                if not tighten:
                    continue
            result.violations.append(
                Violation(layer="tcb", crate=finding.crate, path=finding.path,
                          detail=finding.detail)
            )

    # ── Ratchet hygiene (reported, fatal only under --strict for stale entries —
    # the ratchet never loosens itself, but a shrinking list must be visible so
    # it can be tightened) ────
    result.unused_file_entries = sorted(set(allow.files) - seen_unsafe)
    result.unused_crate_entries = sorted(set(allow.crates) - used_crate_entries)
    for e in allow.stale(today):
        desc = (
            f"{e.key} (approved {e.date}, {e.age_days(today)}d ago"
            + (f", review_by {e.review_by}" if e.review_by else "")
            + (f", tracking {e.tracking}" if e.tracking else "")
            + ")"
        )
        result.stale_entries.append(desc)
        if strict:
            result.violations.append(
                Violation(
                    layer="stale",
                    # A [[crate]] key is already the crate name; a [[file]] key
                    # is a path owned by some crate.
                    crate=e.key if e.key in allow.crates else _crate_of(e.key, owners),
                    path=e.key,
                    detail=f"overdue for re-review: {desc}",
                )
            )
    for x in allow.stale_exceptions(today):
        desc = (
            f"[[tier_exception]] {x.label} (approved {x.entry.date}, review_by "
            f"{x.entry.review_by}"
            + (f", tracking {x.entry.tracking}" if x.entry.tracking else "")
            + ")"
        )
        result.stale_entries.append(desc)
        if strict:
            result.violations.append(
                Violation(
                    layer="stale",
                    crate=x.crate,
                    path="[[tier_exception]]",
                    detail=f"overdue for re-review: {desc}",
                )
            )
    return result
