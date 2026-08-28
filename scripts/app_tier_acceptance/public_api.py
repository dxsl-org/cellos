"""Complete content-addressed snapshot of the in-repository Native SDK surface."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

from .checks import GIT, cached_paths, canonical_digest, exact, safe_file

ROOTS = (
    "libs/api/src/lib.rs",
    "libs/ostd/src/lib.rs",
    "libs/types/src/lib.rs",
    "libs/viui/src/lib.rs",
    "libs/viui-macros/src/lib.rs",
    "libs/viui/Cargo.toml",
    "libs/viui-macros/Cargo.toml",
)
FILE_MODULE = re.compile(
    r'(?:^\s*#\[\s*path\s*=\s*"([^"]+)"\s*\]\s*\n)?'
    r"^\s*(?:pub\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;",
    re.MULTILINE,
)


def paths(root: Path) -> set[str]:
    """Resolve every file-backed module reachable from the public SDK roots."""
    return cached_paths(root, "public-api", lambda: _discover_paths(root))


def _discover_paths(root: Path) -> set[str]:
    """Walk the public SDK module graph without consulting validation caches."""
    pending, found = [Path(path) for path in ROOTS], set()
    while pending:
        relative = pending.pop()
        name = relative.as_posix()
        if name in found:
            continue
        source_path = root / relative
        if not source_path.is_file():
            raise ValueError(f"public SDK source is missing: {name}")
        found.add(name)
        if relative.suffix != ".rs":
            continue
        module_root = relative.parent if relative.name in {"lib.rs", "mod.rs"} else relative.parent / relative.stem
        source = source_path.read_text(encoding="utf-8")
        for declared_path, module in FILE_MODULE.findall(source):
            candidates = (
                (relative.parent / declared_path,)
                if declared_path
                else (module_root / f"{module}.rs", module_root / module / "mod.rs")
            )
            resolved = next((candidate for candidate in candidates if (root / candidate).is_file()), None)
            if resolved is None:
                raise ValueError(f"public module {module} declared by {name} has no source file")
            pending.append(resolved)
    return found


def _committed_sources(root: Path, revision: str, expected: set[str]) -> dict[str, bytes]:
    """Batch-read exact Git blob bytes for the claimed public source revision."""
    if not GIT.fullmatch(revision):
        raise ValueError("clean public API revision is invalid")
    listing = subprocess.run(
        ["git", "ls-tree", "-r", "-z", revision, "--", *sorted(expected)],
        cwd=root,
        capture_output=True,
    )
    if listing.returncode:
        raise ValueError("clean public API revision is not readable")
    objects = {}
    try:
        for record in filter(None, listing.stdout.split(b"\0")):
            metadata, raw_path = record.split(b"\t", 1)
            _, kind, object_id = metadata.split(b" ", 2)
            path = raw_path.decode("utf-8")
            if kind != b"blob" or path in objects:
                raise ValueError
            objects[path] = object_id
    except (UnicodeDecodeError, ValueError):
        raise ValueError("clean public API tree is malformed") from None
    if set(objects) != expected:
        raise ValueError("clean public API revision is incomplete")
    ordered = [(path, objects[path]) for path in sorted(expected)]
    batch = subprocess.run(
        ["git", "cat-file", "--batch"],
        cwd=root,
        input=b"".join(object_id + b"\n" for _, object_id in ordered),
        capture_output=True,
    )
    if batch.returncode:
        raise ValueError("clean public API blobs are not readable")
    committed, offset = {}, 0
    try:
        for path, expected_id in ordered:
            header_end = batch.stdout.index(b"\n", offset)
            object_id, kind, raw_size = batch.stdout[offset:header_end].split(b" ")
            size = int(raw_size)
            content_start, content_end = header_end + 1, header_end + 1 + size
            if object_id != expected_id or kind != b"blob" or batch.stdout[content_end:content_end + 1] != b"\n":
                raise ValueError
            committed[path] = batch.stdout[content_start:content_end]
            offset = content_end + 1
    except (ValueError, IndexError):
        raise ValueError("clean public API blob batch is malformed") from None
    if offset != len(batch.stdout):
        raise ValueError("clean public API blob batch has trailing data")
    return committed


def validate(root: Path, snapshot: object, revision: str, dirty: bool, abi_version: str) -> str:
    expected = paths(root)
    if not isinstance(snapshot, list) or {entry.get("path") for entry in snapshot} != expected:
        raise ValueError("public API snapshot is incomplete")
    for artifact in snapshot:
        exact(artifact, {"path", "sha256", "size_bytes", "kind"}, "public API artifact")
        if artifact["kind"] != "source":
            raise ValueError("public API snapshot must contain source artifacts")
        safe_file(root, artifact["path"], artifact["sha256"], artifact["size_bytes"], artifact["kind"])
    if not dirty:
        archived = _committed_sources(root, revision, expected)
        if any(archived[path] != (root / path).read_bytes() for path in expected):
            raise ValueError("clean public API artifact is not present at the claimed revision")
    manifest = (root / "libs/api/src/abi/manifest_flags.rs").read_text(encoding="utf-8")
    if f"pub const MANIFEST_VERSION: u8 = {abi_version};" not in manifest:
        raise ValueError("ABI version is not derived from the public source snapshot")
    return canonical_digest(snapshot)
