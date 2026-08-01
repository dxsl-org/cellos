"""Source-level scanning primitives for the F1 (no-unsafe) policy check.

F1 (Spec 16 §6) says every Cell crate carries `#![forbid(unsafe_code)]`. This
module answers the two questions the checker needs, using nothing but the source
tree — no cargo invocation, no network, no build:

  * does a crate root source carry the `#![forbid(unsafe_code)]` attribute?
  * which `.rs` files under a crate contain the `unsafe` keyword?

Both answers are computed on text reduced to code by `lexer.strip_noncode`:
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
    F1 attribute. `sources` is every `.rs` file the crate owns, including files
    outside the module graph — those are exactly what a token scan must catch.
    """

    name: str
    directory: Path
    roots: list[Path] = field(default_factory=list)
    sources: list[Path] = field(default_factory=list)


def _root_sources(manifest: dict, base: Path) -> list[Path]:
    """Crate-root source files: explicit lib/bin paths plus cargo's defaults."""
    roots: list[Path] = []
    lib = manifest.get("lib", {})
    if "path" in lib:
        roots.append(base / lib["path"])
    elif (base / "src/lib.rs").is_file():
        roots.append(base / "src/lib.rs")

    bins = manifest.get("bin", [])
    explicit = False
    for entry in bins:
        if "path" in entry:
            roots.append(base / entry["path"])
            explicit = True
    if not explicit and (base / "src/main.rs").is_file():
        roots.append(base / "src/main.rs")
    for extra in sorted((base / "src/bin").glob("*.rs")):
        roots.append(extra)
    unique = list(dict.fromkeys(p.resolve() for p in roots if p.is_file()))
    return [Path(p) for p in unique]


def discover_crates(repo: Path, roots: list[str]) -> list[Crate]:
    """Find every Cargo package under `roots`, with sources assigned by ownership.

    A `.rs` file belongs to the *deepest* crate directory containing it, so a
    workspace member nested inside another crate's tree does not steal or leak
    files. Files under `target/` are ignored.

    All returned paths are absolute: `_root_sources` resolves, and callers relate
    results back to `repo`, so a relative `repo` would produce a mix of both.
    """
    repo = repo.resolve()
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
        crates[base.resolve()] = Crate(
            name=package.get("name", base.name),
            directory=base,
            roots=_root_sources(manifest, base),
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
