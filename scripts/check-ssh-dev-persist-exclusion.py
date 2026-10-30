#!/usr/bin/env python3
"""Keep KSSH development custody out of production-like KMS artifacts.

Sibling of check-production-relay-image.py (which this script never edits or
imports): it knows only the five closed ASCII markers of the opt-in SSH custody
features and proves which variant a KMS artifact is.

  --expect none     (default, production-like) the feature list must not name
                    ssh-dev-custody or ssh-dev-persist and the artifact must
                    contain none of the markers.
  --expect custody  (K8 I-T0 lane) ssh-dev-custody, not ssh-dev-persist, in the
                    feature list; the custody banner present; the dev root label
                    and the persist banner absent.
  --expect persist  (K8 I-T1 lane) ssh-dev-persist in the feature list; the
                    persist banner and the dev root label present; the custody
                    banner absent.

Exit 0 pass, 1 violation, 2 usage. The script can prove that a lane booted the
intended variant; it proves nothing about a different binary.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

FEATURE_CUSTODY = "ssh-dev-custody"
FEATURE_PERSIST = "ssh-dev-persist"
FEATURES = frozenset({FEATURE_CUSTODY, FEATURE_PERSIST})

ROOT_LABEL = b"CELLOS-KSSH-T1-DEV-ROOT-v1"
PERSIST_BANNER = b"DEV_POSTURE ssh-dev-persist"
CUSTODY_BANNER = b"DEV_POSTURE ssh-dev-custody"
# Closed ASCII marker names; the order is the report order.
MARKERS = (
    ROOT_LABEL,
    PERSIST_BANNER,
    CUSTODY_BANNER,
    FEATURE_PERSIST.encode("ascii"),
    FEATURE_CUSTODY.encode("ascii"),
)
CHUNK = 64 * 1024


def feature_set(value: str) -> set[str]:
    """Exact feature names, separated by commas and/or whitespace."""
    return {item for item in value.replace(",", " ").split() if item}


def scan_artifact(path: Path) -> tuple[list[str], set[bytes]]:
    """Return (errors, markers found), scanning in chunks with overlap."""
    if not path.is_file() or path.stat().st_size == 0:
        return [f"missing or empty artifact: {path}"], set()
    overlap = max(map(len, MARKERS)) - 1
    found: set[bytes] = set()
    previous = b""
    with path.open("rb") as artifact:
        while chunk := artifact.read(CHUNK):
            window = previous + chunk
            found.update(marker for marker in MARKERS if marker in window)
            previous = window[-overlap:]
    return [], found


def check(features: set[str], found: set[bytes], expect: str) -> list[str]:
    errors: list[str] = []

    def require(marker: bytes, present: bool) -> None:
        text = marker.decode("ascii")
        if present and marker not in found:
            errors.append(f"expected marker missing: {text}")
        if not present and marker in found:
            errors.append(f"forbidden marker present: {text}")

    if expect == "none":
        for name in sorted(features & FEATURES):
            errors.append(f"forbidden feature: {name}")
        for marker in MARKERS:
            require(marker, False)
    elif expect == "custody":
        if FEATURE_CUSTODY not in features:
            errors.append(f"expected feature missing: {FEATURE_CUSTODY}")
        if FEATURE_PERSIST in features:
            errors.append(f"forbidden feature: {FEATURE_PERSIST}")
        require(CUSTODY_BANNER, True)
        require(ROOT_LABEL, False)
        require(PERSIST_BANNER, False)
    elif expect == "persist":
        if FEATURE_PERSIST not in features:
            errors.append(f"expected feature missing: {FEATURE_PERSIST}")
        require(PERSIST_BANNER, True)
        require(ROOT_LABEL, True)
        require(CUSTODY_BANNER, False)
    return errors


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--kms-features", required=True)
    parser.add_argument("--kms-artifact", required=True, type=Path)
    parser.add_argument("--expect", choices=("none", "custody", "persist"), default="none")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    arguments = parse_args(argv)
    errors, found = scan_artifact(arguments.kms_artifact)
    if not errors:
        errors = check(feature_set(arguments.kms_features), found, arguments.expect)
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(f"PASS: --expect {arguments.expect}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
