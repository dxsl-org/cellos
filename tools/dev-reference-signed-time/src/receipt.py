"""Pure immutable receipt construction and exact retry recovery."""

import hashlib
import hmac
from dataclasses import dataclass
from typing import Any

from allocation import AllocationResult, AllocationState
from protocol import response_signing_bytes
from protocol_errors import ProtocolError
from protocol_models import MAX_UINT64, SOURCE_ID, SignedRequest, UnsignedResponse
from request_protocol import encode_request

SOURCE_STATE_KEY = f"source#{SOURCE_ID}/state"
_AUTHORITY_ID_BYTES = 32
_REQUEST_ID_BYTES = 16
_DIGEST_BYTES = 32


class ReceiptError(ValueError):
    """Stable local failure whose message contains only a value-free code."""

    __slots__ = ("code",)

    def __init__(self, code: str) -> None:
        self.code = code
        super().__init__(code)


@dataclass(frozen=True, slots=True)
class Receipt:
    """Immutable canonical-request digest and validated response labels 1..14."""

    request_digest: bytes
    response: UnsignedResponse


def _fail(code: str) -> None:
    raise ReceiptError(code)


def _exact_bytes(value: Any, length: int) -> bool:
    return type(value) is bytes and len(value) == length


def _uint64(value: Any) -> bool:
    return type(value) is int and 0 <= value <= MAX_UINT64


def authority_registration_key(authority_id: bytes) -> str:
    """Return ``authority#<32-byte-lowerhex>/registration`` exactly."""
    if not _exact_bytes(authority_id, _AUTHORITY_ID_BYTES):
        _fail("invalid-authority-id")
    return f"authority#{authority_id.hex()}/registration"


def request_receipt_key(authority_id: bytes, request_id: bytes) -> str:
    """Return ``request#<authority-lowerhex>/<request-lowerhex>`` exactly."""
    if not _exact_bytes(authority_id, _AUTHORITY_ID_BYTES):
        _fail("invalid-authority-id")
    if not _exact_bytes(request_id, _REQUEST_ID_BYTES):
        _fail("invalid-request-id")
    return f"request#{authority_id.hex()}/{request_id.hex()}"


def _valid_response(response: Any) -> bool:
    if type(response) is not UnsignedResponse:
        return False
    try:
        response_signing_bytes(response)
    except ProtocolError:
        return False
    return True


def construct_receipt(allocation: AllocationResult) -> Receipt:
    """Freeze the exact validated response and full canonical-request digest."""
    if type(allocation) is not AllocationResult:
        _fail("invalid-allocation")
    state = allocation.state
    if type(state) is not AllocationState:
        _fail("invalid-allocation")
    if not (
        _uint64(state.source_epoch)
        and _uint64(state.source_sequence)
        and _uint64(state.last_unix_seconds)
    ):
        _fail("invalid-allocation")
    if not _exact_bytes(allocation.request_digest, _DIGEST_BYTES):
        _fail("invalid-allocation")
    if not _valid_response(allocation.response):
        _fail("invalid-allocation")
    response = allocation.response
    if (state.source_epoch, state.source_sequence, state.last_unix_seconds) != (
        response.source_epoch, response.source_sequence, response.unix_seconds,
    ):
        _fail("invalid-allocation")
    return Receipt(allocation.request_digest, response)


def recover_receipt(
    receipt: Receipt | None,
    request: SignedRequest,
    *,
    configured_source_epoch: int,
    manifest_key_id: str,
) -> UnsignedResponse:
    """Return unchanged committed labels only for one exact authenticated retry."""
    if receipt is None:
        _fail("missing-receipt")
    if type(receipt) is not Receipt:
        _fail("malformed-receipt")
    if not _exact_bytes(receipt.request_digest, _DIGEST_BYTES):
        _fail("malformed-receipt")
    if not _valid_response(receipt.response):
        _fail("malformed-receipt")
    if type(request) is not SignedRequest:
        _fail("invalid-request")
    invalid_request = False
    try:
        canonical_request = encode_request(request)
    except ProtocolError:
        invalid_request = True
    if invalid_request:
        _fail("invalid-request")
    digest = hashlib.sha256(canonical_request).digest()
    if not hmac.compare_digest(receipt.request_digest, digest):
        _fail("request-digest-mismatch")
    if not _uint64(configured_source_epoch):
        _fail("invalid-source-epoch")
    if type(manifest_key_id) is not str or not manifest_key_id:
        _fail("invalid-key-id")
    response = receipt.response
    request_tuple = (
        request.device_id,
        request.authority_id,
        request.boot_epoch,
        request.request_id,
        request.purpose,
        request.nonce,
    )
    response_tuple = (
        response.device_id,
        response.authority_id,
        response.boot_epoch,
        response.request_id,
        response.purpose,
        response.nonce,
    )
    if response_tuple != request_tuple:
        _fail("receipt-mismatch")
    if response.source_epoch != configured_source_epoch:
        _fail("receipt-mismatch")
    if response.key_id != manifest_key_id:
        _fail("receipt-mismatch")
    return response
