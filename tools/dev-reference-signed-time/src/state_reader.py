"""Fail-closed transactional read of allocation authority and source state."""

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

from allocation import AllocationState
from request_protocol import encode_request
from protocol_models import MAX_UINT64, SignedRequest
from receipt import SOURCE_STATE_KEY, authority_registration_key
from state_codec import (
    AuthorityRegistration,
    decode_allocation_state,
    decode_authority_registration,
)

_CONFIGURATION_FAILURE = "invalid state reader configuration"
_READER_FAILURE = "state reader operation failed"


class ReaderError(RuntimeError):
    """Stable value-free failure at the transactional read boundary."""


@dataclass(frozen=True, slots=True)
class StateSnapshot:
    """One immutable, transactionally consistent pre-allocation snapshot."""

    registration: AuthorityRegistration
    state: AllocationState


def _successful_envelope(result: Any) -> bool:
    if not isinstance(result, Mapping):
        return False
    metadata = result.get("ResponseMetadata")
    return (
        type(metadata) is dict
        and type(metadata.get("HTTPStatusCode")) is int
        and metadata["HTTPStatusCode"] == 200
        and type(metadata.get("RequestId")) is str
        and bool(metadata["RequestId"])
    )


class DynamoStateReader:
    """Load one exact authority registration and allocator state transactionally."""

    __slots__ = ("_configured_source_epoch", "_table_name", "_transact_get_items")

    def __init__(self, client: Any, table_name: str, configured_source_epoch: int) -> None:
        failed = False
        try:
            transact_get_items = client.transact_get_items
            if not callable(transact_get_items):
                raise TypeError("DynamoDB transaction read is not callable")
            if type(table_name) is not str or not table_name:
                raise ValueError("invalid table name")
            if not (
                type(configured_source_epoch) is int
                and 0 <= configured_source_epoch <= MAX_UINT64
            ):
                raise ValueError("invalid source epoch")
        except Exception:
            failed = True
        if failed:
            raise ReaderError(_CONFIGURATION_FAILURE)
        self._transact_get_items = transact_get_items
        self._table_name = table_name
        self._configured_source_epoch = configured_source_epoch

    def load_snapshot(self, request: SignedRequest) -> StateSnapshot:
        """Revalidate ``request`` and load its exact authority and source state."""
        failed = False
        try:
            if type(request) is not SignedRequest:
                raise TypeError("request has the wrong type")
            encode_request(request)
            transaction = [
                {"Get": {
                    "TableName": self._table_name,
                    "Key": {"pk": {"S": authority_registration_key(request.authority_id)}},
                }},
                {"Get": {
                    "TableName": self._table_name,
                    "Key": {"pk": {"S": SOURCE_STATE_KEY}},
                }},
            ]
            result = self._transact_get_items(TransactItems=transaction)
            if not _successful_envelope(result):
                raise ValueError("DynamoDB read did not return a success envelope")
            responses = result.get("Responses")
            if type(responses) is not list or len(responses) != 2:
                raise ValueError("DynamoDB read returned the wrong response count")
            first, second = responses
            if not all(
                isinstance(entry, Mapping) and set(entry) == {"Item"}
                for entry in (first, second)
            ):
                raise ValueError("DynamoDB read did not return two exact items")
            registration = decode_authority_registration(first["Item"])
            state = decode_allocation_state(second["Item"])
            if registration.revoked or (
                registration.device_id,
                registration.authority_id,
                registration.public_key_der,
            ) != (request.device_id, request.authority_id, request.authority_pubkey):
                raise ValueError("registration does not authorize request")
            if state.source_epoch != self._configured_source_epoch:
                raise ValueError("allocation state has the wrong source epoch")
            snapshot = StateSnapshot(registration, state)
        except Exception:
            failed = True
        if failed:
            raise ReaderError(_READER_FAILURE)
        return snapshot
