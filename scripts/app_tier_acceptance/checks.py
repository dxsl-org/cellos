"""Small strict-validation helpers shared by the acceptance ledger checks."""

from __future__ import annotations

from collections.abc import Callable

import datetime as dt
import hashlib
import json
import os
import re
import subprocess
from pathlib import Path
from contextvars import ContextVar

UTC = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
HEX = re.compile(r"^[0-9a-f]{64}$")
GIT = re.compile(r"^[0-9a-f]{40}$")
# Bound the history walk used when a working copy has moved on: evidence paths
# are rewritten rarely, so the newest 100 commits that touch one are plenty.
HISTORY_LIMIT = 100
_CACHE: ContextVar[dict | None] = ContextVar("app_tier_acceptance_cache", default=None)

from . import source as sdk_source


def begin_context():
    """Start an isolated cache for one top-level validator invocation."""
    return _CACHE.set({"files": {}, "paths": {}, "git": {}})


def end_context(token) -> None:
    """Discard a top-level validator cache even when validation fails."""
    _CACHE.reset(token)

def cached_paths(root: Path, namespace: str, discover: Callable[[], set[str]]) -> set[str]:
    """Reuse one immutable path discovery result within a validator invocation."""
    cache = _CACHE.get()
    if cache is None:
        return discover()
    key = (namespace, str(root))
    if key not in cache["paths"]:
        cache["paths"][key] = frozenset(discover())
    return set(cache["paths"][key])


def exact(value: object, keys: set[str], label: str) -> dict:
    """Require an object with no omitted or undeclared schema keys."""
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError(f"{label}: exact keys required")
    return value


def text(value: object, label: str) -> str:
    """Require a non-empty string, never accepting a truthy substitute."""
    if not isinstance(value, str) or not value:
        raise ValueError(f"{label}: non-empty string required")
    return value


def integer(value: object, label: str) -> int:
    """Require a real integer and reject Python's bool subtype explicitly."""
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{label}: integer required")
    return value


def timestamp(value: object, label: str) -> dt.datetime:
    """Require strict RFC3339 UTC seconds and return an aware instant."""
    string = text(value, label)
    if not UTC.fullmatch(string):
        raise ValueError(f"{label}: RFC3339 UTC timestamp required")
    return dt.datetime.fromisoformat(string.replace("Z", "+00:00"))


def canonical_digest(value: object) -> str:
    """Hash canonical JSON to make history state independent of formatting."""
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def preserve(root: Path, rel: str, digest: object) -> Path:
    """Return the archived copy of the source revision behind a recorded digest.

    Source files are amendable in place, so a witness can never be verified
    against the working tree: the contract keeps one archived revision per
    digest and every other source file is mirrored under `docs/evidence/source/`.
    """
    if rel == sdk_source.SOURCE_PATH:
        return root / sdk_source.snapshot_path(text(digest, "artifact.sha256"))
    if rel.startswith(f"{sdk_source.SNAPSHOT_DIR}/{sdk_source.SNAPSHOT_PREFIX}"):
        return root / rel
    return root / sdk_source.SOURCE_MIRROR / rel


def in_history(root: Path, rel: str, digest: str, size: int) -> bool:
    """True when a commit reachable from HEAD still holds those exact bytes.

    A recorded log or artifact is evidence of a moment, not a licence to freeze
    the working file forever. The file may legitimately move on - a log is
    appended to, a patch is revised - and the recorded bytes stay verifiable
    because history keeps them. This is the working-tree counterpart of the
    content-addressed mirror that `kind = "source"` evidence resolves against:
    neither class of evidence can be invalidated by an ordinary later edit, and
    a digest that no commit holds is still refused.
    """
    log = subprocess.run(
        ["git", "log", f"--max-count={HISTORY_LIMIT}", "--format=%H", "--", rel],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if log.returncode != 0:
        return False
    for revision in log.stdout.split():
        blob = subprocess.run(["git", "show", f"{revision}:{rel}"], cwd=root, capture_output=True)
        if blob.returncode == 0 and len(blob.stdout) == size and hashlib.sha256(blob.stdout).hexdigest() == digest:
            return True
    return False


def safe_file(root: Path, path: object, digest: object, size: object, kind: object) -> None:
    """Verify a repository-contained regular evidence file and its exact digest."""
    rel = text(path, "artifact.path")
    if "\\" in rel or "\x00" in rel or rel.startswith("/") or ".." in Path(rel).parts:
        raise ValueError("artifact path is not safe repository-relative")
    if not HEX.fullmatch(text(digest, "artifact.sha256")):
        raise ValueError("artifact size or digest schema is invalid")
    int_size = integer(size, "artifact.size_bytes")
    if kind not in {"log", "artifact", "source"}:
        raise ValueError("artifact digest or kind is invalid")
    key = (str(root), rel, digest, int_size, kind)
    cache = _CACHE.get()
    if cache is not None and key in cache["files"]:
        return
    target = root / rel
    if kind == "source":
        target = preserve(root, rel, digest)
        if not target.is_file():
            raise ValueError("preserved source revision is missing")
    if target.is_symlink() or not target.is_file() or root not in target.resolve().parents:
        raise ValueError("artifact path is missing, outside root, or a symlink")
    size_matches = int_size == target.stat().st_size
    digest_matches = size_matches and hashlib.sha256(target.read_bytes()).hexdigest() == digest
    if not digest_matches and kind != "source" and in_history(root, rel, digest, int_size):
        # The working copy moved on, but the recorded revision is preserved.
        size_matches = digest_matches = True
    if not size_matches:
        raise ValueError("artifact size or digest schema is invalid")
    if not digest_matches:
        raise ValueError("artifact digest or kind is invalid")
    if cache is not None:
        cache["files"][key] = True
