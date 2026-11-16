"""Source-level scanning primitives for the F1 (no-unsafe) policy check.

F1 (Spec 16 §6) says every Cell crate carries `#![forbid(unsafe_code)]`. This
module answers the questions the checker needs, using nothing but the source
tree — no cargo invocation, no network, no build:

  * does a crate root source carry the `#![forbid(unsafe_code)]` attribute?
  * which `.rs` files under a crate contain the `unsafe` keyword?
  * how many unsafe attributes (`no_mangle`, `export_name`, `link_section`) and
    `global_asm!` invocations does a file use? rustc's `unsafe_code` lint fires
    on them without the keyword;
  * does a file declare an `extern { … }` block (a C-ABI import)?

All answers are computed on text reduced to code by `lexer.strip_noncode`:
comments and string/char literals are gone, so neither the word "unsafe" in
prose nor a `/*` or a counterfeit attribute inside a string literal can steer
the verdict (Spec 18 §2.1: false positives are acceptable, false negatives are
not).
"""

from __future__ import annotations

import re
import subprocess
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from .lexer import strip_noncode

# `unsafe` as a word, but not the `unsafe_code` lint name.
UNSAFE_RE = re.compile(r"\bunsafe\b(?!_)")
# Attributes the `unsafe_code` lint treats as unsafe, bare, inside `unsafe(..)`
# or inside `cfg_attr(..)`, plus `global_asm!`, which the pinned rustc rejects
# under `forbid(unsafe_code)` ("usage of `core::arch::global_asm`") without the
# keyword. `asm!` is not here: it is only callable inside an `unsafe` block, which
# the keyword count already sees. Counted as words, so a macro matcher that
# names one counts too: a false positive, never a false negative.
UNSAFE_ATTR_RE = re.compile(r"\b(?:no_mangle|export_name|link_section|global_asm)\b")
# An `extern` block: `extern "C" {` reduces to `extern   {` once the ABI string
# is blanked, `unsafe extern "C" {` likewise. `extern crate` and `extern "C" fn`
# do not match.
EXTERN_BLOCK_RE = re.compile(r"\bextern\s*\{")
# Inner attribute `#![forbid(..., unsafe_code, ...)]`, whitespace-tolerant.
# Anchored to the start of a line: a crate-root attribute always begins one, and
# refusing to accept it mid-line keeps anything embedded in surrounding text from
# passing as the real attribute.
FORBID_RE = re.compile(
    r"^[ \t]*#!\s*\[\s*forbid\s*\([^)]*\bunsafe_code\b[^)]*\)\s*\]", re.MULTILINE
)


def read_code(path: Path) -> str:
    """Return the file's source reduced to code — no comments, no literals."""
    return strip_noncode(path.read_text(encoding="utf-8", errors="replace"))


def count_unsafe(code: str) -> int:
    """Number of `unsafe` keyword occurrences in reduced source."""
    return len(UNSAFE_RE.findall(code))


def count_unsafe_attrs(code: str) -> int:
    """Number of `no_mangle` / `export_name` / `link_section` / `global_asm` uses."""
    return len(UNSAFE_ATTR_RE.findall(code))


def has_extern_block(code: str) -> bool:
    """True if reduced source declares an `extern { … }` block."""
    return EXTERN_BLOCK_RE.search(code) is not None


def has_forbid(code: str) -> bool:
    """True if reduced source carries `#![forbid(unsafe_code)]`."""
    return FORBID_RE.search(code) is not None


def tracked_sources(repo: Path, roots: list[str]) -> list[Path]:
    """Every git-tracked `.rs` file under `roots`, as repo-relative paths.

    The token scan deliberately uses git's index rather than a filesystem walk:
    an untracked scratch file cannot fail CI, and a deleted-but-not-committed
    file cannot silently pass. Falls back to a filesystem walk outside a git
    checkout (release tarballs), which is a superset and therefore never weaker.
    """
    try:
        out = subprocess.run(
            ["git", "-C", str(repo), "ls-files", "-z", "--", *[f"{r}/**/*.rs" for r in roots],
             *[f"{r}/*.rs" for r in roots]],
            capture_output=True, text=True, check=True,
        ).stdout
        paths = [Path(p) for p in out.split("\0") if p]
        if paths:
            return sorted(set(paths))
    except (OSError, subprocess.CalledProcessError):
        pass
    walked: list[Path] = []
    for rel in roots:
        for src in (repo / rel).rglob("*.rs"):
            if "target" not in src.relative_to(repo).parts:
                walked.append(src.relative_to(repo))
    return sorted(set(walked))


