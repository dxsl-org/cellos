"""Draft-11 request construction and strict Cloudflare response verification."""

from dataclasses import dataclass
import hashlib
import hmac
import struct
from typing import NoReturn

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from roughtime_codec import (
    RoughtimeCodecError, RoughtimeMessage, decode_message, decode_packet,
    encode_message, encode_packet,
)
from roughtime_config import (
    MAX_MERKLE_PATH_NODES, MAX_MESSAGE_PAIRS, RoughtimeProviderConfig,
    provider_config, validate_provider_config,
)

_RESPONSE_CONTEXT = b"RoughTime v1 response signature\0"
_DELEGATION_CONTEXT = b"RoughTime v1 delegation signature--\0"
_ERROR = "invalid roughtime response"


class RoughtimeVerificationError(ValueError):
    __slots__ = ()


def _fail() -> NoReturn:
    raise RoughtimeVerificationError(_ERROR) from None


def _hash(value: bytes) -> bytes:
    return hashlib.sha512(value).digest()[:32]


@dataclass(frozen=True, slots=True)
class VerifiedRoughtime:
    midpoint: int
    radius: int


def _require(message: RoughtimeMessage, mandatory: frozenset[str]) -> None:
    if not mandatory.issubset(message.names()):
        _fail()


def _uint(value: bytes, size: int) -> int:
    if len(value) != size:
        _fail()
    return int.from_bytes(value, "little")


def _verify_signature(key: bytes, signature: bytes, signed: bytes) -> None:
    if len(key) != 32 or len(signature) != 64:
        _fail()
    failed = False
    try:
        Ed25519PublicKey.from_public_bytes(key).verify(signature, signed)
    except (InvalidSignature, TypeError, ValueError):
        failed = True
    if failed:
        _fail()


def build_request(
    nonce: bytes, config: RoughtimeProviderConfig | None = None,
) -> bytes:
    """Build the sole exact 1,024-byte UDP packet from a fresh 32-byte nonce."""
    selected = provider_config() if config is None else config
    validate_provider_config(selected)
    if type(nonce) is not bytes or len(nonce) != 32:
        _fail()
    padding = selected.request_message_bytes - 32 - 4 - 32 - 32
    if padding < 0 or padding % 4:
        _fail()
    message = encode_message((
        ("VER", struct.pack("<I", selected.version)),
        ("NONC", nonce),
        ("SRV", _hash(b"\xff" + selected.public_key)),
        ("ZZZZ", b"\0" * padding),
    ), max_pairs=4, max_bytes=selected.request_message_bytes)
    if len(message) != selected.request_message_bytes:
        _fail()
    packet = encode_packet(message, max_packet_bytes=selected.max_packet_bytes)
    if len(packet) != selected.max_packet_bytes:
        _fail()
    return packet


def _decode_nested(value: bytes, limit: int) -> RoughtimeMessage:
    return decode_message(value, max_pairs=MAX_MESSAGE_PAIRS, max_bytes=limit)




def _verify_merkle(
    nonce: bytes, path: bytes, index: int, expected_root: bytes,
) -> None:
    if len(expected_root) != 32 or len(path) % 32:
        _fail()
    count = len(path) // 32
    if count > MAX_MERKLE_PATH_NODES or index >> count:
        _fail()
    current = _hash(b"\0" + nonce)
    for offset in range(0, len(path), 32):
        node = path[offset:offset + 32]
        if index & 1:
            current = _hash(b"\1" + node + current)
        else:
            current = _hash(b"\1" + current + node)
        index >>= 1
    if not hmac.compare_digest(current, expected_root):
        _fail()


def verify_response(
    response_packet: bytes,
    request_packet: bytes,
    nonce: bytes,
    config: RoughtimeProviderConfig | None = None,
) -> VerifiedRoughtime:
    """Return midpoint/radius only after complete draft-11 authentication."""
    selected = provider_config() if config is None else config
    failed = False
    result = None
    try:
        validate_provider_config(selected)
        if (
            type(request_packet) is not bytes
            or type(response_packet) is not bytes
            or not hmac.compare_digest(request_packet, build_request(nonce, selected))
            or len(response_packet) > len(request_packet)
        ):
            _fail()
        root = decode_message(
            decode_packet(response_packet, max_packet_bytes=selected.max_packet_bytes),
            max_pairs=MAX_MESSAGE_PAIRS,
            max_bytes=selected.request_message_bytes,
        )
        _require(root, frozenset(("SIG", "VER", "NONC", "PATH", "SREP", "CERT", "INDX")))
        if (
            _uint(root.value("VER"), 4) != selected.version
            or not hmac.compare_digest(root.value("NONC"), nonce)
        ):
            _fail()
        cert_raw = root.value("CERT")
        cert = _decode_nested(cert_raw, selected.request_message_bytes)
        _require(cert, frozenset(("DELE", "SIG")))
        dele_raw = cert.value("DELE")
        dele = _decode_nested(dele_raw, selected.request_message_bytes)
        _require(dele, frozenset(("MINT", "MAXT", "PUBK")))
        _verify_signature(
            selected.public_key, cert.value("SIG"), _DELEGATION_CONTEXT + dele_raw,
        )
        srep_raw = root.value("SREP")
        srep = _decode_nested(srep_raw, selected.request_message_bytes)
        _require(srep, frozenset(("ROOT", "MIDP", "RADI")))
        _verify_signature(
            dele.value("PUBK"), root.value("SIG"), _RESPONSE_CONTEXT + srep_raw,
        )
        midpoint = _uint(srep.value("MIDP"), 8)
        radius = _uint(srep.value("RADI"), 4)
        minimum = _uint(dele.value("MINT"), 8)
        maximum = _uint(dele.value("MAXT"), 8)
        if radius < 3 or not minimum <= midpoint <= maximum:
            _fail()
        index = _uint(root.value("INDX"), 4)
        _verify_merkle(nonce, root.value("PATH"), index, srep.value("ROOT"))
        result = VerifiedRoughtime(midpoint, radius)
    except RoughtimeVerificationError:
        raise
    except (RoughtimeCodecError, OverflowError, struct.error):
        failed = True
    if failed or type(result) is not VerifiedRoughtime:
        _fail()
    return result
