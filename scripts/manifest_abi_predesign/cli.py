"""Thin command-line entrypoint for frozen predesign validation."""
from __future__ import annotations

import json
import sys

from .common import ARTIFACTS, PARENT_PLAN, PLAN, ROOT
from .report import validate_loaded, validate_parent_plan_text, validate_plan_text, validate_report


def main() -> int:
    if len(sys.argv) != 1:
        raise SystemExit("usage: validate-manifest-abi-predesign.py")
    corpus = json.loads((ARTIFACTS / "manifest-v1-v2-corpus.json").read_text(encoding="utf-8"))
    inventory = json.loads((ARTIFACTS / "manifest-consumer-inventory.json").read_text(encoding="utf-8"))
    matrix = json.loads((ARTIFACTS / "manifest-downgrade-matrix.json").read_text(encoding="utf-8"))
    report = json.loads((ARTIFACTS / "predesign-validation-report.json").read_text(encoding="utf-8"))
    validate_loaded(corpus, inventory, matrix, ROOT, scan=True)
    validate_report(report, corpus, inventory, matrix, ROOT)
    validate_plan_text(PLAN.read_text(encoding="utf-8"))
    validate_parent_plan_text(PARENT_PLAN.read_text(encoding="utf-8"))
    return 0


def run() -> None:
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"manifest ABI predesign validation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
