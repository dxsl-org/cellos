"""One-shot DynamoDB table-incarnation verification for allocator lineage."""

from collections.abc import Mapping
from typing import Any

from lineage import LineageContract

_CONFIGURATION_FAILURE = "invalid table identity verifier configuration"
_VERIFICATION_FAILURE = "table identity verification failed"


class TableIdentityError(RuntimeError):
    """Stable failure for unavailable or substituted DynamoDB table identities."""


class DynamoTableIdentityVerifier:
    """Verify allocator and lineage `TableId` pins without retry or fallback."""

    __slots__ = ("_contract", "_describe_table")

    def __init__(self, client: Any, contract: LineageContract) -> None:
        failed = False
        try:
            describe = client.describe_table
            if not callable(describe) or type(contract) is not LineageContract:
                raise TypeError("invalid table identity dependency")
        except Exception:
            failed = True
        if failed:
            raise TableIdentityError(_CONFIGURATION_FAILURE) from None
        self._describe_table = describe
        self._contract = contract

    def verify(self) -> LineageContract:
        """Return the admitted contract only after both live `TableId` values match."""
        failed = False
        try:
            expected = (
                (
                    self._contract.transition.allocator_table_name,
                    self._contract.transition.allocator_table_id,
                ),
                (self._contract.lineage_table_name, self._contract.lineage_table_id),
            )
            for name, table_id in expected:
                result = self._describe_table(TableName=name)
                if not isinstance(result, Mapping):
                    raise TypeError("invalid DescribeTable response")
                table = result.get("Table")
                metadata = result.get("ResponseMetadata")
                if not (
                    isinstance(table, Mapping)
                    and table.get("TableName") == name
                    and table.get("TableId") == table_id
                    and table.get("TableStatus") == "ACTIVE"
                    and table.get("DeletionProtectionEnabled") is True
                    and type(metadata) is dict
                    and type(metadata.get("HTTPStatusCode")) is int
                    and metadata["HTTPStatusCode"] == 200
                    and type(metadata.get("RequestId")) is str
                    and bool(metadata["RequestId"])
                ):
                    raise ValueError("DynamoDB table identity mismatch")
        except Exception:
            failed = True
        if failed:
            raise TableIdentityError(_VERIFICATION_FAILURE) from None
        return self._contract
