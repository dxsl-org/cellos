"""The F1 policy check: attribute layer + token layer, over one allowlist.

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

The boundary this check therefore enforces: **Cells are forbid-clean; `libs/*`
is trusted TCB and out of F1 scope.** `libs/ostd` is deliberately absent from
CELL_ROOTS — it is the supervisor-side runtime whose whole job is the `unsafe`
a Cell must not write, and it is reviewed as TCB, not ratcheted as a Cell.
"""

from __future__ import annotations

import datetime as _dt
from dataclasses import dataclass, field
from pathlib import Path

from . import scan
from .allowlist import Allowlist

# Directories whose crates are Cells subject to F1.
CELL_ROOTS = ["cells"]


@dataclass(frozen=True)
class Violation:
    """One F1 failure, addressed to the developer who must fix it."""

    layer: str  # "attribute" | "token"
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

    @property
    def ok(self) -> bool:
        return not self.violations


def _crate_of(path: str, owners: list[tuple[str, str]]) -> str:
    """Name the crate owning `path`; owners are (dir, name) longest-first."""
    for directory, name in owners:
        if path == directory or path.startswith(directory + "/"):
            return name
    return "<no crate>"


def check(repo: Path, allow: Allowlist, today: _dt.date | None = None) -> Result:
    """Run both F1 layers over `cells/`. Pure source parsing — no build, no network."""
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
        for crate in scan.discover_crates(repo, CELL_ROOTS)
        if any(root.resolve() in tracked for root in crate.roots)
    ]
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
    for rel in sources:
        posix = rel.as_posix()
        absolute = repo / rel
        if not absolute.is_file():
            continue  # tracked but deleted in the working tree
        if scan.count_unsafe(scan.read_code(absolute)) == 0:
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

    # ── Ratchet hygiene (reported, never fatal — the ratchet never loosens
    # itself, but a shrinking list must be visible so it can be tightened) ────
    result.unused_file_entries = sorted(set(allow.files) - seen_unsafe)
    result.unused_crate_entries = sorted(set(allow.crates) - used_crate_entries)
    result.stale_entries = [
        f"{e.key} (approved {e.date}, {e.age_days(today)}d ago"
        + (f", review_by {e.review_by}" if e.review_by else "")
        + (f", tracking {e.tracking}" if e.tracking else "")
        + ")"
        for e in allow.stale(today)
    ]
    return result
