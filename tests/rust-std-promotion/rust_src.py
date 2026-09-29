"""Resolve the pinned rust-src checkout the feasibility pins were taken from.

`pal-hook-support-map.json` and `approval-input-manifest.json` record the absolute
path of the rust-src tree the package was verified against — a maintainer-machine
layout that no CI runner shares. The same pinned toolchain is installed there, so
the same tree is reachable through its sysroot; resolving it that way keeps every
digest check running on CI instead of failing on a path that exists on one machine
only. An unresolvable pin is an error, never a skip.
"""
from __future__ import annotations

import subprocess
from pathlib import Path

PINNED_TOOLCHAIN = "nightly-2026-05-01"


def resolve(pinned: str) -> Path:
    """Return the rust-src root named by `pinned`, or the installed toolchain's."""
    candidate = Path(pinned)
    if candidate.is_dir():
        return candidate

    commands = (
        ("rustc", f"+{PINNED_TOOLCHAIN}", "--print", "sysroot"),
        ("rustc", "--print", "sysroot"),
    )
    for command in commands:
        found = subprocess.run(command, capture_output=True, text=True)
        if found.returncode != 0 or not found.stdout.strip():
            continue
        candidate = Path(found.stdout.strip()) / "lib/rustlib/src/rust"
        if candidate.is_dir():
            return candidate

    raise FileNotFoundError(
        f"pinned rust-src not found at {pinned!r} and not resolvable through the "
        f"{PINNED_TOOLCHAIN} sysroot (install that toolchain with the rust-src component)"
    )
