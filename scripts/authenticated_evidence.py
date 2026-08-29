"""Shared fail-closed parsing and content verification for authenticated evidence."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path


def digest_bytes(value: bytes) -> str:
    """Return the lowercase SHA-256 digest of bytes."""
    return hashlib.sha256(value).hexdigest()


def digest_file(path: Path) -> str:
    """Hash a regular file without retaining its full contents."""
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def verify_members(root: Path, manifest: dict, kind: str, required: bool) -> None:
    """Verify every declared member path, size, uniqueness, and digest."""
    members = manifest.get(kind)
    if not isinstance(members, list) or (required and not members):
        raise ValueError(f"{kind} must be a non-empty list")
    names = set()
    for member in members:
        if not isinstance(member, dict):
            raise ValueError(f"invalid {kind} member")
        name, relative, expected, size = (
            member.get(key) for key in ("name", "path", "sha256", "bytes")
        )
        if not isinstance(name, str) or name in names or relative != f"{kind}/{name}":
            raise ValueError(f"invalid {kind} path")
        path = (root / relative).resolve()
        if (
            root not in path.parents
            or not path.is_file()
            or path.stat().st_size != size
            or digest_file(path) != expected
        ):
            raise ValueError(f"{kind}/{name} digest mismatch")
        names.add(name)


def verify_bundle_bytes(path: Path, expected_manifest_sha256: str) -> dict:
    """Verify the manifest digest, schema, and all content-addressed members."""
    resolved = path.resolve()
    try:
        manifest_bytes = resolved.read_bytes()
        manifest = json.loads(manifest_bytes)
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid manifest: {error}") from error
    if digest_bytes(manifest_bytes) != expected_manifest_sha256:
        raise ValueError("manifest digest differs from attested subject")
    if not isinstance(manifest, dict) or manifest.get("schema") != "cellos.authenticated-evidence/v1":
        raise ValueError("unsupported evidence schema")
    verify_members(resolved.parent, manifest, "inputs", True)
    verify_members(resolved.parent, manifest, "logs", True)
    verify_members(resolved.parent, manifest, "images", False)
    return manifest
