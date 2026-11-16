#!/usr/bin/env python3
"""Generate `scripts/tier1-closure.generated.toml`: what Tier-1 cells link.

    python3 scripts/gen-tier1-closure.py           # rewrite the file
    python3 scripts/gen-tier1-closure.py --check   # fail on drift (CI)

Method:
  * `cargo metadata --locked --offline --format-version 1 --filter-platform T`
    for each bare-metal target in TARGETS (the registry sources must already be
    in the Cargo cache: CI runs `cargo fetch --locked` first);
  * roots: the workspace members under `cells/` whose manifest class leaves
    them in Tier 1, read by the signer's own rule (`tier_class.manifest_class`);
  * edges: resolve-graph dependencies with a normal kind only (no dev, no
    build); traversal stops at proc-macro crates, which run on the host and
    whose output lands in the calling crate, where its own scan sees it;
  * union over the targets. `cargo metadata` unifies features across the
    workspace, so the closure is a superset of what any one cell links.

Output: the Tier-1 roots, the in-tree non-cell packages they link (the signer
holds their files to `scripts/tcb-unsafe-baseline.toml`), and every third-party
package as a `name@version` pin with its `unsafe` token and unsafe-attribute
counts (same lexer and rule as the signer). The pins are the review point for
third-party code: a new crate, a version bump or a changed count is a diff of
this file, and `--check` fails until it is regenerated and committed.

Cell crates that are not workspace members are not resolved by cargo; each
must be listed in NON_MEMBER_CELLS with the reason, or the generator fails.
"""

from __future__ import annotations

import argparse
import difflib
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from cellos_sign import policy, scan, tcb, tier_class  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
TARGETS = ("x86_64-unknown-none", "riscv64gc-unknown-none-elf", "aarch64-unknown-none")

# Cell crates outside the Cargo workspace, so outside `cargo metadata`.
NON_MEMBER_CELLS = {
    "cells/demos/std-smoke": "std cell built with a custom sysroot and target spec; "
    "links Rust std, outside this scan",
    "cells/guests/silo-guest": "bare-metal hypervisor guest, runs in its own VM, not a cell",
    "cells/services/budget-authority": "Spec 26 design skeleton (owner decision D1, "
    "2026-11-10): not wired, never packaged",
    "cells/services/placement-controller": "Spec 26 design skeleton (owner decision D1, "
    "2026-11-10): not wired, never packaged",
    "cells/services/workload-router": "Spec 26 design skeleton (owner decision D1, "
    "2026-11-10): not wired, never packaged",
}


def metadata(target: str) -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1",
         "--filter-platform", target],
        cwd=REPO, capture_output=True, text=True,
    )
    if out.returncode != 0:
        sys.exit(f"cargo metadata --filter-platform {target} failed "
                 f"(run `cargo fetch --locked` first):\n{out.stderr}")
    return json.loads(out.stdout)


def tier1_cells() -> dict[str, str]:
    """Tier-1 cell crates under `cells/` (name → repo-relative directory)."""
    repo = REPO.resolve()
    sources = scan.tracked_sources(repo, policy.CELL_ROOTS)
    tracked = {(repo / rel).resolve() for rel in sources}
    out = {}
    for crate in scan.discover_crates(repo, policy.CELL_ROOTS):
        if not any(root.resolve() in tracked for root in crate.roots):
            continue
        if not tier_class.is_tier2(tier_class.manifest_class(crate, tracked)):
            out[crate.name] = crate.directory.relative_to(repo).as_posix()
    return out


def registry_counts(package_dir: Path) -> tcb.Counts:
    files = [p for p in package_dir.rglob("*.rs")]
    total = tcb.Counts()
    for path in tcb.linked_sources(package_dir, files):
        total = total + tcb.count(scan.read_code(path))
    return total


