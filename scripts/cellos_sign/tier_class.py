"""Tier-class gate: a crate holding C-ABI or raw-pointer `unsafe` runs in Tier 2.

The kernel picks a signed cell's tier from the manifest class byte that the
signature covers: FFI or UNTRUSTED gives Tier 2 (a page-table wall), anything
else — including no manifest at all — gives Tier 1, where language isolation is
the only wall (`kernel/src/loader/governed_spawn.rs`). An allowlist entry of
class `c-ffi` or `legacy-raw-ptr` says the crate holds `unsafe` that Rust's
type system cannot vouch for, so its manifest must ask for Tier 2. A Tier-1
crate may keep such an entry only through a reviewed `[[tier_exception]]` that
names the manifest class it accepts.

The manifest class is read from source, and the reading fails closed. A crate
reads FFI or UNTRUSTED only when
  * every `declare_manifest!` / `app_entry!` invocation in its tracked sources
    passes `tier = abi::manifest::<FFI or UNTRUSTED constant>` literally, and
  * every binary root carries at least one such invocation.
A tier spelt any other way (a local `const`, a re-export) reads as
`UNRECOGNISED`, a hand-written `__ViCell_manifest` section likewise, and a
binary root without an invocation adds `ABSENT`. All of these leave the crate
in Tier 1: a false positive, never a false negative.

Like the rest of `cellos-sign`, this is a check on the tree, not on an ELF. The
signature covers the manifest byte, so nothing after signing can change the
class (Spec 18 §2.1).
"""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from . import scan
from .allowlist import TIER2_CLASSES, Allowlist

TIER2_MANIFESTS = frozenset({"FFI", "UNTRUSTED"})

# Constant name → manifest class (`libs/abi/src/manifest_flags.rs`).
_CONSTANTS = {
    "PROTECTION_CLASS_FFI": "FFI",
    "TIER_TIER1B_FFI": "FFI",
    "PROTECTION_CLASS_UNTRUSTED": "UNTRUSTED",
    "TIER_UNTRUSTED": "UNTRUSTED",
    "PROTECTION_CLASS_STANDARD": "STANDARD",
    "TIER_STANDARD": "STANDARD",
    "PROTECTION_CLASS_TRUSTED_CORE": "TRUSTED_CORE",
    "TIER_TRUSTED_CORE": "TRUSTED_CORE",
    "PROTECTION_CLASS_LEGACY": "LEGACY",
    "TIER_LEGACY": "LEGACY",
}
_INVOCATION_RE = re.compile(r"\b(?:__declare_manifest|declare_manifest|app_entry)\s*!\s*([(\[{])")
_TIER_VALUE_RE = re.compile(r"(?:::)?abi::manifest::([A-Z_][A-Z0-9_]*)")
# A manifest section written by hand instead of through the macro. Checked on
# the raw text, because the section name is a string literal.
_HAND_SECTION_RE = re.compile(r'link_section\s*=\s*"__ViCell_manifest"')
_CLOSERS = {"(": ")", "[": "]", "{": "}"}


def _arguments(code: str, open_index: int) -> str:
    """The text between the bracket at `open_index` and its matching closer."""
    depth, i = 0, open_index
    while i < len(code):
        char = code[i]
        if char in _CLOSERS:
            depth += 1
        elif char in _CLOSERS.values():
            depth -= 1
            if depth == 0:
                return code[open_index + 1 : i]
        i += 1
    return code[open_index + 1 :]


def _split_top_level(args: str) -> list[str]:
    parts, depth, start = [], 0, 0
    for i, char in enumerate(args):
        if char in _CLOSERS:
            depth += 1
        elif char in _CLOSERS.values():
            depth -= 1
        elif char == "," and depth == 0:
            parts.append(args[start:i])
            start = i + 1
    parts.append(args[start:])
    return [p.strip() for p in parts if p.strip()]


def invocation_classes(code: str) -> list[str]:
    """The manifest class of each manifest-macro invocation in reduced source."""
    classes = []
    for match in _INVOCATION_RE.finditer(code):
        klass = "LEGACY"  # the tier-less forms emit PROTECTION_CLASS_LEGACY
        for arg in _split_top_level(_arguments(code, match.start(1))):
            name, sep, value = arg.partition("=")
            if not sep or name.strip() != "tier":
                continue
            value = re.sub(r"\s+", "", value)
            constant = _TIER_VALUE_RE.fullmatch(value)
            klass = _CONSTANTS.get(constant.group(1), "UNRECOGNISED") if constant else "UNRECOGNISED"
        classes.append(klass)
    return classes


