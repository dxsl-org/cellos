#!/usr/bin/env python3
"""Validate the byte-pinned authoritative Phase 04 prequalification catalog."""

from __future__ import annotations

import argparse
import sys

from admission_prequalification.validator import CATALOG_PATH, validate_catalog_bytes


def main() -> int:
    """Validate the authoritative catalog without accepting execution inputs."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    try:
        validate_catalog_bytes(CATALOG_PATH.read_bytes())
    except (OSError, TypeError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(
        "PASS: PREQUALIFICATION INFRASTRUCTURE COMPLETE; "
        "authoritative Phase 04 catalog pin matches; "
        "ADMISSIBLE EVIDENCE BLOCKED; production remains BLOCKED"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
