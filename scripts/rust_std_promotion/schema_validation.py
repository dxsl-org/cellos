"""Small fail-closed validator for the JSON Schema features used by fixtures."""
from __future__ import annotations

import json
import re
from datetime import datetime
from typing import Any


_DATETIME = re.compile(
    r"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$"
)


class SchemaError(ValueError):
    """The document does not conform to the bundled schema."""


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def _equal(left: Any, right: Any) -> bool:
    return _canonical(left) == _canonical(right)


def reject_nonfinite(value: str) -> None:
    raise ValueError(f"non-finite JSON number: {value}")


def _date_time(value: str) -> bool:
    match = _DATETIME.fullmatch(value)
    if match is None:
        return False
    try:
        datetime.fromisoformat(value[:-1] + "+00:00" if value.endswith("Z") else value)
    except ValueError:
        return False
    return True


def _fail(path: str, keyword: str) -> None:
    raise SchemaError(f"schema:{path}:{keyword}")


def _validate(value: Any, rule: dict[str, Any], root: dict[str, Any], path: str) -> None:
    reference = rule.get("$ref")
    if reference is not None:
        prefix = "#/$defs/"
        if not isinstance(reference, str) or not reference.startswith(prefix):
            _fail(path, "ref")
        _validate(value, root["$defs"][reference[len(prefix):]], root, path)
        return

    expected_type = rule.get("type")
    matches_type = {
        "object": type(value) is dict,
        "array": type(value) is list,
        "string": type(value) is str,
        "integer": type(value) is int,
        "boolean": type(value) is bool,
    }
    if expected_type is not None and not matches_type.get(expected_type, False):
        _fail(path, "type")
    if "const" in rule and not _equal(value, rule["const"]):
        _fail(path, "const")
    if "enum" in rule and not any(_equal(value, member) for member in rule["enum"]):
        _fail(path, "enum")

    if expected_type == "object":
        required = rule.get("required", [])
        if any(key not in value for key in required):
            _fail(path, "required")
        properties = rule.get("properties", {})
        if rule.get("additionalProperties") is False and any(key not in properties for key in value):
            _fail(path, "additionalProperties")
        for key, child in properties.items():
            if key in value:
                _validate(value[key], child, root, f"{path}.{key}")
    elif expected_type == "array":
        if len(value) < rule.get("minItems", 0):
            _fail(path, "minItems")
        if rule.get("uniqueItems") and len({_canonical(item) for item in value}) != len(value):
            _fail(path, "uniqueItems")
        if "items" in rule:
            for index, item in enumerate(value):
                _validate(item, rule["items"], root, f"{path}[{index}]")
    elif expected_type == "string":
        if len(value) < rule.get("minLength", 0):
            _fail(path, "minLength")
        if "pattern" in rule and re.search(rule["pattern"], value) is None:
            _fail(path, "pattern")
        if rule.get("format") == "date-time" and not _date_time(value):
            _fail(path, "format")
    elif expected_type == "integer":
        if "minimum" in rule and value < rule["minimum"]:
            _fail(path, "minimum")
        if "maximum" in rule and value > rule["maximum"]:
            _fail(path, "maximum")


def validate_schema(document: Any, schema: dict[str, Any]) -> None:
    """Validate exactly the schema keyword subset present in the bundled schema."""
    _validate(document, schema, schema, "$")
