#!/usr/bin/env python3
"""Phase 1 admission validator for the DEV_REFERENCE lane.

Deterministic, read-only report generator over an operator inventory
(`admission.schema.json`). The validator never probes hardware, calls the
network, or mutates state: it only checks that every named asset, the
dedicated AWS DEV account, and the pinned upstream time source are fully
recorded with on-hand evidence hashes, then emits exactly one of
`READY_FOR_PHASE_02` or `BLOCKED`.

Usage:
    python3 admission.py validate --inventory <inventory.json> \
        --evidence-dir <local-evidence-dir>

Exit codes: 0 READY_FOR_PHASE_02, 1 BLOCKED, 2 unusable input.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from admission_aws import (
    attachment_path as _attachment_path,
    aws_identity_problems as _aws_identity_problems,
    file_sha256 as _file_sha256,
)
from admission_schema import AdmissionError, load_json, validate_node

SCHEMA_PATH = Path(__file__).with_name("admission.schema.json")
EXPECTED_KINDS = (
    "visionfive2-board",
    "stm32h573i-dk",
    "slb9672-kit",
    "power-reset-isolation",
    "logic-analysis",
)
READY = "READY_FOR_PHASE_02"
BLOCKED = "BLOCKED"


def semantic_checks(inventory: dict, evidence_dir: Path, checks: list) -> None:
    def record(check_id: str, problems: list) -> None:
        if problems:
            checks.append({"id": check_id, "result": "fail", "detail": "; ".join(problems)})
        else:
            checks.append({"id": check_id, "result": "pass"})

    assets = inventory["assets"]
    kinds = sorted(asset["asset_kind"] for asset in assets)
    record(
        "asset-kind-set",
        [] if kinds == sorted(EXPECTED_KINDS)
        else [f"asset kinds {kinds} != exact required set {sorted(EXPECTED_KINDS)}"],
    )

    by_hash: dict = {}
    for asset in assets:
        for attachment in asset["attachment_hashes"]:
            by_hash.setdefault(attachment["sha256"], []).append(asset["asset_kind"])
    duplicates = sorted(h for h, owners in by_hash.items() if len(owners) > 1)
    record(
        "attachment-hash-uniqueness",
        [f"duplicate evidence sha256 across assets: {duplicates}"] if duplicates else [],
    )

    problems = []
    evidence_root = evidence_dir.resolve()
    for asset in assets:
        for attachment in asset["attachment_hashes"]:
            path = _attachment_path(evidence_root, attachment["name"])
            if path is None:
                problems.append(
                    f"{asset['asset_kind']}/{attachment['name']}: path must be relative, "
                    "canonical, and beneath evidence directory"
                )
                continue
            if not path.is_file():
                problems.append(f"{asset['asset_kind']}/{attachment['name']}: missing from evidence dir")
                continue
            if _file_sha256(path) != attachment["sha256"]:
                problems.append(f"{asset['asset_kind']}/{attachment['name']}: sha256 mismatch")
    record("evidence-attachments-present", problems)
    record("aws-read-only-identity", _aws_identity_problems(inventory["aws_dev_account"], evidence_dir))


def evaluate_inventory(inventory: dict, evidence_dir: Path) -> tuple[str, dict]:
    """Evaluate one already-loaded inventory without reopening its path."""
    if not evidence_dir.is_dir():
        raise AdmissionError(f"evidence directory not found: {evidence_dir}")
    schema = load_json(SCHEMA_PATH)

    checks = []
    schema_failures: list = []
    validate_node(inventory, schema, schema, "$", schema_failures)
    if schema_failures:
        for failure in sorted(schema_failures):
            checks.append({"id": "closed-schema", "result": "fail", "detail": failure})
    else:
        checks.append({"id": "closed-schema", "result": "pass"})
        semantic_checks(inventory, evidence_dir, checks)

    status = READY if all(c["result"] == "pass" for c in checks) else BLOCKED
    report = {
        "classification": "DEV_REFERENCE",
        "schema": "cellos-dev-admission-v1",
        "status": status,
        "checks": checks,
    }
    return status, report


def evaluate(inventory_path: Path, evidence_dir: Path) -> tuple[str, dict]:
    return evaluate_inventory(load_json(inventory_path), evidence_dir)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="DEV_REFERENCE admission validator")
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("validate", help="validate an operator inventory")
    run.add_argument("--inventory", required=True, help="path to inventory JSON")
    run.add_argument("--evidence-dir", required=True, help="directory holding hashed attachments")
    args = parser.parse_args(argv)

    try:
        status, report = evaluate(Path(args.inventory), Path(args.evidence_dir))
    except AdmissionError as exc:
        print(f"admission: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(report, sort_keys=True, indent=2))
    return 0 if status == READY else 1


if __name__ == "__main__":
    sys.exit(main())
