"""Closed JSON-schema validation for frozen predesign artifacts."""
from __future__ import annotations

import json
import math
import re
from pathlib import Path
from typing import Any

from .common import SCHEMA_FILES, canonical


def schema_error(path: str, message: str) -> None:
    raise ValueError(f"schema {path}: {message}")


def is_finite_json_number(value: Any) -> bool:
    return (isinstance(value, int) and not isinstance(value, bool)) or (
        isinstance(value, float) and math.isfinite(value)
    )


def validate_schema(value: Any, schema: dict, path: str = "$") -> None:
    """Validate the closed JSON-schema subset used by the frozen artifacts."""
    if "oneOf" in schema:
        matches = 0
        for candidate in schema["oneOf"]:
            try:
                validate_schema(value, candidate, path)
            except ValueError:
                continue
            matches += 1
        if matches != 1:
            schema_error(path, "must match exactly one oneOf branch")
        return
    if isinstance(value, float) and not math.isfinite(value):
        schema_error(path, "non-finite numbers are invalid")
    if "const" in schema and value != schema["const"]:
        schema_error(path, "does not equal const")
    if "enum" in schema and value not in schema["enum"]:
        schema_error(path, "is outside enum")
    if "type" in schema:
        expected = schema["type"]
        expected_types = (expected,) if isinstance(expected, str) else tuple(expected)
        type_matches = {
            "object": isinstance(value, dict), "array": isinstance(value, list),
            "string": isinstance(value, str),
            "integer": isinstance(value, int) and not isinstance(value, bool),
            "number": is_finite_json_number(value), "boolean": isinstance(value, bool),
            "null": value is None,
        }
        if not any(type_matches.get(expected_type, False) for expected_type in expected_types):
            schema_error(path, f"expected {expected}")
    if is_finite_json_number(value):
        if "minimum" in schema and value < schema["minimum"]:
            schema_error(path, f"is below minimum {schema['minimum']}")
        if "maximum" in schema and value > schema["maximum"]:
            schema_error(path, f"is above maximum {schema['maximum']}")
    if isinstance(value, str):
        if "minLength" in schema and len(value) < schema["minLength"]:
            schema_error(path, "is shorter than minLength")
        if "pattern" in schema and re.fullmatch(schema["pattern"], value) is None:
            schema_error(path, "does not match pattern")
    if isinstance(value, list):
        if "minItems" in schema and len(value) < schema["minItems"]:
            schema_error(path, "has too few items")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            schema_error(path, "has too many items")
        if schema.get("uniqueItems") and len({canonical(item) for item in value}) != len(value):
            schema_error(path, "has duplicate items")
        if "items" in schema:
            for index, item in enumerate(value):
                validate_schema(item, schema["items"], f"{path}[{index}]")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        missing = set(schema.get("required", ())) - set(value)
        if missing:
            schema_error(path, f"is missing {sorted(missing)}")
        if schema.get("additionalProperties") is False:
            unknown = set(value) - set(properties)
            if unknown:
                schema_error(path, f"has unknown keys {sorted(unknown)}")
        for key, child_schema in properties.items():
            if key in value:
                validate_schema(value[key], child_schema, f"{path}.{key}")


def validate_artifact_schemas(corpus: dict, inventory: dict, matrix: dict, root: Path) -> None:
    for name, document in {"corpus": corpus, "inventory": inventory, "matrix": matrix}.items():
        schema_path = root / ".agents/260822-phase08-manifest-predesign/artifacts" / SCHEMA_FILES[name]
        if not schema_path.is_file() or schema_path.is_symlink():
            raise ValueError(f"missing or unsafe {name} schema")
        validate_schema(document, json.loads(schema_path.read_text(encoding="utf-8")), f"${name}")
