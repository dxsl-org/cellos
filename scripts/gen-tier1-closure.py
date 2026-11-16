#!/usr/bin/env python3
"""Generate `scripts/tier1-closure.generated.toml`: what Tier-1 cells link.

    python3 scripts/gen-tier1-closure.py           # rewrite the file
    python3 scripts/gen-tier1-closure.py --check   # fail on drift (CI)

Method:
  * `cargo metadata --locked --offline --all-features --format-version 1
    --filter-platform T` for each bare-metal target in TARGETS (the registry
    sources must already be in the Cargo cache: CI runs `cargo fetch --locked`
    first). `--all-features` makes the closure a superset of every shipped
    feature flavour (`service-net --features qemu-dev-https`, the relay's
    `tls-ca-*`, …), and cargo unifies features across the workspace on top;
  * roots: the workspace members under `cells/` whose manifest class leaves
    them in Tier 1, read by the signer's own rule (`tier_class.manifest_class`)
    with the `[[bin]]` targets this metadata reports;
  * edges: resolve-graph dependencies with a normal kind only (no dev, no
    build). A proc-macro runs on the host and is not followed further, but it
    writes code into the crates that call it, which their text scan does not
    see (`#[derive]` is all that is on disk): a third-party proc-macro is pinned
    by name, version and source in `[[proc_macro]]`, so a new or bumped one is a
    reviewed diff, and an in-tree one is a `[[package]]` whose files the
    baseline holds to ceilings, so `unsafe` spelt in its `quote!` output counts;
  * union over the targets.

Refusals (the generator exits non-zero):
  * a `cells/` crate reached from a Tier-1 root reads as Tier 2: the tier gate
    exempts it and the cells total leaves it out, so its `unsafe` would reach a
    Tier-1 cell unseen;
  * a closure crate has a build script and an `include!` whose argument is not
    a plain string literal (`concat!(env!("OUT_DIR"), …)`, `env!("GEN_RS")`, a
    macro variable: a computed path can name build output that is not on disk
    to count), while its build script or any build-dependency, in-tree or
    third-party (with their normal deps), spells a counted token (`unsafe`,
    `no_mangle`, `export_name`, `link_section`, `global_asm`) anywhere, string
    literals included — unless REVIEWED_GENERATED_INCLUDES records a review of
    that exact build script (sha256);
  * a Tier-1 cell outside the workspace and outside NON_MEMBER_CELLS.

Output: the Tier-1 roots, the in-tree non-cell packages they link (the signer
holds their files to `scripts/tcb-unsafe-baseline.toml`), every third-party
package pinned by name, version and full source string (a git dependency's
revision included) with its `unsafe` token and unsafe-attribute counts (same
lexer and rule as the signer), the third-party proc-macros, and the `[[bin]]`
targets of every `cells/` workspace member, which the signer uses to find
crate roots and to attribute a target ELF to its crate. A new crate, a version
or revision bump, a changed count or a new binary target is a diff of this
file, and `--check` fails until it is regenerated and committed.

Cell crates that are not workspace members are not resolved by cargo; each
must be listed in NON_MEMBER_CELLS with the reason, or the generator fails.
"""

from __future__ import annotations

import argparse
import difflib
import functools
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from cellos_sign import policy, scan, tcb, tier_class  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
# The AArch64 cells build for the soft-float target (scripts/build-aarch64-*.sh).
TARGETS = ("x86_64-unknown-none", "riscv64gc-unknown-none-elf", "aarch64-unknown-none-softfloat")

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

# `include!` with any argument: the argument is a literal exactly when it reduces
# to whitespace once strings are blanked (`include!("x.rs")` → `include!( )`).
_INCLUDE = re.compile(r"\binclude\s*!\s*([(\[{])")
_COUNTED_WORD = re.compile(r"\b(?:unsafe|no_mangle|export_name|link_section|global_asm)\b")

