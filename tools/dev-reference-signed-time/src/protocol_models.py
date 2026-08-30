"""Immutable typed values for the signed-time version 1 wire protocol."""

from dataclasses import dataclass

PROTOCOL_VERSION = 1
SOURCE_ID = "cellos-dev-time-v1"
SIGNING_ALGORITHM = "ECDSA_SHA_256"
MAX_REQUEST_BYTES = 1024
MAX_RESPONSE_BYTES = 1024
MAX_UINT64 = (1 << 64) - 1
VALID_PURPOSES = frozenset((1, 2, 3))


@dataclass(frozen=True, slots=True)
class RegisteredAuthority:
    """The exact device/authority/key tuple selected by the caller's registry."""

    device_id: bytes
    authority_id: bytes
    public_key_der: bytes


@dataclass(frozen=True, slots=True)
class UnsignedRequest:
    """Request labels 1 through 8; these exact claims are Ed25519-signed."""

    device_id: bytes
    authority_id: bytes
    boot_epoch: int
    request_id: bytes
    purpose: int
    nonce: bytes
    authority_pubkey: bytes


@dataclass(frozen=True, slots=True)
class SignedRequest(UnsignedRequest):
    """Complete request including the 64-byte Ed25519 signature at label 9."""

    signature: bytes


@dataclass(frozen=True, slots=True)
class UnsignedResponse:
    """Response labels 1 through 14; their SHA-256 digest is sent to KMS."""

    source_epoch: int
    source_sequence: int
    unix_seconds: int
    expires_at: int
    device_id: bytes
    authority_id: bytes
    boot_epoch: int
    request_id: bytes
    purpose: int
    nonce: bytes
    key_id: str


@dataclass(frozen=True, slots=True)
class SignedResponse(UnsignedResponse):
    """Complete response including strict DER P-256 ECDSA signature label 15."""

    signature: bytes
