"""Exact-key DynamoDB receipt reads for durable retry recovery."""

from collections.abc import Callable, Mapping
from typing import Any

from protocol_models import SignedRequest
from request_protocol import encode_request
from receipt import Receipt, recover_receipt, request_receipt_key
from state_codec import decode_receipt


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


def read_committed_receipt(
    transact_get_items: Callable[..., Any],
    table_name: str,
    request: SignedRequest,
    configured_source_epoch: int,
    manifest_key_id: str,
) -> Receipt | None:
    """Read and validate the receipt bound to one exact signed request."""
    encode_request(request)
    key = request_receipt_key(request.authority_id, request.request_id)
    result = transact_get_items(TransactItems=[{"Get": {
        "TableName": table_name, "Key": {"pk": {"S": key}},
    }}])
    if (
        type(result) is not dict
        or set(result) != {"Responses", "ResponseMetadata"}
        or not operation_succeeded(result)
    ):
        raise ValueError("DynamoDB read did not return an exact success envelope")
    responses = result.get("Responses")
    if type(responses) is not list or len(responses) != 1:
        raise ValueError("DynamoDB read returned the wrong response count")
    entry = responses[0]
    if entry is None:
        return None
    if type(entry) is not dict or set(entry) != {"Item"}:
        raise ValueError("DynamoDB read did not return one exact item")
    receipt = decode_receipt(entry["Item"])
    recover_receipt(
        receipt,
        request,
        configured_source_epoch=configured_source_epoch,
        manifest_key_id=manifest_key_id,
    )
    return receipt
