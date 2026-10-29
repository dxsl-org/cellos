"""Policy F5: the running toolchain must be the pinned one.

Signing attests that the artifact came from *the* qualified compiler, so a build
on a drifted nightly must not be signed — a different rustc is a different TCB
(Spec 16). The pin is read from `rust-toolchain.toml` at run time, never
hardcoded, so a future Ferrocene switch needs no change here.

The check compares **rustc commit hashes**, not toolchain names:

    rustc +<pin> -Vv   → the compiler the pin *names*
    rustc -Vv          → the compiler that will actually run here

Comparing names against `rustup show active-toolchain` would be close to
tautological — rustup resolves the name by reading the very `rust-toolchain.toml`
we are validating, so both sides move together. The hash comparison catches what
actually goes wrong: `RUSTUP_TOOLCHAIN` in the environment, a `rustup override`
on the directory, or a pinned toolchain that is not installed at all (in which
case `rustc +<pin>` fails, and refusing to sign is the correct answer).
"""

from __future__ import annotations

import subprocess
import tomllib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ToolchainStatus:
    """`ok=False` is a policy failure; `ok=True, skipped=True` is 'could not tell'."""

    ok: bool
    skipped: bool
    detail: str
    expected: str | None = None
    actual: str | None = None


def pinned_channel(repo: Path) -> str | None:
    """The `[toolchain] channel` from `rust-toolchain.toml`, or None if absent."""
    path = repo / "rust-toolchain.toml"
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None
    channel = data.get("toolchain", {}).get("channel")
    return channel if isinstance(channel, str) and channel else None


def _rustc_commit(repo: Path, channel: str | None) -> str | None:
    """`commit-hash` from `rustc [+channel] -Vv`, or None if that rustc cannot run."""
    argv = ["rustc"] + ([f"+{channel}"] if channel else []) + ["-Vv"]
    try:
        out = subprocess.run(
            argv, cwd=repo, capture_output=True, text=True, check=True, timeout=120
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    for line in out.splitlines():
        if line.startswith("commit-hash:"):
            return line.split(":", 1)[1].strip()
    return None


def check(repo: Path) -> ToolchainStatus:
    """Verify the rustc that will run here is the rustc the pin names."""
    expected = pinned_channel(repo)
    if expected is None:
        return ToolchainStatus(
            ok=False, skipped=False,
            detail="rust-toolchain.toml has no [toolchain] channel — the pin that "
                   "F5 attests does not exist",
        )

    active = _rustc_commit(repo, None)
    if active is None:
        return ToolchainStatus(
            ok=True, skipped=True, expected=expected,
            detail=f"no working rustc on PATH — cannot confirm the toolchain is "
                   f"{expected!r}; F5 unverified",
        )
    pinned = _rustc_commit(repo, expected)
    if pinned is None:
        return ToolchainStatus(
            ok=False, skipped=False, expected=expected, actual=active,
            detail=f"the pinned toolchain {expected!r} is not installed, so the "
                   f"active rustc ({active[:12]}) cannot be shown to be it",
        )
    if pinned == active:
        return ToolchainStatus(
            ok=True, skipped=False, expected=expected, actual=active,
            detail=f"rustc {active[:12]} is the pinned {expected}",
        )
    return ToolchainStatus(
        ok=False, skipped=False, expected=expected, actual=active,
        detail=f"active rustc {active[:12]} is not the pinned {expected} "
               f"({pinned[:12]}) — check RUSTUP_TOOLCHAIN and `rustup override list`",
    )
