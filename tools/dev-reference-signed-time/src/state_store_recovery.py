"""Exact-key DynamoDB receipt reads for durable retry recovery."""

from collections.abc import Callable, Mapping
from typing import Any

from lineage import LineageContract
from lineage_state import lineage_head_get, require_lineage_head
from protocol_models import SignedRequest
from receipt import Receipt, _recover_validated_receipt, request_receipt_key
from state_codec import (
    AuthorityRegistration, decode_receipt, encode_authority_registration,
)


def operation_succeeded(result: Any) -> bool:
    """Recognize the minimum exact successful DynamoDB metadata contract."""
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


def encode_active_registration(
    request: SignedRequest,
    registration: AuthorityRegistration,
) -> dict[str, dict[str, Any]]:
    """Validate and encode the exact active registration tuple."""
    if type(request) is not SignedRequest:
        raise TypeError("request has the wrong type")
    item = encode_authority_registration(registration)
    if registration.revoked or (
        registration.device_id,
        registration.authority_id,
        registration.public_key_der,
    ) != (request.device_id, request.authority_id, request.authority_pubkey):
        raise ValueError("registration is not active for request")
    return item


def _read_committed_receipt(
    transact_get_items: Callable[..., Any],
    contract: LineageContract,
    request: SignedRequest,
    canonical_request: bytes,
) -> Receipt | None:
    """Read and validate the receipt bound to one exact signed request."""
    key = request_receipt_key(request.authority_id, request.request_id)
    result = transact_get_items(TransactItems=[
        lineage_head_get(contract),
        {"Get": {
            "TableName": contract.transition.allocator_table_name,
            "Key": {"pk": {"S": key}},
        }},
    ])
    if (
        type(result) is not dict
        or set(result) != {"Responses", "ResponseMetadata"}
        or not operation_succeeded(result)
    ):
        raise ValueError("DynamoDB read did not return an exact success envelope")
    responses = result.get("Responses")
    if type(responses) is not list or len(responses) != 2:
        raise ValueError("DynamoDB read returned the wrong response count")
    head, entry = responses
    if type(head) is not dict or set(head) != {"Item"}:
        raise ValueError("DynamoDB read did not return the lineage head")
    require_lineage_head(head["Item"], contract)
    if entry is None:
        return None
    if type(entry) is not dict or set(entry) != {"Item"}:
        raise ValueError("DynamoDB read did not return one exact receipt")
    receipt = decode_receipt(entry["Item"])
    _recover_validated_receipt(
        receipt,
        request,
        canonical_request,
        configured_source_epoch=contract.transition.source_epoch,
        manifest_key_id=contract.transition.response_key_id,
    )
    return receipt
