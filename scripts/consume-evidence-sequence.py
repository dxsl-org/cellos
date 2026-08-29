#!/usr/bin/env python3
"""Provision or atomically consume a trusted authenticated-evidence sequence."""

from __future__ import annotations

import argparse
import fcntl
import os
from pathlib import Path

from authenticated_evidence import verify_bundle_bytes
from evidence_sequence_store import (
    CommitIndeterminate,
    atomic_write,
    load_store,
    open_lock,
    open_trusted_directory,
    state,
)


def parse_sequence(value: str) -> tuple[int, int]:
    """Return a positive `(run_id, attempt)` pair or reject malformed input."""
    parts = value.split(":")
    if len(parts) != 2 or not all(part.isascii() and part.isdigit() for part in parts):
        raise ValueError("sequence must be <positive-run-id>:<positive-attempt>")
    sequence = (int(parts[0]), int(parts[1]))
    if sequence[0] < 1 or sequence[1] < 1:
        raise ValueError("sequence values must be positive")
    return sequence


def initialize(store: Path, repository: str, workflow_ref: str) -> None:
    """Provision an empty store while holding the same lock used by consumers."""
    directory = open_trusted_directory(store, None)
    lock = open_lock(directory, store.name)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if load_store(directory, store.name) is not None:
            raise ValueError("sequence store already exists")
        atomic_write(directory, store.name, state(repository, workflow_ref, "0:0", "0" * 64))
    finally:
        os.close(lock)
        os.close(directory)


def consume(
    store: Path,
    repository: str,
    workflow_ref: str,
    sequence_text: str,
    manifest_path: Path,
    expected_digest: str,
) -> None:
    """Reverify and consume a strictly newer sequence under one pinned lock."""
    sequence = parse_sequence(sequence_text)
    directory = open_trusted_directory(store, manifest_path)
    lock = open_lock(directory, store.name)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        previous = load_store(directory, store.name)
        if previous is None:
            raise ValueError("sequence store is not provisioned")
        if previous["repository"] != repository or previous["workflow_ref"] != workflow_ref:
            raise ValueError("sequence store identity mismatch")
        previous_text = previous["sequence"]
        prior = (0, 0) if previous_text == "0:0" else parse_sequence(previous_text)
        if sequence <= prior:
            raise ValueError("evidence sequence replay or regression")
        manifest = verify_bundle_bytes(manifest_path, expected_digest)
        if manifest.get("sequence") != sequence_text or manifest.get("workflow_ref") != workflow_ref:
            raise ValueError("manifest identity changed before sequence consumption")
        atomic_write(
            directory,
            store.name,
            state(repository, workflow_ref, sequence_text, expected_digest),
        )
    finally:
        os.close(lock)
        os.close(directory)


def main() -> int:
    """Provision a store explicitly or atomically consume verified evidence."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--store", required=True, type=Path)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--workflow-ref", required=True)
    parser.add_argument("--initialize", action="store_true")
    parser.add_argument("--sequence")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--expected-manifest-sha256")
    args = parser.parse_args()
    try:
        if args.initialize:
            if args.sequence or args.manifest or args.expected_manifest_sha256:
                raise ValueError("--initialize cannot consume evidence")
            initialize(args.store, args.repository, args.workflow_ref)
            print(f"PASS: provisioned evidence sequence store {args.store}")
        else:
            if not args.sequence or args.manifest is None or not args.expected_manifest_sha256:
                raise ValueError("consumption requires sequence, manifest, and expected manifest digest")
            consume(
                args.store,
                args.repository,
                args.workflow_ref,
                args.sequence,
                args.manifest,
                args.expected_manifest_sha256,
            )
            print(f"PASS: consumed evidence sequence {args.sequence}")
    except CommitIndeterminate as error:
        parser.exit(2, f"INDETERMINATE: {error}; inspect the trusted store before retrying\n")
    except (OSError, ValueError) as error:
        parser.exit(1, f"FAIL: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
