"""Exact, lossless import of the ratified Native SDK matrix."""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE_PATH = "docs/specs/23-native-sdk-contract.md"
CELL_AXES = ("rust-no-std", "rust-std", "ffi-posix", "lua", "T1", "T2")


def sha256_bytes(value: bytes) -> str:
    """Return the lowercase SHA-256 of raw evidence bytes."""
    return hashlib.sha256(value).hexdigest()


def source_file(root: Path = ROOT) -> Path:
    """Return the SDK contract location beneath a validation root."""
    return root / SOURCE_PATH


def matrix(root: Path = ROOT) -> list[list[object]]:
    """Import all ten rows and six original cells without normalising wording."""
    text = source_file(root).read_text(encoding="utf-8")
    try:
        section = text.split("## 5. Capability matrix", 1)[1].split("### 5.1", 1)[0]
    except IndexError as error:
        raise ValueError("Spec 23 capability matrix is missing") from error
    rows = []
    for line in section.splitlines():
        if line.startswith("| C2-"):
            fields = [field.strip() for field in line.strip().strip("|").split("|")]
            rows.append([fields[0].split(" /", 1)[0], fields[1:7]])
    if len(rows) != 10 or any(len(row[1]) != 6 for row in rows):
        raise ValueError("Spec 23 must contain exactly ten rows of six cells")
    return rows


def matrix_digest(root: Path = ROOT) -> str:
    """Hash the canonical, complete imported matrix."""
    return sha256_bytes(json.dumps(matrix(root), sort_keys=True, separators=(",", ":")).encode())


def availability(value: str) -> str:
    """Extract one and only one source availability enum."""
    found = set(re.findall(r"\*\*(USABLE|PARTIAL|BLOCKED|PLANNED|UNSUPPORTED|N/A)\*\*", value))
    if len(found) != 1:
        raise ValueError(f"ambiguous source availability: {value}")
    return found.pop()


TARGETS = ("riscv64gc-unknown-none-elf", "aarch64-unknown-none-softfloat", "x86_64-unknown-none")
FEATURES = tuple(
    f"api={api};ostd={ostd};viui={viui}"
    for api in ("default", "posix", "mlibc", "posix+mlibc")
    for ostd in ("default", "json", "http", "json+http")
    for viui in ("default", "gles2")
)


def denominator_tuple(target: str, feature_selection: str) -> tuple[str, ...]:
    """Return every field in the canonical ratified Rust build denominator."""
    cpu = "riscv64" if target.startswith("riscv64") else "aarch64" if target.startswith("aarch64") else "x86_64"
    flags = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg" if cpu == "aarch64" else "-C relocation-model=pic"
    return ("nightly-2026-05-01", target, "rust", f'target_arch="{cpu}"', flags, feature_selection, feature_selection, "release", "rust-no-std", "T1")


def denominator(target: str, feature_selection: str) -> str:
    """Return the stable key for a canonical ratified Rust build denominator."""
    return "|".join(denominator_tuple(target, feature_selection))


def applicability(row_id: str, cell_id: str) -> dict[str, list[str]]:
    """Declare only the build denominators ratified by Spec 23.

    Runtime execution environments are witness facts, never inferred from a
    build tuple.  Rust-std and Tier-2 cells have no ratified build denominator.
    """
    del row_id
    axis = cell_id.rsplit("/", 1)[1]
    if axis in {"rust-no-std", "T1"}:
        return {"build_denominators": [denominator(target, features) for target in TARGETS for features in FEATURES]}
    return {"build_denominators": []}