# Closure crates whose build inputs spell a counted token although the code they
# `include!` from the build script cannot hold one: name@version → (sha256 of the
# build script as reviewed, what was checked). An edit of the build script, a new
# version, or an entry that no longer matches a flagged crate fails the generator.
REVIEWED_GENERATED_INCLUDES: dict[str, tuple[str, str]] = {
    "service-silo@0.1.0": (
        "10e1f7a08eb23c0a8f6355c10d44b889d1bdcc6a67bf15e2909e2a1952aa1395",
        "P18 re-review 2026-11-16: build.rs writes silo-guest.bin (include_bytes!, data) and "
        "one generated line, `pub const GUEST_SHA256: [u8; 32] = {digest:?};`, formatted from "
        "a [u8; 32]; build.rs spells no counted token. The hits are the host-side SIMD code of "
        "its build-dependency sha2, which computes the digest and emits nothing.",
    ),
}


def metadata(target: str) -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--all-features", "--format-version", "1",
         "--filter-platform", target],
        cwd=REPO, capture_output=True, text=True,
    )
    if out.returncode != 0:
        sys.exit(f"cargo metadata --filter-platform {target} failed "
                 f"(run `cargo fetch --locked` first):\n{out.stderr}")
    return json.loads(out.stdout)


def _q(text: str) -> str:
    """A TOML basic string (JSON's escaping is valid TOML)."""
    return json.dumps(text)


def _rel(path: Path, repo: Path) -> str | None:
    path = path.resolve()
    return path.relative_to(repo).as_posix() if path.is_relative_to(repo) else None


def registry_counts(package_dir: Path) -> tcb.Counts:
    files = [p for p in package_dir.rglob("*.rs")]
    total = tcb.Counts()
    for path in tcb.linked_sources(package_dir, files):
        total = total + tcb.count(scan.read_code(path))
    return total


@functools.cache
def _nonliteral_includes(package_dir: Path) -> tuple[str, ...]:
    """`file: argument` for every `include!` in the package's linked sources whose
    argument is not a plain string literal (`concat!(env!("OUT_DIR"), …)`,
    `env!("GEN_RS")`, a macro variable): its path is computed, so it can name
    build-script output that is not on disk to count."""
    hits = []
    for path in tcb.linked_sources(package_dir, list(package_dir.rglob("*.rs"))):
        code = scan.read_code(path)
        for match in _INCLUDE.finditer(code):
            argument = tier_class._arguments(code, match.start(1)).strip().rstrip(",").strip()
            if argument:
                hits.append(f"{path.name}: include!({' '.join(argument.split())})")
    return tuple(hits)


def _spelt_tokens(files: list[Path]) -> list[str]:
    """`path: word` for every counted word in the raw text (literals included)."""
    hits = []
    for path in files:
        raw = path.read_text(encoding="utf-8", errors="replace")
        hits += [f"{path}: {m.group(0)}" for m in _COUNTED_WORD.finditer(raw)]
    return hits


