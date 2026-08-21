#!/usr/bin/env python3
"""Validate the authoritative app-tier ledger and optionally require C9 closure."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
from pathlib import Path

from app_tier_acceptance.source import ROOT
from app_tier_acceptance.validator import validate


def main() -> int:
    """Run validation with an optional trusted prior ledger baseline."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ledger", nargs="?", type=Path, default=ROOT / "docs/app-tier-acceptance-ledger.json")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--baseline-root", type=Path, help="trusted checkout containing baseline source and evidence")
    parser.add_argument("--require-fully-qualified", action="store_true")
    parser.add_argument("--as-of", help="RFC3339 UTC clock injection for deterministic validation")
    args = parser.parse_args()
    try:
        if bool(args.baseline) != bool(args.baseline_root):
            raise ValueError("--baseline and --baseline-root must be supplied together")
        baseline = json.loads(args.baseline.read_text()) if args.baseline else None
        as_of = None
        if args.as_of:
            as_of = dt.datetime.fromisoformat(args.as_of.replace("Z", "+00:00"))
            if as_of.tzinfo is None:
                raise ValueError("--as-of must include a UTC offset")
        result = validate(
            json.loads(args.ledger.read_text()),
            baseline=baseline,
            baseline_root=args.baseline_root,
            as_of=as_of,
        )
        if args.require_fully_qualified and result != "FULLY_QUALIFIED":
            raise ValueError("C9 is not FULLY_QUALIFIED")
    except (OSError, ValueError, TypeError, json.JSONDecodeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(f"PASS: C9={result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
