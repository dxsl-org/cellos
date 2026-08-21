"""Complete content-addressed snapshot of the in-repository Native SDK surface."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

from .checks import canonical_digest, exact, safe_file

ROOTS = (
    "libs/api/src/lib.rs",
    "libs/ostd/src/lib.rs",
    "libs/types/src/lib.rs",
    "libs/viui/src/lib.rs",
    "libs/viui-macros/src/lib.rs",
    "libs/viui/Cargo.toml",
    "libs/viui-macros/Cargo.toml",
)
FILE_MODULE = re.compile(r"^\s*(?:pub\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", re.MULTILINE)


def paths(root: Path) -> set[str]:
    """Resolve every file-backed module reachable from the public SDK roots."""
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
        for module in FILE_MODULE.findall(source_path.read_text(encoding="utf-8")):
            candidates = (module_root / f"{module}.rs", module_root / module / "mod.rs")
            resolved = next((candidate for candidate in candidates if (root / candidate).is_file()), None)
            if resolved is None:
                raise ValueError(f"public module {module} declared by {name} has no source file")
            pending.append(resolved)
    return found


def validate(root: Path, snapshot: object, revision: str, dirty: bool, abi_version: str) -> str:
    """Verify the exact public-module snapshot and return its aggregate digest."""
    expected = paths(root)
    if not isinstance(snapshot, list) or {entry.get("path") for entry in snapshot} != expected:
        raise ValueError("public API snapshot is incomplete")
    for artifact in snapshot:
        exact(artifact, {"path", "sha256", "size_bytes", "kind"}, "public API artifact")
        if artifact["kind"] != "source":
            raise ValueError("public API snapshot must contain source artifacts")
        safe_file(root, artifact["path"], artifact["sha256"], artifact["size_bytes"], artifact["kind"])
        if not dirty:
            committed = subprocess.run(
                ["git", "show", f"{revision}:{artifact['path']}"], cwd=root, capture_output=True
            )
            if committed.returncode or committed.stdout != (root / artifact["path"]).read_bytes():
                raise ValueError("clean public API artifact is not present at the claimed revision")
    manifest = (root / "libs/api/src/abi/manifest_flags.rs").read_text(encoding="utf-8")
    if f"pub const MANIFEST_VERSION: u8 = {abi_version};" not in manifest:
        raise ValueError("ABI version is not derived from the public source snapshot")
    return canonical_digest(snapshot)