def build() -> str:
    repo = REPO.resolve()
    metas = {target: metadata(target) for target in TARGETS}

    # ── Binary targets of every cells/ workspace member (union over targets) ──
    bins: dict[str, dict[str, str]] = {}
    bin_package: dict[str, str] = {}
    member_cell_dirs: set[str] = set()
    for meta in metas.values():
        packages = {p["id"]: p for p in meta["packages"]}
        for pid in meta["workspace_members"]:
            pkg = packages[pid]
            directory = _rel(Path(pkg["manifest_path"]).parent, repo)
            if not directory or not directory.startswith("cells/"):
                continue
            member_cell_dirs.add(directory)
            bin_package[directory] = pkg["name"]
            for target in pkg["targets"]:
                if "bin" in target["kind"]:
                    src = _rel(Path(target["src_path"]), repo)
                    if src:
                        bins.setdefault(directory, {})[target["name"]] = src

    # ── Every cells/ crate's tier, read by the signer's rule ──────────────────
    sources = scan.tracked_sources(repo, policy.CELL_ROOTS)
    tracked = {(repo / rel).resolve() for rel in sources}
    tier_of: dict[str, int] = {}
    cells: dict[str, str] = {}
    for crate in scan.discover_crates(repo, policy.CELL_ROOTS, bins):
        if not any(root.resolve() in tracked for root in crate.roots):
            continue
        directory = crate.directory.relative_to(repo).as_posix()
        tier_of[directory] = 2 if tier_class.is_tier2(tier_class.manifest_class(crate, tracked)) else 1
        if tier_of[directory] == 1:
            cells[crate.name] = directory

    roots_found: set[str] = set()
    in_tree: dict[str, tuple[str, str | None]] = {}  # name → (directory, kind)
    third: dict[tuple[str, str, str], Path] = {}
    macros: set[tuple[str, str, str]] = set()
    errors: list[str] = []
    # label → (non-literal includes, build script files, all build inputs)
    generated_includes: dict[str, tuple[tuple[str, ...], list[Path], list[Path]]] = {}

    for target, meta in metas.items():
        packages = {p["id"]: p for p in meta["packages"]}
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

        def directory_of(pkg: dict) -> str | None:
            return _rel(Path(pkg["manifest_path"]).parent, repo)

        def build_inputs(pid: str) -> tuple[list[Path], list[Path]]:
            """(the package's build script files, those plus the linked sources of
            every build-dependency, in-tree or third-party, and of their normal deps)."""
            pkg = packages[pid]
            scripts: list[Path] = []
            for build_target in pkg["targets"]:
                if "custom-build" in build_target["kind"]:
                    script = Path(build_target["src_path"])
                    # A build script in its own directory may have modules there.
                    root = Path(pkg["manifest_path"]).parent
                    scripts += [script] if script.parent == root else sorted(
                        script.parent.rglob("*.rs"))
            files = list(scripts)
            stack = [d["pkg"] for d in nodes[pid]["deps"]
                     if any(k["kind"] == "build" for k in d["dep_kinds"])]
            seen: set[str] = set()
            while stack:
                dep = stack.pop()
                if dep in seen:
                    continue
                seen.add(dep)
                dep_dir = Path(packages[dep]["manifest_path"]).parent
                files += tcb.linked_sources(dep_dir, list(dep_dir.rglob("*.rs")))
                stack += [d["pkg"] for d in nodes[dep]["deps"]
                          if any(k["kind"] is None for k in d["dep_kinds"])]
            return scripts, files

        stack = []
        root_ids = set()
        for pid in meta["workspace_members"]:
            pkg = packages[pid]
            directory = directory_of(pkg)
            if directory and cells.get(pkg["name"]) == directory:
                stack.append(pid)
                root_ids.add(pid)
                roots_found.add(pkg["name"])
        seen: set[str] = set()
        while stack:
            pid = stack.pop()
            if pid in seen:
                continue
            seen.add(pid)
            pkg = packages[pid]
            directory = directory_of(pkg)
            local = pkg["source"] is None and directory is not None
            if any("proc-macro" in t["kind"] for t in pkg["targets"]):
                if local:
                    in_tree[pkg["name"]] = (directory, "proc-macro")
                else:
                    macros.add((pkg["name"], pkg["version"], pkg["source"] or "path"))
                continue
            if local and directory.startswith("cells/"):
                if pid not in root_ids and tier_of.get(directory) == 2:
                    errors.append(f"{directory} ({pkg['name']}) reads as Tier 2 but is linked into "
                                  f"a Tier-1 cell on {target}: the tier gate exempts it and its "
                                  "unsafe is counted nowhere")
            elif local:
                in_tree[pkg["name"]] = (directory, None)
            else:
                source = pkg["source"] or f"path+{Path(pkg['manifest_path']).parent}"
                third[(pkg["name"], pkg["version"], source)] = Path(pkg["manifest_path"]).parent
            package_dir = Path(pkg["manifest_path"]).parent
            has_build_script = any("custom-build" in t["kind"] for t in pkg["targets"])
            if has_build_script and _nonliteral_includes(package_dir):
                generated_includes[f"{pkg['name']}@{pkg['version']}"] = (
                    _nonliteral_includes(package_dir), *build_inputs(pid))
            for dep in nodes[pid]["deps"]:
                if any(k["kind"] is None for k in dep["dep_kinds"]):
                    stack.append(dep["pkg"])

    needed: set[str] = set()
    for label, (includes, scripts, inputs) in sorted(generated_includes.items()):
        spelt = _spelt_tokens(inputs)
        if not spelt:
            continue
        needed.add(label)
        reviewed = REVIEWED_GENERATED_INCLUDES.get(label)
        digest = hashlib.sha256(b"".join(p.read_bytes() for p in scripts)).hexdigest()
        if reviewed is None:
            errors.append(f"{label} has a build script and include!s a computed path "
                          f"({includes[0]}), and its build inputs spell counted tokens the "
                          f"scan cannot see in generated code: {spelt[:5]} — make the include a "
                          "literal path, or review it into REVIEWED_GENERATED_INCLUDES")
        elif reviewed[0] != digest:
            errors.append(f"{label}: its build script changed since the review in "
                          f"REVIEWED_GENERATED_INCLUDES (sha256 now {digest}) — re-review it")
    for label in sorted(set(REVIEWED_GENERATED_INCLUDES) - needed):
        errors.append(f"REVIEWED_GENERATED_INCLUDES lists {label}, which no longer needs it")

    unresolved = dict(NON_MEMBER_CELLS)
    for name, directory in sorted(cells.items()):
        if name not in roots_found and directory not in unresolved:
            errors.append(f"Tier-1 cell {name} ({directory}) is not a workspace member and not in "
                          "NON_MEMBER_CELLS — add it to the workspace or record why it is outside")
    for directory in sorted(unresolved):
        if directory in member_cell_dirs or not (repo / directory / "Cargo.toml").is_file():
            errors.append(f"NON_MEMBER_CELLS lists {directory}, which is a workspace member or gone")
    if errors:
        sys.exit("gen-tier1-closure: refused:\n  " + "\n  ".join(sorted(set(errors))))

    lines = [
        "# GENERATED by scripts/gen-tier1-closure.py — do not edit by hand.",
        "# Regenerate after a dependency, manifest-class, binary-target or Cargo.lock",
        "# change; CI (f1-admission) fails on drift. Read by cellos-sign",
        "# (scripts/cellos_sign/tcb.py, scan.py, tier_class.py).",
        "#",
        "# tier1_roots: cell crates whose manifest class leaves them in Tier 1.",
        "# [[package]]: in-tree non-cell packages they link (kind = \"proc-macro\": an",
        "#              in-tree proc-macro whose output they contain); files ceilinged",
        "#              in scripts/tcb-unsafe-baseline.toml.",
        "# [[thirdparty]]: pins by name, version and full source, with `unsafe` tokens and",
        "#              unsafe attributes (no_mangle / export_name / link_section /",
        "#              global_asm); any change is a diff here.",
        "# [[proc_macro]]: third-party proc-macros that write code into them, pinned alike.",
        "# [[bin]]: binary targets of every cells/ workspace member (crate roots; the",
        "#              signer attributes a target ELF to its crate by this name).",
        "# [[unresolved]]: cell crates outside the Cargo workspace, with the reason.",
        "",
        "version = 1",
        "targets = [" + ", ".join(_q(t) for t in TARGETS) + "]",
        "tier1_roots = [",
        *[f"    {_q(name)}," for name in sorted(roots_found)],
        "]",
    ]
    for name, (directory, kind) in sorted(in_tree.items()):
        lines += ["", "[[package]]", f"name = {_q(name)}", f"path = {_q(directory)}"]
        if kind:
            lines.append(f"kind = {_q(kind)}")
    for (name, version, source), directory in sorted(third.items()):
        counts = registry_counts(directory)
        lines += ["", "[[thirdparty]]", f"name = {_q(name)}", f"version = {_q(version)}",
                  f"source = {_q(source)}", f"unsafe = {counts.unsafe}", f"attrs = {counts.attrs}"]
    for name, version, source in sorted(macros):
        lines += ["", "[[proc_macro]]", f"name = {_q(name)}", f"version = {_q(version)}",
                  f"source = {_q(source)}"]
    for directory in sorted(bins):
        for name, src in sorted(bins[directory].items()):
            lines += ["", "[[bin]]", f"name = {_q(name)}", f"package = {_q(bin_package[directory])}",
                      f"dir = {_q(directory)}", f"src = {_q(src)}"]
    for directory, reason in sorted(unresolved.items()):
        lines += ["", "[[unresolved]]", f"path = {_q(directory)}", f"reason = {_q(reason)}"]
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
