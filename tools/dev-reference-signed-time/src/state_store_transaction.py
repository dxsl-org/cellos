"""Exact cross-table transaction for one lineage-bound signed-time allocation."""

from typing import Any

from lineage import LineageContract
from lineage_state import lineage_head_condition


def build_write_transaction(
    contract: LineageContract,
    registration: dict[str, Any],
    prior: dict[str, Any],
    state: dict[str, Any],
    receipt: dict[str, Any],
) -> list[dict[str, Any]]:
    """Build lineage check, registration check, state CAS, and receipt creation."""
    registration_names = {
        "#pk": "pk", "#sv": "schema_version", "#rt": "record_type",
        "#di": "device_id", "#ai": "authority_id", "#key": "public_key_der",
        "#revoked": "revoked",
    }
    registration_values = {
        ":pk": registration["pk"], ":sv": registration["schema_version"],
        ":rt": registration["record_type"], ":di": registration["device_id"],
        ":ai": registration["authority_id"], ":key": registration["public_key_der"],
        ":revoked": {"BOOL": False},
    }
    state_names = {
        "#pk": "pk", "#sv": "schema_version", "#rt": "record_type",
        "#epoch": "source_epoch", "#sequence": "source_sequence",
        "#time": "last_unix_seconds",
    }
    state_values = {
        ":pk": prior["pk"], ":sv": prior["schema_version"],
        ":rt": prior["record_type"], ":epoch": prior["source_epoch"],
        ":sequence": prior["source_sequence"], ":time": prior["last_unix_seconds"],
    }
    table = contract.transition.allocator_table_name
    return [
        lineage_head_condition(contract),
        {"ConditionCheck": {
            "TableName": table, "Key": {"pk": registration["pk"]},
            "ConditionExpression": " AND ".join(
                f"{name} = :{name[1:]}" for name in registration_names
            ),
            "ExpressionAttributeNames": registration_names,
            "ExpressionAttributeValues": registration_values,
        }},
        {"Put": {
            "TableName": table, "Item": state,
            "ConditionExpression": " AND ".join(
                f"{name} = :{name[1:]}" for name in state_names
            ),
            "ExpressionAttributeNames": state_names,
            "ExpressionAttributeValues": state_values,
        }},
        {"Put": {
            "TableName": table, "Item": receipt,
            "ConditionExpression": "attribute_not_exists(#pk)",
            "ExpressionAttributeNames": {"#pk": "pk"},
        }},
    ]
