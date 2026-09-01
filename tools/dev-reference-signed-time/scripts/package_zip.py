#!/usr/bin/env python3
"""Create one byte-deterministic Lambda ZIP from a prepared staging tree."""

import base64
import hashlib
import os
import sys
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

_TIMESTAMP = (1980, 1, 1, 0, 0, 0)


def _fail(message: str) -> None:
    raise SystemExit(message)


def package_tree(source: Path, output: Path) -> bytes:
    """Write sorted normalized files from ``source`` and return SHA-256 bytes."""
    if not source.is_dir() or output.exists() or not output.parent.is_dir():
        _fail("invalid package paths")
    files = sorted(path for path in source.rglob("*") if path.is_file())
    if not files or not any(
        path.relative_to(source).as_posix() == "manifest.json" for path in files
    ):
        _fail("staging tree is incomplete")
    temporary = output.with_name(output.name + ".tmp")
    try:
        with ZipFile(temporary, "w", ZIP_DEFLATED, compresslevel=9) as archive:
            for path in files:
                relative = path.relative_to(source).as_posix()
                if "__pycache__" in path.parts or relative.endswith((".pyc", ".pyo")):
                    _fail("staging tree contains generated Python cache")
                info = ZipInfo(relative, _TIMESTAMP)
                info.compress_type = ZIP_DEFLATED
                info.create_system = 3
                info.external_attr = (0o100644 & 0xFFFF) << 16
                archive.writestr(info, path.read_bytes(), compresslevel=9)
        os.replace(temporary, output)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise
    return hashlib.sha256(output.read_bytes()).digest()


def main(arguments: list[str]) -> None:
    """Accept exactly ``STAGING OUTPUT`` or terminate without an artifact."""
    if len(arguments) != 2:
        _fail("usage: package_zip.py STAGING OUTPUT")
    digest = package_tree(Path(arguments[0]), Path(arguments[1]))
    print(f"UnsignedZipSha256Base64={base64.b64encode(digest).decode('ascii')}")
    print(f"UnsignedZipSha256Hex={digest.hex()}")


if __name__ == "__main__":
    main(sys.argv[1:])
