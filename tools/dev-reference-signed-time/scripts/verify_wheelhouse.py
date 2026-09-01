#!/usr/bin/env python3
"""Verify one complete flat wheelhouse against its reviewed SHA-256 index."""

import hashlib
import re
import sys
from pathlib import Path

_LINE = re.compile(r"([0-9a-f]{64})  ([A-Za-z0-9_.+-]+\.whl)").fullmatch


def _fail() -> None:
    raise SystemExit("wheelhouse verification failed")


def verify_wheelhouse(directory: Path) -> None:
    """Require exact regular wheel files and matching lowercase SHA-256 entries."""
    index = directory / "SHA256SUMS"
    if not directory.is_dir() or not index.is_file() or index.is_symlink():
        _fail()
    expected: dict[str, str] = {}
    try:
        lines = index.read_text(encoding="ascii").splitlines()
        for line in lines:
            match = _LINE(line)
            if match is None or match.group(2) in expected:
                _fail()
            expected[match.group(2)] = match.group(1)
        wheel_entries = [
            path for path in directory.iterdir() if path.name.endswith(".whl")
        ]
        if any(not path.is_file() or path.is_symlink() for path in wheel_entries):
            _fail()
        wheels = {path.name: path for path in wheel_entries}
        if not expected or set(expected) != set(wheels):
            _fail()
        for name, digest in expected.items():
            if hashlib.sha256(wheels[name].read_bytes()).hexdigest() != digest:
                _fail()
    except (OSError, UnicodeError):
        _fail()


def main(arguments: list[str]) -> None:
    """Accept exactly one wheelhouse directory or terminate without detail."""
    if len(arguments) != 1:
        _fail()
    verify_wheelhouse(Path(arguments[0]))


if __name__ == "__main__":
    main(sys.argv[1:])
