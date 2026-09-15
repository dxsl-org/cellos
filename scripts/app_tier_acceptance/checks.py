"""Small strict-validation helpers shared by the acceptance ledger checks."""

from __future__ import annotations

from collections.abc import Callable

import datetime as dt
import hashlib
import json
import os
import re
from pathlib import Path
from contextvars import ContextVar

UTC = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
HEX = re.compile(r"^[0-9a-f]{64}$")
GIT = re.compile(r"^[0-9a-f]{40}$")
_CACHE: ContextVar[dict | None] = ContextVar("app_tier_acceptance_cache", default=None)

from . import source as sdk_source


def begin_context():
    """Start an isolated cache for one top-level validator invocation."""
    return _CACHE.set({"files": {}, "paths": {}})


def end_context(token) -> None:
    """Discard a top-level validator cache even when validation fails."""
    _CACHE.reset(token)

def cached_paths(root: Path, namespace: str, discover: Callable[[], set[str]]) -> set[str]:
    """Reuse one immutable path discovery result within a validator invocation."""
    cache = _CACHE.get()
    if cache is None:
        return discover()
    key = (namespace, str(root.resolve()))
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


def safe_file(root: Path, path: object, digest: object, size: object, kind: object) -> None:
    """Verify a repository-contained regular evidence file and its exact digest."""
    rel = text(path, "artifact.path")
    if "\\" in rel or "\x00" in rel or rel.startswith("/") or ".." in Path(rel).parts:
        raise ValueError("artifact path is not safe repository-relative")
    target = root / rel
    if kind == "source":
        target = preserve(root, rel, digest)
        if not target.is_file():
            raise ValueError("preserved source revision is missing")
    if target.is_symlink() or not target.is_file() or root not in target.resolve().parents:
        raise ValueError("artifact path is missing, outside root, or a symlink")
    if not HEX.fullmatch(text(digest, "artifact.sha256")) or integer(size, "artifact.size_bytes") != target.stat().st_size:
        raise ValueError("artifact size or digest schema is invalid")
    key = (str(root.resolve()), rel, digest, size, kind)
    cache = _CACHE.get()
    if cache is not None and key in cache["files"]:
        return
    if hashlib.sha256(target.read_bytes()).hexdigest() != digest or kind not in {"log", "artifact", "source"}:
        raise ValueError("artifact digest or kind is invalid")
    if cache is not None:
        cache["files"][key] = True
