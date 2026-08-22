"""Downgrade-matrix invariants for frozen predesign artifacts."""
from __future__ import annotations

import itertools
from pathlib import Path

from .common import MATRIX_KEYS, ROW_KEYS, digest, exact_keys
from .state import validate_derived_source_state


def expected_result(pub: str, owner: str, version: str, route: str) -> str:
    if owner in ("ahead-or-conflicting", "floor-unavailable-or-invalid"):
        return "recovery-required-no-publication"
    if pub != "current" or owner == "stale-below-floor" or version == "unsupported-byte":
        return "deny-before-publication"
    if route in ("substituted-weaker-route", "required-route-unavailable-or-disabled"):
        return "deny-before-publication"
    if version == "future-symbolic":
        return "decision-required-phase08-blocked"
    return "preserve-existing-v1v2-policy"


def validate_matrix(doc: dict, root: Path) -> None:
    exact_keys(doc, MATRIX_KEYS, "matrix")
    validate_derived_source_state(doc, root, "matrix")
    dimensions, rows = doc["dimensions"], doc["rows"]
    wanted = set(itertools.product(dimensions["publisher_epoch_state"], dimensions["owner_generation_state"], dimensions["manifest_version_class"], dimensions["route_state"]))
    for row in rows:
        exact_keys(row, ROW_KEYS, f"row {row.get('threat_id')}")
    actual = {(row["publisher_epoch_state"], row["owner_generation_state"], row["manifest_version_class"], row["route_state"]) for row in rows}
    if len(rows) != 240 or actual != wanted:
        raise ValueError("matrix Cartesian coverage drift")
    for row in rows:
        key = (row["publisher_epoch_state"], row["owner_generation_state"], row["manifest_version_class"], row["route_state"])
        if row["expected_result"] != expected_result(*key):
            raise ValueError("downgrade invariant violated")
    hostile = doc["mandatory_hostile_tuples"]
    for item in hostile:
        exact_keys(item, ROW_KEYS | {"scenario"}, f"hostile {item.get('threat_id')}")
    expected_ids = [f"hostile-{index:02d}-{suffix}" for index, suffix in enumerate(("stale-resign", "owner-digest-replay", "old-provenance-digest", "unauthorized-key-rotation", "routing-byte-tamper", "both-owner-slots-stale", "owner-ahead", "floor-unavailable", "weaker-route-substitution", "symbolic-route-disabled", "unsupported-version-byte-03", "rollback-legacy-preservation", "different-final-elf"), 1)]
    if [item["threat_id"] for item in hostile] != expected_ids:
        raise ValueError("mandatory hostile tuple drift")
    different_final_elf = [item for item in hostile if item["artifact_binding"] == "different-final-elf"]
    if len(different_final_elf) != 1 or different_final_elf[0]["threat_id"] != "hostile-13-different-final-elf" or different_final_elf[0]["expected_result"] != "deny-before-publication":
        raise ValueError("different-final-elf hostile coverage drift")
    if digest(rows) != doc["matrix_sha256"]:
        raise ValueError("matrix aggregate drift")
