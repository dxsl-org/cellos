"""Strict public-key and signature operations for signed-time protocol v1."""

from cryptography.exceptions import InvalidSignature, UnsupportedAlgorithm
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519, utils

ED25519_SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")
P256_ORDER = int("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551", 16)


class CryptoError(ValueError):
    """Raised for malformed keys/signatures or failed verification."""


def load_ed25519_spki(der: bytes) -> ed25519.Ed25519PublicKey:
    """Load only the canonical 44-byte RFC 8410 Ed25519 DER-SPKI form."""
    if type(der) is not bytes or len(der) != 44 or not der.startswith(ED25519_SPKI_PREFIX):
        raise CryptoError("Ed25519 key is not canonical DER-SPKI")
    return ed25519.Ed25519PublicKey.from_public_bytes(der[len(ED25519_SPKI_PREFIX):])


def verify_ed25519(der: bytes, signature: bytes, message: bytes) -> None:
    """Verify an Ed25519 signature or raise ``CryptoError``."""
    if type(signature) is not bytes or len(signature) != 64:
        raise CryptoError("Ed25519 signature must be 64 bytes")
    if type(message) is not bytes:
        raise CryptoError("Ed25519 message must be bytes")
    try:
        load_ed25519_spki(der).verify(signature, message)
    except InvalidSignature as exc:
        raise CryptoError("Ed25519 signature verification failed") from exc


def _parse_p256_der_signature(signature: bytes) -> tuple[int, int]:
    if type(signature) is not bytes or not 8 <= len(signature) <= 72:
        raise CryptoError("P-256 signature has invalid length")
    if signature[0] != 0x30 or signature[1] != len(signature) - 2:
        raise CryptoError("P-256 signature is not a canonical DER sequence")
    offset = 2
    values: list[int] = []
    for _ in range(2):
        if offset + 2 > len(signature) or signature[offset] != 0x02:
            raise CryptoError("P-256 signature is missing a DER integer")
        length = signature[offset + 1]
        offset += 2
        if not 1 <= length <= 33 or offset + length > len(signature):
            raise CryptoError("P-256 DER integer has invalid length")
        encoded = signature[offset:offset + length]
        offset += length
        if encoded[0] & 0x80:
            raise CryptoError("P-256 DER integer is negative")
        if len(encoded) > 1 and encoded[0] == 0 and not encoded[1] & 0x80:
            raise CryptoError("P-256 DER integer has redundant padding")
        value = int.from_bytes(encoded, "big")
        if not 1 <= value < P256_ORDER:
            raise CryptoError("P-256 signature scalar is out of range")
        values.append(value)
    if offset != len(signature):
        raise CryptoError("trailing bytes in P-256 DER signature")
    canonical = utils.encode_dss_signature(values[0], values[1])
    if canonical != signature:
        raise CryptoError("P-256 signature is not canonical DER")
    return values[0], values[1]


def parse_p256_der_signature(signature: bytes) -> tuple[int, int]:
    """Parse canonical low-S DER ECDSA-P256 signature and enforce scalar ranges."""
    r, s = _parse_p256_der_signature(signature)
    if s > P256_ORDER // 2:
        raise CryptoError("P-256 signature is not low-S")
    return r, s


def canonicalize_p256_der_signature(signature: bytes) -> bytes:
    """Normalize a strict DER KMS P-256 signature to the protocol's low-S form."""
    r, s = _parse_p256_der_signature(signature)
    return utils.encode_dss_signature(r, min(s, P256_ORDER - s))


def load_p256_spki(der: bytes) -> ec.EllipticCurvePublicKey:
    """Load a canonical DER-SPKI public key on exactly NIST P-256."""
    if type(der) is not bytes:
        raise CryptoError("P-256 public key must be DER bytes")
    try:
        key = serialization.load_der_public_key(der)
    except (TypeError, ValueError, UnsupportedAlgorithm) as exc:
        raise CryptoError("invalid P-256 DER-SPKI") from exc
    if not isinstance(key, ec.EllipticCurvePublicKey) or not isinstance(key.curve, ec.SECP256R1):
        raise CryptoError("response key is not P-256")
    canonical = key.public_bytes(serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)
    if canonical != der:
        raise CryptoError("P-256 key is not canonical DER-SPKI")
    return key


def verify_p256_digest(der: bytes, signature: bytes, digest: bytes) -> None:
    """Verify strict DER ECDSA over one 32-byte SHA-256 digest."""
    parse_p256_der_signature(signature)
    if type(digest) is not bytes or len(digest) != 32:
        raise CryptoError("SHA-256 digest must be 32 bytes")
    try:
        load_p256_spki(der).verify(
            signature, digest, ec.ECDSA(utils.Prehashed(hashes.SHA256()))
        )
    except InvalidSignature as exc:
        raise CryptoError("P-256 signature verification failed") from exc
