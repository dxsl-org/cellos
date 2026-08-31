from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterable


TEMPLATE_PATH = Path(__file__).resolve().parents[1] / "template.yaml"


def load_template() -> dict[str, Any]:
    with TEMPLATE_PATH.open("r", encoding="utf-8") as source:
        value = json.load(source)
    if not isinstance(value, dict):
        raise AssertionError("template root must be an object")
    return value


TEMPLATE = load_template()
RESOURCES = TEMPLATE["Resources"]


def resource(name: str) -> dict[str, Any]:
    return RESOURCES[name]


def statements(document: dict[str, Any]) -> list[dict[str, Any]]:
    value = document["Statement"]
    return value if isinstance(value, list) else [value]


def inline_statements(role_name: str) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []
    for policy in resource(role_name)["Properties"].get("Policies", []):
        result.extend(statements(policy["PolicyDocument"]))
    return result


def actions(statement: dict[str, Any]) -> set[str]:
    value = statement["Action"]
    return {value} if isinstance(value, str) else set(value)


def allow_statements() -> Iterable[tuple[str, dict[str, Any]]]:
    for name, item in RESOURCES.items():
        properties = item.get("Properties", {})
        documents = []
        if "KeyPolicy" in properties:
            documents.append(properties["KeyPolicy"])
        if "PolicyDocument" in properties:
            documents.append(properties["PolicyDocument"])
        documents.extend(
            policy["PolicyDocument"] for policy in properties.get("Policies", [])
        )
        if "AssumeRolePolicyDocument" in properties:
            documents.append(properties["AssumeRolePolicyDocument"])
        for document in documents:
            for statement in statements(document):
                if statement.get("Effect") == "Allow":
                    yield name, statement