def _lib_root(crate: scan.Crate) -> Path | None:
    try:
        manifest = tomllib.loads((crate.directory / "Cargo.toml").read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None
    lib = manifest.get("lib", {})
    path = crate.directory / lib.get("path", "src/lib.rs")
    return path.resolve() if path.is_file() else None


def manifest_class(crate: scan.Crate, tracked: set[Path]) -> str:
    """Read a crate's manifest class from its tracked sources (fail closed).

    Returns `ABSENT`, a single class, or several joined by `+` (`FFI+LEGACY`).
    """
    found: set[str] = set()
    with_invocation: set[Path] = set()
    for src in crate.sources:
        resolved = src.resolve()
        if resolved not in tracked:
            continue
        raw = src.read_text(encoding="utf-8", errors="replace")
        classes = invocation_classes(scan.strip_noncode(raw))
        if _HAND_SECTION_RE.search(raw):
            classes.append("UNRECOGNISED")
        if classes:
            found.update(classes)
            with_invocation.add(resolved)
    lib = _lib_root(crate)
    bins = [r.resolve() for r in crate.roots if r.resolve() != lib and r.resolve() in tracked]
    if not found or any(root not in with_invocation for root in bins):
        found.add("ABSENT")
    return "+".join(sorted(found))


def is_tier2(manifest: str) -> bool:
    return set(manifest.split("+")) <= TIER2_MANIFESTS


@dataclass
class Record:
    """One crate's row of the allowlist-class × manifest-class join."""

    crate: str
    directory: str
    classes: set[str]
    manifest: str

    @property
    def tier(self) -> int:
        return 2 if is_tier2(self.manifest) else 1

    @property
    def triggers(self) -> set[str]:
        return self.classes & TIER2_CLASSES


@dataclass
class Outcome:
    records: dict[str, Record] = field(default_factory=dict)
    # (crate, class, detail) for every mismatch not covered by an exception.
    mismatches: list[tuple[str, str, str]] = field(default_factory=list)
    used_exceptions: set[tuple[str, str]] = field(default_factory=set)
    unused_exceptions: list[str] = field(default_factory=list)


def evaluate(
    repo: Path,
    crates: list[scan.Crate],
    tracked: set[Path],
    allow: Allowlist,
    crate_of_path,
) -> Outcome:
    """Join allowlist classes with manifest classes and apply the gate.

    `crate_of_path` maps a repo-relative path to its owning crate name, with
    the same deepest-directory rule the token layer uses.
    """
    out = Outcome()
    for crate in crates:
        out.records[crate.name] = Record(
            crate=crate.name,
            directory=crate.directory.relative_to(repo).as_posix(),
            classes=set(),
            manifest=manifest_class(crate, tracked),
        )
    for name, entry in allow.crates.items():
        if name in out.records:
            out.records[name].classes.add(entry.klass)
    for path, entry in allow.files.items():
        owner = crate_of_path(path)
        if owner in out.records:
            out.records[owner].classes.add(entry.klass)

    for record in sorted(out.records.values(), key=lambda r: r.crate):
        if record.tier == 2:
            continue
        for klass in sorted(record.triggers):
            exception = allow.tier_exceptions.get((record.crate, klass))
            if exception is not None and exception.manifest == record.manifest:
                out.used_exceptions.add((record.crate, klass))
                continue
            detail = (
                f"allowlist class `{klass}` needs manifest class FFI or UNTRUSTED, "
                f"but the crate reads {record.manifest} (Tier 1). Declare "
                "`tier = abi::manifest::PROTECTION_CLASS_FFI` (or `_UNTRUSTED`), "
                "or add a reviewed [[tier_exception]]"
            )
            if exception is not None:
                detail = (
                    f"the [[tier_exception]] for `{klass}` accepts manifest "
                    f"{exception.manifest}, but the crate now reads {record.manifest}; "
                    "re-review the exception"
                )
            out.mismatches.append((record.crate, klass, detail))

    out.unused_exceptions = sorted(
        exception.label
        for key, exception in allow.tier_exceptions.items()
        if key not in out.used_exceptions
    )
    return out
