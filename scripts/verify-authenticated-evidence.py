#!/usr/bin/env python3
"""Verify manifest integrity after GitHub attestation verification."""
import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def verify_members(root, manifest, kind, required):
    members = manifest.get(kind)
    if not isinstance(members, list) or (required and not members):
        raise ValueError(f"{kind} must be a non-empty list")
    names = set()
    for member in members:
        if not isinstance(member, dict):
            raise ValueError(f"invalid {kind} member")
        name, relative, expected, size = (member.get(key) for key in ("name", "path", "sha256", "bytes"))
        if not isinstance(name, str) or name in names or relative != f"{kind}/{name}":
            raise ValueError(f"invalid {kind} path")
        path = (root / relative).resolve()
        if root not in path.parents or not path.is_file() or path.stat().st_size != size or digest(path) != expected:
            raise ValueError(f"{kind}/{name} digest mismatch")
        names.add(name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--expected-revision", required=True)
    parser.add_argument("--expected-sequence", required=True)
    args = parser.parse_args()
    path = args.manifest.resolve()
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"invalid manifest: {error}") from error
    if not isinstance(manifest, dict) or manifest.get("schema") != "cellos.authenticated-evidence/v1":
        raise SystemExit("unsupported evidence schema")
    if manifest.get("revision") != args.expected_revision or manifest.get("sequence") != args.expected_sequence:
        raise SystemExit("revision or sequence mismatch")
    if manifest.get("result") != "passed" or not all(isinstance(manifest.get(key), str) and manifest[key] for key in ("runner", "workflow_ref", "command")):
        raise SystemExit("invalid evidence identity or result")
    if not isinstance(manifest.get("environment"), dict):
        raise SystemExit("invalid environment")
    try:
        verify_members(path.parent, manifest, "inputs", True)
        verify_members(path.parent, manifest, "logs", True)
        verify_members(path.parent, manifest, "images", False)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    print("PASS: authenticated evidence contents verified")


if __name__ == "__main__":
    main()
