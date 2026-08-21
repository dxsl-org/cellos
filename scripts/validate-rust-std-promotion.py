#!/usr/bin/env python3
"""Validate one synthetic rust-std benchmark fixture; never mint promotion evidence."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from rust_std_promotion.validator import canonical_bytes, load_and_validate


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture", type=Path)
    args = parser.parse_args()
    try:
        result = load_and_validate(args.fixture)
        payload = canonical_bytes(result.report)
    except (OSError, UnicodeError, ValueError) as error:
        payload = canonical_bytes({
            "cells": [], "fixture_only": True, "overall_status": "INVALID",
            "promotion_eligible": False, "reasons": [f"input:{error.__class__.__name__}"],
        })
        result_code = 2
    else:
        result_code = result.exit_code
    sys.stdout.buffer.write(payload)
    return result_code


if __name__ == "__main__":
    raise SystemExit(main())
