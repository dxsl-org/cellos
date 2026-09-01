"""Exact DynamoDB head item and transaction conditions for allocator lineage."""

from collections.abc import Mapping
from typing import Any, NoReturn

from lineage import (
    LINEAGE_HEAD_KEY, LineageContract, LineageError, require_direct_child,
)

_ERROR = "invalid allocator lineage head"
_FIELDS = {"pk", "schema_version", "record_type", "transition"}


class LineageStateError(ValueError):
    """Stable rejection for a malformed or substituted lineage head item."""


def _fail() -> NoReturn:
    raise LineageStateError(_ERROR) from None


def encode_lineage_head(contract: LineageContract) -> dict[str, dict[str, Any]]:
    """Encode the exact expected active head for one admitted lineage contract."""
    if type(contract) is not LineageContract:
        _fail()
    return {
        "pk": {"S": LINEAGE_HEAD_KEY},
        "schema_version": {"N": "1"},
        "record_type": {"S": "lineage_head"},
        "transition": {"B": contract.encoded_transition},
    }


def require_lineage_head(item: Any, contract: LineageContract) -> None:
    """Reject unless *item* is the exact active head selected by *contract*."""
    if (
        type(item) is not dict
        or set(item) != _FIELDS
        or item != encode_lineage_head(contract)
        or not all(
            isinstance(value, Mapping) and len(value) == 1
            for value in item.values()
        )
    ):
        _fail()


def lineage_head_get(contract: LineageContract) -> dict[str, dict[str, Any]]:
    """Return one exact transactional read operation for the pinned lineage head."""
    if type(contract) is not LineageContract:
        _fail()
    return {"Get": {
        "TableName": contract.lineage_table_name,
        "Key": {"pk": {"S": LINEAGE_HEAD_KEY}},
    }}


def lineage_head_condition(contract: LineageContract) -> dict[str, dict[str, Any]]:
    """Return the exact cross-table condition that seals stale allocator branches."""
    item = encode_lineage_head(contract)
    names = {
        "#pk": "pk", "#sv": "schema_version",
        "#rt": "record_type", "#transition": "transition",
    }
    values = {
        ":pk": item["pk"], ":sv": item["schema_version"],
        ":rt": item["record_type"], ":transition": item["transition"],
    }
    return {"ConditionCheck": {
        "TableName": contract.lineage_table_name,
        "Key": {"pk": item["pk"]},
        "ConditionExpression": " AND ".join(
            f"{name} = :{name[1:]}" for name in names
        ),
        "ExpressionAttributeNames": names,
        "ExpressionAttributeValues": values,
    }}

def lineage_transition_update(
    previous: LineageContract,
    child: LineageContract,
) -> dict[str, dict[str, Any]]:
    """Return the exact compare-and-swap update from one admitted head to its child."""
    try:
        require_direct_child(previous, child)
    except LineageError:
        _fail()
    old = encode_lineage_head(previous)
    names = {
        "#pk": "pk", "#sv": "schema_version",
        "#rt": "record_type", "#transition": "transition",
    }
    values = {
        ":pk": old["pk"], ":sv": old["schema_version"],
        ":rt": old["record_type"], ":old": old["transition"],
        ":child": {"B": child.encoded_transition},
    }
    return {"Update": {
        "TableName": previous.lineage_table_name,
        "Key": {"pk": old["pk"]},
        "UpdateExpression": "SET #transition = :child",
        "ConditionExpression": (
            "#pk = :pk AND #sv = :sv AND #rt = :rt AND #transition = :old"
        ),
        "ExpressionAttributeNames": names,
        "ExpressionAttributeValues": values,
    }}
