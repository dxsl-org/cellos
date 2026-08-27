"""Resolve the trusted ledger baseline revision for CI events."""

from __future__ import annotations

import subprocess

LEDGER = "docs/app-tier-acceptance-ledger.json"
ZERO = "0" * 40


def trusted_snapshot(ref: str) -> str:
    """Return the latest commit at or before `ref` that changed the ledger."""
    history = subprocess.run(
        ["git", "log", "-1", "--format=%H", ref, "--", LEDGER],
        capture_output=True,
        text=True,
    )
    revision = history.stdout.strip()
    if history.returncode or not revision:
        raise ValueError("trusted ref has no ledger snapshot")
    probe = subprocess.run(["git", "cat-file", "-e", f"{revision}:{LEDGER}"])
    if probe.returncode:
        raise ValueError("trusted ledger snapshot is unreadable")
    return revision


def dispatch_baseline(ref: str, default_branch: str) -> str:
    """Return the parent of the latest first-parent ledger transition.

    Raises ValueError when a manual dispatch is not on the default branch or
    the transition has no parent ledger snapshot.
    """
    if ref != default_branch:
        raise ValueError("workflow_dispatch must run on the default branch")
    history = subprocess.run(["git", "rev-list", "--first-parent", "HEAD"], check=True, capture_output=True, text=True)
    for commit in history.stdout.splitlines():
        changed = subprocess.run(["git", "diff-tree", "--no-commit-id", "--name-only", "-r", commit], check=True, capture_output=True, text=True)
        if LEDGER not in changed.stdout.splitlines():
            continue
        parent = subprocess.run(["git", "rev-parse", f"{commit}^"], capture_output=True, text=True)
        if parent.returncode:
            raise ValueError("initial ledger seed has no baseline parent")
        probe = subprocess.run(["git", "cat-file", "-e", f"{parent.stdout.strip()}:{LEDGER}"])
        if probe.returncode:
            raise ValueError("ledger transition parent lacks a baseline ledger")
        return parent.stdout.strip()
    raise ValueError("no first-parent ledger transition found")
