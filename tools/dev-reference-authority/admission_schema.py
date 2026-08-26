"""Closed JSON-schema subset used by the offline admission validator."""

from __future__ import annotations

import json
import math
import re
from pathlib import Path


class AdmissionError(ValueError):
    """The inventory or evidence directory cannot be processed at all."""


def _no_duplicate_keys(pairs):
    seen: dict = {}
    for key, value in pairs:
        if key in seen:
            raise AdmissionError(f"duplicate JSON key: {key}")
        seen[key] = value
    return seen

def _reject_nonfinite(value: str):
    raise AdmissionError(f"non-finite JSON number: {value}")


def load_json(path: Path):
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        raise AdmissionError(f"cannot read {path}: {exc}") from exc
    try:
        return json.loads(
            text,
            object_pairs_hook=_no_duplicate_keys,
            parse_constant=_reject_nonfinite,
        )
    except (json.JSONDecodeError, ValueError) as exc:
        raise AdmissionError(f"invalid JSON in {path}: {exc}") from exc


def _type_ok(value, expected: str) -> bool:
    if expected == "object":
        return isinstance(value, dict)
    if expected == "array":
        return isinstance(value, list)
    if expected == "string":
        return isinstance(value, str)
    if expected == "boolean":
        return isinstance(value, bool)
    if expected == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if expected == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    raise AdmissionError(f"unsupported schema type: {expected}")


def validate_node(value, schema: dict, root: dict, path: str, failures: list) -> None:
    """Validate one value against the closed schema subset, appending failures."""
    def fail(detail: str) -> None:
        failures.append(f"{path}: {detail}")

    if "$ref" in schema:
        node = root
        for part in schema["$ref"].lstrip("#/").split("/"):
            node = node[part]
        extra = {key: item for key, item in schema.items() if key != "$ref"}
        return validate_node(value, {**node, **extra}, root, path, failures)
    if "oneOf" in schema:
        probes = []
        for option in schema["oneOf"]:
            probe: list = []
            validate_node(value, option, root, path, probe)
            if not probe:
                return
            probes.extend(probe)
        fail("matches no allowed variant; e.g. " + "; ".join(sorted(probes)[:2]))
        return
    if "allOf" in schema:
        for branch in schema["allOf"]:
            validate_node(value, branch, root, path, failures)
    if "const" in schema and value != schema["const"]:
        fail(f"must equal {schema['const']!r}")
    if "enum" in schema and value not in schema["enum"]:
        fail(f"must be one of {schema['enum']!r}")
    if "type" in schema and not _type_ok(value, schema["type"]):
        fail(f"must be {schema['type']}")
        return
    if isinstance(value, str):
        if "pattern" in schema and not re.search(schema["pattern"], value):
            fail(f"does not match {schema['pattern']!r}")
        if len(value) < schema.get("minLength", 0):
            fail("must be non-empty")
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if isinstance(value, float) and not math.isfinite(value):
            fail("must be finite")
            return
        if "minimum" in schema and value < schema["minimum"]:
            fail(f"below minimum {schema['minimum']}")
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            fail("must not be empty")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            fail(f"must contain at most {schema['maxItems']} items")
        for index, item in enumerate(value):
            validate_node(item, schema.get("items", {}), root, f"{path}[{index}]", failures)
    if isinstance(value, dict):
        props = schema.get("properties", {})
        for required in schema.get("required", []):
            if required not in value:
                fail(f"missing required field {required!r}")
        if schema.get("additionalProperties") is False:
            for key in sorted(set(value) - set(props)):
                fail(f"unexpected field {key!r}")
        for key in sorted(set(value) & set(props)):
            validate_node(value[key], props[key], root, f"{path}.{key}", failures)
