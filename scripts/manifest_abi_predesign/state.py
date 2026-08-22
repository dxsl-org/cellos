"""Revision and live-source identity validation."""
from __future__ import annotations

import re
from pathlib import Path

from .common import (
    DERIVED_SOURCE_STATE_KEYS,
    IMMUTABLE_BASE_REVISION,
    PHASE08_ARTIFACT_PREFIX,
    SOURCE_INPUT_KEYS,
    digest,
    exact_keys,
    file_digest,
)


def validate_derived_source_state(doc: dict, root: Path, label: str) -> None:
    if doc["base_revision"] != IMMUTABLE_BASE_REVISION or not re.fullmatch(r"[0-9a-f]{40}", doc["base_revision"]):
        raise ValueError(f"{label} immutable base revision disagreement")
    state = doc["derived_source_state"]
    exact_keys(state, DERIVED_SOURCE_STATE_KEYS, f"{label} derived source state")
    if state["hash_algorithm"] != "sha256":
        raise ValueError(f"{label} derived source-state algorithm drift")
    inputs = state["inputs"]
    if not inputs or inputs != sorted(inputs, key=lambda item: item["path"]):
        raise ValueError(f"{label} source inputs are not nonempty/path sorted")
    if digest({"hash_algorithm": state["hash_algorithm"], "inputs": inputs}) != state["derived_source_state_sha256"]:
        raise ValueError(f"{label} derived source-state digest drift")
    seen = set()
    for item in inputs:
        exact_keys(item, SOURCE_INPUT_KEYS, f"{label} source input")
        relative = Path(item["path"])
        if relative.is_absolute() or ".." in relative.parts or item["path"].startswith(PHASE08_ARTIFACT_PREFIX) or not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]):
            raise ValueError(f"{label} unsafe or self-referential source input")
        if item["path"] in seen:
            raise ValueError(f"{label} duplicate source input")
        seen.add(item["path"])
        path = root / relative
        if not path.is_file() or path.is_symlink() or file_digest(path) != item["sha256"]:
            raise ValueError(f"{label} source input content drift")


def validate_common_base_revision(*documents: dict) -> None:
    if {document["base_revision"] for document in documents} != {IMMUTABLE_BASE_REVISION}:
        raise ValueError("artifact immutable base revision disagreement")
