"""Strict parsing and registered-authority authentication for requests."""

from typing import Any

import cbor_codec
from protocol_errors import ProtocolError
from protocol_crypto import CryptoError, load_ed25519_spki, verify_ed25519
from protocol_models import (
    MAX_REQUEST_BYTES,
    MAX_UINT64,
    PROTOCOL_VERSION,
    VALID_PURPOSES,
    RegisteredAuthority,
    SignedRequest,
    UnsignedRequest,
)


def _bytes(value: Any, length: int, name: str) -> None:
    if type(value) is not bytes or len(value) != length:
        raise ProtocolError(f"{name} must be exactly {length} bytes")


def _uint(value: Any, name: str) -> None:
    if type(value) is not int or not 0 <= value <= MAX_UINT64:
        raise ProtocolError(f"{name} must be a uint64")


def _request_map(request: UnsignedRequest, signature: bytes | None = None) -> dict[int, Any]:
    result = {
        1: PROTOCOL_VERSION,
        2: request.device_id,
        3: request.authority_id,
        4: request.boot_epoch,
        5: request.request_id,
        6: request.purpose,
        7: request.nonce,
        8: request.authority_pubkey,
    }
    if signature is not None:
        result[9] = signature
    return result


def _validate_request(request: UnsignedRequest) -> None:
    if type(request) not in (UnsignedRequest, SignedRequest):
        raise ProtocolError("request must be an UnsignedRequest")
    _bytes(request.device_id, 32, "device_id")
    _bytes(request.authority_id, 32, "authority_id")
    _uint(request.boot_epoch, "boot_epoch")
    _bytes(request.request_id, 16, "request_id")
    if type(request.purpose) is not int or request.purpose not in VALID_PURPOSES:
        raise ProtocolError("purpose must be 1, 2, or 3")
    _bytes(request.nonce, 32, "nonce")
    _bytes(request.authority_pubkey, 44, "authority_pubkey")
    try:
        load_ed25519_spki(request.authority_pubkey)
    except CryptoError as exc:
        raise ProtocolError(str(exc)) from exc


def request_signing_bytes(request: UnsignedRequest) -> bytes:
    """Return canonical labels 1..8, the exact Ed25519 request message."""
    _validate_request(request)
    return cbor_codec.dumps(_request_map(request))


def _verify_signed_request(request: SignedRequest) -> None:
    if type(request) is not SignedRequest:
        raise ProtocolError("request must be a SignedRequest")
    _bytes(request.signature, 64, "request_signature")
    message = request_signing_bytes(request)
    try:
        verify_ed25519(request.authority_pubkey, request.signature, message)
    except CryptoError as exc:
        raise ProtocolError(str(exc)) from exc


def encode_request(request: SignedRequest) -> bytes:
    """Self-verify and encode a complete request as canonical CBOR."""
    _verify_signed_request(request)
    encoded = cbor_codec.dumps(_request_map(request, request.signature))
    if len(encoded) > MAX_REQUEST_BYTES:
        raise ProtocolError("request exceeds 1024 bytes")
    return encoded


def parse_request(data: bytes) -> SignedRequest:
    """Parse canonical request bytes and verify only their embedded-key signature."""
    if type(data) is not bytes:
        raise ProtocolError("CBOR input must be bytes")
    if len(data) > MAX_REQUEST_BYTES:
        raise ProtocolError("CBOR input exceeds size limit")
    try:
        value = cbor_codec.loads(data, max_size=MAX_REQUEST_BYTES)
    except cbor_codec.CborError as exc:
        raise ProtocolError(str(exc)) from exc
    if type(value) is not dict or set(value) != set(range(1, 10)):
        raise ProtocolError("request map must contain exactly labels 1 through 9")
    if type(value[1]) is not int or value[1] != PROTOCOL_VERSION:
        raise ProtocolError("unsupported request schema version")
    request = SignedRequest(
        value[2], value[3], value[4], value[5], value[6], value[7], value[8], value[9]
    )
    _verify_signed_request(request)
    if cbor_codec.dumps(_request_map(request, request.signature)) != data:
        raise ProtocolError("request is not canonical CBOR")
    return request


def _validate_registration(registration: RegisteredAuthority) -> None:
    if type(registration) is not RegisteredAuthority:
        raise ProtocolError("registration must be a RegisteredAuthority")
    _bytes(registration.device_id, 32, "registered device_id")
    _bytes(registration.authority_id, 32, "registered authority_id")
    _bytes(registration.public_key_der, 44, "registered public_key_der")
    try:
        load_ed25519_spki(registration.public_key_der)
    except CryptoError as exc:
        raise ProtocolError(str(exc)) from exc


def authenticate_request(
    request: SignedRequest, registration: RegisteredAuthority
) -> SignedRequest:
    """Bind a self-signed request to one exact registered authority tuple."""
    _validate_registration(registration)
    encode_request(request)
    if (request.device_id, request.authority_id, request.authority_pubkey) != (
        registration.device_id,
        registration.authority_id,
        registration.public_key_der,
    ):
        raise ProtocolError("request does not match the registered authority tuple")
    try:
        verify_ed25519(
            registration.public_key_der, request.signature, request_signing_bytes(request)
        )
    except CryptoError as exc:
        raise ProtocolError(str(exc)) from exc
    return request


def decode_request(data: bytes, registration: RegisteredAuthority) -> SignedRequest:
    """Parse, self-verify, and authenticate request bytes in one composed step."""
    request = parse_request(data)
    return authenticate_request(request, registration)
