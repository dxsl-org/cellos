#!/usr/bin/env python3
"""Read-only validator for frozen Manifest v1/v2 predesign artifacts."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from manifest_abi_predesign.cli import run
from manifest_abi_predesign.common import ROOT, canonical, digest, file_digest
from manifest_abi_predesign.inventory import scan_sources
from manifest_abi_predesign.report import (
    validate_loaded as _validate_loaded,
    validate_parent_plan_text,
    validate_plan_text,
    validate_report,
)
from manifest_abi_predesign.schema import validate_artifact_schemas, validate_schema


def validate_loaded(corpus: dict, inventory: dict, matrix: dict, root: Path = ROOT, scan: bool = False) -> None:
    """Compatibility facade preserving the testable validator surface."""
    _validate_loaded(corpus, inventory, matrix, root, scan, scan_sources)


if __name__ == "__main__":
    run()
