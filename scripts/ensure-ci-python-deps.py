#!/usr/bin/env python3
"""Ensure the pinned packages in requirements-ci.txt are importable.

CI calls this instead of a bare `pip install`: when every direct `name==version`
pin is already installed (a self-hosted runner whose system Python ships the
pinned version and may have no pip at all) nothing is installed; otherwise it
runs `pip install --require-hashes`, so pip refuses any file whose sha256 is not
recorded in requirements-ci.txt, and its exit status is ours.

A pin followed by a `# via <package>` comment is a dependency of another pin. It
is hashed for pip but not required up front: distro packages need not register
their dependencies under the same names.
"""

import subprocess
import sys
from importlib import metadata
from pathlib import Path

REQUIREMENTS = Path(__file__).resolve().parent.parent / "requirements-ci.txt"


def direct_pins(text: str) -> list[tuple[str, str]]:
    """(name, version) of each requirement not marked `# via`.

    Every requirement must be an exact pin carrying at least one `--hash`, also
    on runners that install nothing: an unhashed pin would otherwise pass there
    and fail only where pip runs.
    """
    pins: list[list] = []
    logical = ""
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("#") or not line:
            if line.startswith("# via") and pins and not logical:
                pins[-1][2] = False
            continue
        logical += " " + line.removesuffix("\\")
        if line.endswith("\\"):
            continue
        spec, *options = logical.split()
        logical = ""
        name, sep, version = spec.partition("==")
        if not sep or not version:
            raise SystemExit(f"requirements-ci.txt: {spec!r} is not an exact `name==version` pin")
        if not any(opt.startswith("--hash=sha256:") for opt in options):
            raise SystemExit(f"requirements-ci.txt: {spec!r} has no --hash=sha256: entry")
        pins.append([name, version, True])
    return [(name, version) for name, version, direct in pins if direct]


def unmet_pins() -> list[str]:
    unmet = []
    for name, version in direct_pins(REQUIREMENTS.read_text()):
        try:
            installed = metadata.version(name)
        except metadata.PackageNotFoundError:
            installed = None
        if installed != version:
            unmet.append(f"{name}=={version} (installed: {installed or 'none'})")
    return unmet


def main() -> int:
    unmet = unmet_pins()
    if not unmet:
        print("python deps: all pins in requirements-ci.txt already installed")
        return 0
    print("python deps: installing " + "; ".join(unmet))
    cmd = [sys.executable, "-m", "pip", "install", "--user", "--quiet",
           "--require-hashes", "-r", str(REQUIREMENTS)]
    return subprocess.call(cmd)


if __name__ == "__main__":
    sys.exit(main())