def build() -> str:
    repo = REPO.resolve()
    cells = tier1_cells()
    roots_found: set[str] = set()
    member_cell_dirs: set[str] = set()
    in_tree: dict[str, str] = {}
    third: dict[tuple[str, str], tuple[str, Path]] = {}

    for target in TARGETS:
        meta = metadata(target)
        packages = {p["id"]: p for p in meta["packages"]}
        members = set(meta["workspace_members"])
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

        def rel_dir(pkg: dict) -> str | None:
            directory = Path(pkg["manifest_path"]).resolve().parent
            return directory.relative_to(repo).as_posix() if directory.is_relative_to(repo) else None

        stack = []
        for pid in members:
            directory = rel_dir(packages[pid])
            if directory and directory.startswith("cells/"):
                member_cell_dirs.add(directory)
                if cells.get(packages[pid]["name"]) == directory:
                    stack.append(pid)
                    roots_found.add(packages[pid]["name"])
        seen: set[str] = set()
        while stack:
            pid = stack.pop()
            if pid in seen:
                continue
            seen.add(pid)
            pkg = packages[pid]
            if any("proc-macro" in t["kind"] for t in pkg["targets"]):
                continue
            directory = rel_dir(pkg)
            if pkg["source"] is None and directory is not None:
                if not directory.startswith("cells/"):
                    in_tree[pkg["name"]] = directory
            else:
                kind = pkg["source"].split("+", 1)[0] if pkg["source"] else "path"
                third[(pkg["name"], pkg["version"])] = (
                    kind, Path(pkg["manifest_path"]).parent)
            for dep in nodes[pid]["deps"]:
                if any(k["kind"] is None for k in dep["dep_kinds"]):
                    stack.append(dep["pkg"])

    unresolved = {d: r for d, r in NON_MEMBER_CELLS.items()}
    for name, directory in sorted(cells.items()):
        if name not in roots_found and directory not in unresolved:
            sys.exit(f"Tier-1 cell {name} ({directory}) is not a workspace member and not in "
                     "NON_MEMBER_CELLS — add it to the workspace or record why it is outside")
    for directory in sorted(unresolved):
        if directory in member_cell_dirs or not (repo / directory / "Cargo.toml").is_file():
            sys.exit(f"NON_MEMBER_CELLS lists {directory}, which is a workspace member or gone")

    lines = [
        "# GENERATED by scripts/gen-tier1-closure.py — do not edit by hand.",
        "# Regenerate after a dependency, manifest-class or Cargo.lock change; CI",
        "# (f1-admission) fails on drift. Read by cellos-sign (scripts/cellos_sign/tcb.py).",
        "#",
        "# tier1_roots: cell crates whose manifest class leaves them in Tier 1.",
        "# [[package]]: in-tree non-cell packages they link; files ceilinged in",
        "#              scripts/tcb-unsafe-baseline.toml.",
        "# [[thirdparty]]: name@version pins with `unsafe` tokens and unsafe attributes",
        "#              (no_mangle / export_name / link_section); any change is a diff here.",
        "# [[unresolved]]: cell crates outside the Cargo workspace, with the reason.",
        "",
        "version = 1",
        "targets = [" + ", ".join(f'"{t}"' for t in TARGETS) + "]",
        "tier1_roots = [",
        *[f'    "{name}",' for name in sorted(roots_found)],
        "]",
    ]
    for name, directory in sorted(in_tree.items()):
        lines += ["", "[[package]]", f'name = "{name}"', f'path = "{directory}"']
    for (name, version), (kind, directory) in sorted(third.items()):
        counts = registry_counts(directory)
        lines += ["", "[[thirdparty]]", f'name = "{name}"', f'version = "{version}"',
                  f'source = "{kind}"', f"unsafe = {counts.unsafe}", f"attrs = {counts.attrs}"]
    for directory, reason in sorted(unresolved.items()):
        lines += ["", "[[unresolved]]", f'path = "{directory}"', f'reason = "{reason}"']
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true",
                        help="compare with the committed file instead of writing it")
    args = parser.parse_args()
    text = build()
    path = REPO / tcb.CLOSURE_PATH
    if args.check:
        current = path.read_text(encoding="utf-8") if path.is_file() else ""
        if current == text:
            print(f"tier-1 closure: OK ({tcb.CLOSURE_PATH} is current)")
            return 0
        sys.stdout.writelines(difflib.unified_diff(
            current.splitlines(keepends=True), text.splitlines(keepends=True),
            f"{tcb.CLOSURE_PATH} (committed)", f"{tcb.CLOSURE_PATH} (regenerated)"))
        print(f"FAIL: {tcb.CLOSURE_PATH} is stale — run scripts/gen-tier1-closure.py, "
              "review the diff (new crates, version bumps, changed counts) and commit it",
              file=sys.stderr)
        return 1
    path.write_text(text, encoding="utf-8")
    print(f"wrote {tcb.CLOSURE_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
