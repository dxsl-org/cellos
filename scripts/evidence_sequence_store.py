"""Pinned-directory persistence for authenticated-evidence replay state."""

from __future__ import annotations

import json
import os
import secrets
import stat
from pathlib import Path

SCHEMA = "cellos.consumed-evidence-sequence/v1"


class CommitIndeterminate(OSError):
    """The store was replaced, but directory durability could not be confirmed."""


def open_trusted_directory(store: Path, manifest: Path | None) -> int:
    """Pin an operator-owned directory whose ancestors resist substitution."""
    parent = store.parent.absolute()
    if parent.resolve() != parent:
        raise ValueError("sequence store parent cannot contain symlinks")
    current = parent
    while True:
        info = current.lstat()
        writable = bool(info.st_mode & 0o022)
        protected_temporary = bool(info.st_mode & stat.S_ISVTX) and info.st_uid == 0
        if not stat.S_ISDIR(info.st_mode) or (writable and not protected_temporary):
            raise ValueError("sequence store ancestors must reject untrusted replacement")
        if current.parent == current:
            break
        current = current.parent
    parent_info = parent.lstat()
    if parent_info.st_uid != os.geteuid():
        raise ValueError("sequence store parent must be operator-owned")
    if manifest is not None:
        bundle_root = manifest.resolve().parent
        if parent == bundle_root or bundle_root in parent.parents:
            raise ValueError("sequence store must be external to the submitted evidence bundle")
    descriptor = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    pinned = os.fstat(descriptor)
    if (pinned.st_dev, pinned.st_ino) != (parent_info.st_dev, parent_info.st_ino):
        os.close(descriptor)
        raise ValueError("sequence store directory changed during validation")
    return descriptor


def open_lock(directory: int, name: str) -> int:
    """Open and validate the store-specific lock inside the pinned directory."""
    descriptor = os.open(
        f".{name}.lock",
        os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC,
        0o600,
        dir_fd=directory,
    )
    info = os.fstat(descriptor)
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
        os.close(descriptor)
        raise ValueError("sequence lock ownership or mode is unsafe")
    return descriptor


def load_store(directory: int, name: str) -> dict | None:
    """Load a provisioned store through the pinned directory without symlinks."""
    try:
        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=directory)
    except FileNotFoundError:
        return None
    info = os.fstat(descriptor)
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
        os.close(descriptor)
        raise ValueError("sequence store ownership or mode is unsafe")
    try:
        with os.fdopen(descriptor, encoding="utf-8") as source:
            value = json.load(source)
    except json.JSONDecodeError as error:
        raise ValueError("sequence store is malformed") from error
    expected = {"schema", "repository", "workflow_ref", "sequence", "manifest_sha256"}
    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError("sequence store schema is invalid")
    if not all(isinstance(value[key], str) and value[key] for key in expected):
        raise ValueError("sequence store fields are invalid")
    if value["schema"] != SCHEMA:
        raise ValueError("sequence store version is unsupported")
    digest = value["manifest_sha256"]
    if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
        raise ValueError("sequence store manifest digest is invalid")
    return value


def atomic_write(directory: int, name: str, value: dict) -> None:
    """Replace state inside the pinned directory and report uncertain durability."""
    temporary = f".{name}.{secrets.token_hex(16)}"
    descriptor = os.open(
        temporary,
        os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW | os.O_CLOEXEC,
        0o600,
        dir_fd=directory,
    )
    published = False
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            json.dump(value, output, sort_keys=True)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, name, src_dir_fd=directory, dst_dir_fd=directory)
        published = True
        try:
            os.fsync(directory)
        except OSError as error:
            raise CommitIndeterminate("sequence commit is visible but durability is indeterminate") from error
    finally:
        if not published:
            try:
                os.unlink(temporary, dir_fd=directory)
            except FileNotFoundError:
                pass


def state(repository: str, workflow_ref: str, sequence: str, digest: str) -> dict:
    """Build one canonical replay-store record."""
    return {
        "schema": SCHEMA,
        "repository": repository,
        "workflow_ref": workflow_ref,
        "sequence": sequence,
        "manifest_sha256": digest,
    }
