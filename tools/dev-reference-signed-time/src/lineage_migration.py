"""Exact restored-allocator epoch migration before lineage-head selection."""

from typing import Any

from allocation import AllocationState
from lineage import LineageContract, LineageError, require_direct_child
from lineage_state import LineageStateError
from state_codec import decode_allocation_state, encode_allocation_state


def _expected_migrated_state(
    previous: LineageContract,
    child: LineageContract,
    restored_state: AllocationState,
) -> AllocationState:
    try:
        require_direct_child(previous, child)
        if (
            type(restored_state) is not AllocationState
            or restored_state.source_epoch != previous.transition.source_epoch
        ):
            raise TypeError("invalid restored state")
        return AllocationState(
            child.transition.source_epoch,
            restored_state.source_sequence,
            restored_state.last_unix_seconds,
        )
    except (LineageError, TypeError):
        raise LineageStateError("invalid allocator lineage head") from None


def allocator_epoch_migration_update(
    previous: LineageContract,
    child: LineageContract,
    restored_state: AllocationState,
) -> dict[str, dict[str, Any]]:
    """Build the exact CAS that advances a restored source state to its child epoch."""
    migrated = _expected_migrated_state(previous, child, restored_state)
    old = encode_allocation_state(restored_state)
    names = {
        "#pk": "pk", "#sv": "schema_version", "#rt": "record_type",
        "#epoch": "source_epoch", "#sequence": "source_sequence",
        "#time": "last_unix_seconds",
    }
    values = {
        ":pk": old["pk"], ":sv": old["schema_version"],
        ":rt": old["record_type"], ":epoch": old["source_epoch"],
        ":sequence": old["source_sequence"], ":time": old["last_unix_seconds"],
        ":child": {"N": str(migrated.source_epoch)},
    }
    return {"Update": {
        "TableName": child.transition.allocator_table_name,
        "Key": {"pk": old["pk"]},
        "UpdateExpression": "SET #epoch = :child",
        "ConditionExpression": (
            "#pk = :pk AND #sv = :sv AND #rt = :rt AND #epoch = :epoch AND "
            "#sequence = :sequence AND #time = :time"
        ),
        "ExpressionAttributeNames": names,
        "ExpressionAttributeValues": values,
    }}

def require_completed_epoch_migration(
    previous: LineageContract,
    child: LineageContract,
    restored_state: AllocationState,
    item: Any,
) -> AllocationState:
    """Return the exact migrated state after an ambiguous CAS; reject every other item."""
    expected = _expected_migrated_state(previous, child, restored_state)
    try:
        actual = decode_allocation_state(item)
    except (TypeError, ValueError):
        raise LineageStateError("invalid allocator lineage head") from None
    if actual != expected:
        raise LineageStateError("invalid allocator lineage head") from None
    return actual
