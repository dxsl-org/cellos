"""Validate the pinned Phase 04 prequalification catalog and runtime log shape."""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any

CATALOG_PATH = Path(__file__).with_name("catalog.json")
CATALOG_SHA256 = "cde941aa5430fb07f66e5052431ea1d5a1c1e9523b9973b220acad33f0c954d3"
EXPECTED_ROWS = tuple(f"P04-N{number:02d}" for number in range(1, 19))
EXPECTED_IDS = tuple(f"C3-ADM-{number:03d}" for number in range(1, 34))
EXPECTED_RUNTIME_IDS = (
    *(f"C3-ADM-{number:03d}" for number in range(1, 18)),
    "C3-ADM-032",
    "C3-ADM-033",
    *(f"C3-ADM-{number:03d}" for number in range(18, 32)),
)
PREQUALIFIED_ROWS = {"P04-N10", "P04-N11"}
CASE_LINE = re.compile(r"\[selftest\]\s+(C3-ADM-[A-Z0-9-]+):\s+(PASS|FAIL)\s*$")
AGGREGATE_PASS = "admission-core self-test PASS (fail-closed A/B floor model)"
AGGREGATE_FAIL = "admission-core self-test FAIL"
ANSI_ESCAPE = re.compile(rb"\x1b\[[0-9;]*m")


def digest_bytes(content: bytes) -> str:
    """Return the lowercase SHA-256 digest of exact bytes."""
    return hashlib.sha256(content).hexdigest()


def normalized_log(raw_log: bytes) -> str:
    """Strip only transport bytes before strict runtime-log validation."""
    try:
        return ANSI_ESCAPE.sub(b"", raw_log.replace(b"\0", b"")).decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError("runtime log is not UTF-8") from error


def _unique(values: list[str], label: str) -> set[str]:
    if len(values) != len(set(values)):
        raise ValueError(f"duplicate {label}")
    return set(values)


def validate_catalog_bytes(catalog_bytes: bytes) -> tuple[str, ...]:
    """Accept only the byte-pinned authoritative catalog and its exact mappings."""
    if digest_bytes(catalog_bytes) != CATALOG_SHA256:
        raise ValueError("catalog bytes differ from the authoritative pinned catalog")
    try:
        catalog = json.loads(catalog_bytes)
    except (TypeError, json.JSONDecodeError) as error:
        raise ValueError("catalog is not valid JSON") from error
    return _validate_catalog(catalog)


def _validate_catalog(catalog: dict[str, Any]) -> tuple[str, ...]:
    expected_keys = {"schema_version", "designation", "phase_status", "production_admission", "matrix", "executables"}
    if set(catalog) != expected_keys:
        raise ValueError("catalog fields are missing or unknown")
    if catalog["schema_version"] != 1 or catalog["designation"] != "PREQUALIFICATION_ONLY":
        raise ValueError("catalog is not PREQUALIFICATION_ONLY schema version 1")
    if catalog["phase_status"] != "BLOCKED" or catalog["production_admission"] != "DISABLED":
        raise ValueError("catalog attempts to qualify production admission")
    rows = catalog["matrix"]
    row_ids = [row.get("row_id") for row in rows]
    _unique(row_ids, "matrix row ID")
    if set(row_ids) != set(EXPECTED_ROWS):
        raise ValueError("mandatory Phase 04 matrix rows are missing or unknown")
    row_map = {row["row_id"]: row for row in rows}
    for row_id, row in row_map.items():
        if set(row) != {"row_id", "scenario", "required_result", "status", "executable_ids", "blocked_by"}:
            raise ValueError(f"{row_id} fields are missing or unknown")
        expected_status = "PREQUALIFICATION" if row_id in PREQUALIFIED_ROWS else "BLOCKED"
        if row["status"] != expected_status:
            raise ValueError(f"{row_id} cannot be promoted beyond {expected_status}")
        if not isinstance(row["scenario"], str) or not row["scenario"].strip():
            raise ValueError(f"{row_id} lacks a scenario")
        if not isinstance(row["required_result"], str) or not row["required_result"].strip():
            raise ValueError(f"{row_id} lacks a required result")
        if (row["status"] == "BLOCKED") != bool(row["blocked_by"]):
            raise ValueError(f"{row_id} has contradictory prerequisites")
        _unique(row["executable_ids"], f"{row_id} executable ID")
    executables = catalog["executables"]
    executable_ids = [case.get("id") for case in executables]
    _unique(executable_ids, "executable ID")
    if tuple(executable_ids) != EXPECTED_IDS:
        raise ValueError("compiled admission IDs are missing, unknown, or reordered")
    for case in executables:
        if set(case) != {"id", "name", "matrix_rows"} or not case["name"].strip():
            raise ValueError(f"{case.get('id')} fields or stable name are invalid")
        linked_rows = _unique(case["matrix_rows"], f"{case['id']} matrix row")
        reverse_rows = {row_id for row_id, row in row_map.items() if case["id"] in row["executable_ids"]}
        if not linked_rows or linked_rows != reverse_rows:
            raise ValueError(f"{case['id']} matrix mapping is inconsistent")
    referenced = {case_id for row in rows for case_id in row["executable_ids"]}
    if referenced != set(EXPECTED_IDS):
        raise ValueError("matrix references missing or unknown executable IDs")
    return tuple(executable_ids)


def validate_log(raw_log: bytes) -> tuple[str, ...]:
    """Validate 33 unique PASS cases in runtime order and one trailing aggregate PASS."""
    seen: list[str] = []
    terminators: list[int] = []
    lines = normalized_log(raw_log).splitlines()
    for line_number, line in enumerate(lines):
        failures = re.findall(r"\bFAIL\b", line)
        if failures and (len(failures) != 1 or re.search(r"\b0 FAIL\b", line) is None):
            raise ValueError("runtime log contains a nonzero or unscoped FAIL")
        if "admission-core self-test PASS" in line:
            occurrences = line.count(AGGREGATE_PASS)
            if occurrences == 0 or not line.rstrip().endswith(AGGREGATE_PASS):
                raise ValueError("malformed aggregate admission PASS terminator")
            terminators.extend([line_number] * occurrences)
        if "[selftest] C3-ADM-" not in line:
            continue
        match = CASE_LINE.search(line)
        if not match:
            raise ValueError("malformed admission self-test result line")
        case_id, result = match.groups()
        if case_id not in EXPECTED_IDS:
            raise ValueError(f"unknown runtime admission ID {case_id}")
        if result != "PASS":
            raise ValueError(f"runtime admission case {case_id} failed")
        if case_id in seen:
            raise ValueError(f"duplicate runtime admission ID {case_id}")
        seen.append(case_id)
    if tuple(seen) != EXPECTED_RUNTIME_IDS:
        raise ValueError("runtime admission PASS lines are missing or out of order")
    if len(terminators) != 1:
        raise ValueError("expected exactly one aggregate admission PASS terminator")
    last_case_line = max(index for index, line in enumerate(lines) if "[selftest] C3-ADM-" in line)
    if terminators[0] <= last_case_line:
        raise ValueError("aggregate admission PASS must follow the final case")
    return EXPECTED_RUNTIME_IDS
