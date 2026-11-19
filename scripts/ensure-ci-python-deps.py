#!/usr/bin/env python3
"""Ensure the pinned packages in requirements-ci.txt are importable.

CI calls this instead of a bare `pip install --user -r requirements-ci.txt`:
when every `name==version` pin is already installed (a self-hosted runner whose
system Python ships the pinned version and may have no pip at all) nothing is
installed; otherwise it runs that pip command and its exit status is ours.
"""

import subprocess
import sys
from importlib import metadata
from pathlib import Path

REQUIREMENTS = Path(__file__).resolve().parent.parent / "requirements-ci.txt"


def unmet_pins() -> list[str]:
    unmet = []
    for line in REQUIREMENTS.read_text().splitlines():
        spec = line.split("#", 1)[0].strip()
        if not spec:
            continue
        name, _, version = spec.partition("==")
        try:
            installed = metadata.version(name.strip())
        except metadata.PackageNotFoundError:
            installed = None
        if installed != version.strip():
            unmet.append(f"{spec} (installed: {installed or 'none'})")
    return unmet


def main() -> int:
    unmet = unmet_pins()
    if not unmet:
        print("python deps: all pins in requirements-ci.txt already installed")
        return 0
    print("python deps: installing " + "; ".join(unmet))
    cmd = [sys.executable, "-m", "pip", "install", "--user", "--quiet", "-r", str(REQUIREMENTS)]
    return subprocess.call(cmd)


if __name__ == "__main__":
    sys.exit(main())