@dataclass
class Crate:
    """One Cargo package discovered on disk.

    `roots` are the crate-root sources (bin/lib entry points) that must carry the
    F1 attribute: `lib` plus every value of `bins`. `bins` maps each binary
    target's name — the file name of the ELF cargo builds — to its root source.
    `sources` is every `.rs` file the crate owns, including files outside the
    module graph — those are exactly what a token scan must catch.
    """

    name: str
    directory: Path
    lib: Path | None = None
    bins: dict[str, Path] = field(default_factory=dict)
    sources: list[Path] = field(default_factory=list)

    @property
    def roots(self) -> list[Path]:
        roots = ([self.lib] if self.lib else []) + list(self.bins.values())
        return list(dict.fromkeys(roots))


def _lib_root(manifest: dict, base: Path) -> Path | None:
    path = base / manifest.get("lib", {}).get("path", "src/lib.rs")
    return path.resolve() if path.is_file() else None


def _bin_roots(manifest: dict, base: Path, package: str) -> dict[str, Path]:
    """Binary targets by cargo's rules, as a superset.

    Cargo builds every explicit `[[bin]]` (its `path`, or `src/bin/<name>.rs`,
    `src/bin/<name>/main.rs` or, for the package's own name, `src/main.rs`) and,
    unless `autobins = false`, infers `src/main.rs` (named after the package),
    `src/bin/<x>.rs` and `src/bin/<x>/main.rs` (named `<x>`) — an explicit
    `[[bin]] path` does not switch inference off. The inferred targets are kept
    even under `autobins = false`: a root that is never built only costs a false
    positive. `cargo metadata` records the exact set for workspace members (the
    closure file's `[[bin]]` rows, merged in by `discover_crates`).
    """
    bins: dict[str, Path] = {}
    if (base / "src/main.rs").is_file():
        bins[package] = base / "src/main.rs"
    for single in sorted((base / "src/bin").glob("*.rs")):
        bins[single.stem] = single
    for nested in sorted((base / "src/bin").glob("*/main.rs")):
        bins[nested.parent.name] = nested
    for entry in manifest.get("bin", []):
        name = entry.get("name")
        if not isinstance(name, str):
            continue
        candidates = [base / entry["path"]] if "path" in entry else [
            base / "src/bin" / f"{name}.rs", base / "src/bin" / name / "main.rs",
            *([base / "src/main.rs"] if name == package else []),
        ]
        found = next((c for c in candidates if c.is_file()), None)
        if found is not None:
            bins[name] = found
    return {name: path.resolve() for name, path in bins.items() if path.is_file()}


def discover_crates(
    repo: Path, roots: list[str], recorded_bins: dict[str, dict[str, str]] | None = None,
) -> list[Crate]:
    """Find every Cargo package under `roots`, with sources assigned by ownership.

    A `.rs` file belongs to the *deepest* crate directory containing it, so a
    workspace member nested inside another crate's tree does not steal or leak
    files. Files under `target/` are ignored.

    `recorded_bins` (repo-relative crate directory → {bin name: repo-relative
    root}) are the binary targets `cargo metadata` reported; they are merged
    into the on-disk reading, so a crate the metadata covers has at least
    cargo's own bin set and a crate it does not cover (new, or outside the
    workspace) still has the superset `_bin_roots` infers.

    All returned paths are absolute: roots are resolved, and callers relate
    results back to `repo`, so a relative `repo` would produce a mix of both.
    """
    repo = repo.resolve()
    recorded_bins = recorded_bins or {}
    manifests: list[Path] = []
    for rel in roots:
        base = repo / rel
        if base.is_file() and base.name == "Cargo.toml":
            manifests.append(base)
        elif base.is_dir():
            manifests.extend(
                m for m in base.rglob("Cargo.toml") if "target" not in m.parts
            )

    crates: dict[Path, Crate] = {}
    for manifest_path in sorted(set(manifests)):
        try:
            manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError):
            continue
        package = manifest.get("package")
        if not package:
            continue  # virtual workspace manifest
        base = manifest_path.parent
        name = package.get("name", base.name)
        bins = _bin_roots(manifest, base, name)
        for bin_name, rel in recorded_bins.get(base.relative_to(repo).as_posix(), {}).items():
            path = repo / rel
            if path.is_file():
                bins[bin_name] = path.resolve()
        crates[base.resolve()] = Crate(
            name=name, directory=base, lib=_lib_root(manifest, base), bins=bins,
        )

    owners = sorted(crates, key=lambda p: len(p.parts), reverse=True)
    for base, crate in crates.items():
        for src in sorted(base.rglob("*.rs")):
            if "target" in src.relative_to(base).parts:
                continue
            resolved = src.resolve()
            owner = next(o for o in owners if resolved.is_relative_to(o))
            if owner == base:
                crate.sources.append(src)
    return [crates[k] for k in sorted(crates, key=lambda p: str(p))]
