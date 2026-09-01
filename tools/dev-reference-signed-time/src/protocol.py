"""Public typed API for strict CellOS DEV_REFERENCE signed-time responses."""

import hashlib
from typing import Any

import cbor_codec
import request_protocol
from protocol_crypto import CryptoError, parse_p256_der_signature, verify_p256_digest
from protocol_errors import ProtocolError
from protocol_models import (
    MAX_RESPONSE_BYTES, MAX_UNSIGNED_RESPONSE_BYTES, MAX_UINT64, PROTOCOL_VERSION,
    SIGNING_ALGORITHM, SOURCE_ID, VALID_PURPOSES, SignedResponse, UnsignedRequest,
    UnsignedResponse,
)


def _bytes(value: Any, length: int, name: str) -> None:
    if type(value) is not bytes or len(value) != length:
        raise ProtocolError(f"{name} must be exactly {length} bytes")


def _uint(value: Any, name: str) -> None:
    if type(value) is not int or not 0 <= value <= MAX_UINT64:
        raise ProtocolError(f"{name} must be a uint64")


def _response_map(response: UnsignedResponse, signature: bytes | None = None) -> dict[int, Any]:
    result = {
        1: PROTOCOL_VERSION, 2: SOURCE_ID, 3: response.source_epoch,
        4: response.source_sequence, 5: response.unix_seconds, 6: response.expires_at,
        7: response.device_id, 8: response.authority_id, 9: response.boot_epoch,
        10: response.request_id, 11: response.purpose, 12: response.nonce,
        13: response.key_id, 14: SIGNING_ALGORITHM,
    }
    if signature is not None:
        result[15] = signature
    return result

def _dump_response(response: UnsignedResponse, signature: bytes | None = None) -> bytes:
    try:
        encoded = cbor_codec.dumps(_response_map(response, signature))
    except cbor_codec.CborError as exc:
        raise ProtocolError(str(exc)) from exc
    if len(encoded) > (MAX_RESPONSE_BYTES if signature is not None else MAX_UNSIGNED_RESPONSE_BYTES):
        raise ProtocolError("response exceeds size limit")
    return encoded

def _validate_response(response: UnsignedResponse) -> None:
    if not isinstance(response, UnsignedResponse):
        raise ProtocolError("response must be an UnsignedResponse")
    for name in ("source_epoch", "source_sequence", "unix_seconds", "expires_at", "boot_epoch"):
        _uint(getattr(response, name), name)
    for name, length in (("device_id", 32), ("authority_id", 32), ("request_id", 16), ("nonce", 32)):
        _bytes(getattr(response, name), length, name)
    if type(response.purpose) is not int or response.purpose not in VALID_PURPOSES:
        raise ProtocolError("purpose must be 1, 2, or 3")
    if type(response.key_id) is not str or not response.key_id:
        raise ProtocolError("key_id must be non-empty text")
    if not response.unix_seconds < response.expires_at <= response.unix_seconds + 60:
        raise ProtocolError("response expiry must be 1 through 60 seconds after its time")


def response_signing_bytes(response: UnsignedResponse) -> bytes:
    """Return canonical labels 1..14 whose SHA-256 digest is signed by KMS."""
    _validate_response(response)
    return _dump_response(response)


def response_signing_digest(response: UnsignedResponse) -> bytes:
    """Return the KMS ``MessageType=DIGEST`` SHA-256 input for ``ECDSA_SHA_256``."""
    return hashlib.sha256(response_signing_bytes(response)).digest()


def encode_response(response: SignedResponse) -> bytes:
    """Encode a strict low-S response within the frozen wire-size bound."""
    if not isinstance(response, SignedResponse):
        raise ProtocolError("response must be a SignedResponse")
    _validate_response(response)
    try:
        parse_p256_der_signature(response.signature)
    except CryptoError as exc:
        raise ProtocolError(str(exc)) from exc
    encoded = _dump_response(response, response.signature)
    return encoded


def decode_response(data: bytes, public_key_der: bytes, expected_key_id: str, expected_source_epoch: int,
                    request: UnsignedRequest) -> SignedResponse:
    """Decode, bind to source epoch, request, and key ID, then verify P-256."""
    _uint(expected_source_epoch, "expected_source_epoch")
    try:
        value = cbor_codec.loads(data, max_size=MAX_RESPONSE_BYTES)
    except cbor_codec.CborError as exc:
        raise ProtocolError(str(exc)) from exc
    if type(value) is not dict or set(value) != set(range(1, 16)):
        raise ProtocolError("response map must contain exactly labels 1 through 15")
    if type(value[1]) is not int or value[1] != PROTOCOL_VERSION or value[2] != SOURCE_ID:
        raise ProtocolError("unsupported response schema or source")
    if type(value[14]) is not str or value[14] != SIGNING_ALGORITHM:
        raise ProtocolError("unsupported response signing algorithm")
    response = SignedResponse(*[value[label] for label in range(3, 14)], value[15])
    _validate_response(response)
    if response.source_epoch != expected_source_epoch:
        raise ProtocolError("response source_epoch does not match the expected epoch")
    if type(expected_key_id) is not str or response.key_id != expected_key_id:
        raise ProtocolError("response key_id does not match the manifest")
    request_protocol.request_signing_bytes(request)
    if (response.device_id, response.authority_id, response.boot_epoch, response.request_id,
        response.purpose, response.nonce) != (request.device_id, request.authority_id,
        request.boot_epoch, request.request_id, request.purpose, request.nonce):
        raise ProtocolError("response does not bind the exact request")
    try:
        verify_p256_digest(public_key_der, response.signature, response_signing_digest(response))
    except CryptoError as exc:
        raise ProtocolError(str(exc)) from exc
    return response
