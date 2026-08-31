from dataclasses import dataclass, replace
import hashlib
import json
from pathlib import Path
import struct
from unittest.mock import patch

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

import path_bootstrap
from roughtime_codec import encode_message, encode_packet
from roughtime_config import provider_config
from roughtime_verify import build_request

RESPONSE_CONTEXT = b"RoughTime v1 response signature\0"
DELEGATION_CONTEXT = b"RoughTime v1 delegation signature--\0"
NONCE = bytes(range(32))
LONG_TERM_PRIVATE = Ed25519PrivateKey.from_private_bytes(b"L" * 32)
DELEGATED_PRIVATE = Ed25519PrivateKey.from_private_bytes(b"D" * 32)


def public_bytes(private):
    return private.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw,
    )


CONFIG = replace(provider_config(), public_key=public_bytes(LONG_TERM_PRIVATE))


def digest(value):
    return hashlib.sha512(value).digest()[:32]


def exact_request(nonce=NONCE, config=CONFIG):
    with patch("roughtime_verify.validate_provider_config"):
        return build_request(nonce, config)


def merkle_root(nonce, path, index):
    current = digest(b"\0" + nonce)
    for offset in range(0, len(path), 32):
        node = path[offset:offset + 32]
        if index & 1:
            current = digest(b"\1" + node + current)
        else:
            current = digest(b"\1" + current + node)
        index >>= 1
    return current


@dataclass(frozen=True, slots=True)
class ResponseOptions:
    nonce: bytes = NONCE
    root_version: int = 0x8000000B
    midpoint: int = 1_700_000_000
    radius: int = 3
    minimum: int = 1_699_999_000
    maximum: int = 1_700_001_000
    path: bytes = b""
    index: int = 0
    root: bytes | None = None
    omit_root: str = ""
    omit_cert: str = ""
    omit_dele: str = ""
    omit_srep: str = ""
    dele_extra: tuple[tuple[str, bytes], ...] = ()
    cert_extra: tuple[tuple[str, bytes], ...] = ()
    srep_extra: tuple[tuple[str, bytes], ...] = ()
    root_extra: tuple[tuple[str, bytes], ...] = ()
    bad_delegation_signature: bool = False
    bad_response_signature: bool = False


def _without(entries, omitted):
    return tuple(entry for entry in entries if entry[0] != omitted)


def response_packet(options=ResponseOptions(), request=None):
    del request
    delegated_public = public_bytes(DELEGATED_PRIVATE)
    dele_entries = (
        ("MINT", struct.pack("<Q", options.minimum)),
        ("MAXT", struct.pack("<Q", options.maximum)),
        ("PUBK", delegated_public),
    ) + options.dele_extra
    dele = encode_message(
        _without(dele_entries, options.omit_dele), max_pairs=4, max_bytes=1012,
    )
    delegation_signature = LONG_TERM_PRIVATE.sign(DELEGATION_CONTEXT + dele)
    if options.bad_delegation_signature:
        delegation_signature = bytes((delegation_signature[0] ^ 1,)) + delegation_signature[1:]
    cert_entries = (("DELE", dele), ("SIG", delegation_signature)) + options.cert_extra
    cert = encode_message(
        _without(cert_entries, options.omit_cert), max_pairs=3, max_bytes=1012,
    )
    root_hash = options.root
    if root_hash is None and len(options.path) % 32 == 0:
        root_hash = merkle_root(options.nonce, options.path, options.index)
    if root_hash is None:
        root_hash = b"R" * 32
    srep_entries = (
        ("RADI", struct.pack("<I", options.radius)),
        ("MIDP", struct.pack("<Q", options.midpoint)),
        ("ROOT", root_hash),
    ) + options.srep_extra
    srep = encode_message(
        _without(srep_entries, options.omit_srep), max_pairs=4, max_bytes=1012,
    )
    response_signature = DELEGATED_PRIVATE.sign(RESPONSE_CONTEXT + srep)
    if options.bad_response_signature:
        response_signature = bytes((response_signature[0] ^ 1,)) + response_signature[1:]
    root_entries = (
        ("SIG", response_signature),
        ("VER", struct.pack("<I", options.root_version)),
        ("NONC", options.nonce),
        ("PATH", options.path),
        ("SREP", srep),
        ("CERT", cert),
        ("INDX", struct.pack("<I", options.index)),
    ) + options.root_extra
    message = encode_message(
        _without(root_entries, options.omit_root), max_pairs=8, max_bytes=1012,
    )
    return encode_packet(message, max_packet_bytes=1024)


def vector(options=ResponseOptions()):
    request = exact_request()
    return CONFIG, request, response_packet(options)


_fixture = Path(__file__).parents[1] / "vectors" / "cloudflare-roughtime-draft11-001.json"
OFFICIAL_VECTOR = json.loads(_fixture.read_text(encoding="ascii"))
OFFICIAL_REQUEST = (
    bytes.fromhex(OFFICIAL_VECTOR["request_prefix"])
    + b"\0" * OFFICIAL_VECTOR["request_zero_padding_bytes"]
)
OFFICIAL_REPLY = bytes.fromhex(OFFICIAL_VECTOR["reply"])
OFFICIAL_NONCE = bytes.fromhex(OFFICIAL_VECTOR["nonce"])
OFFICIAL_CONFIG = replace(
    provider_config(),
    public_key=public_bytes(Ed25519PrivateKey.from_private_bytes(
        bytes.fromhex(OFFICIAL_VECTOR["root_key"]),
    )),
)

_batched_fixture = (
    Path(__file__).parents[1] / "vectors" / "cloudflare-roughtime-draft11-010.json"
)
BATCHED_VECTOR = json.loads(_batched_fixture.read_text(encoding="ascii"))
BATCHED_REQUEST = (
    bytes.fromhex(BATCHED_VECTOR["request_prefix"])
    + b"\0" * BATCHED_VECTOR["request_zero_padding_bytes"]
)
BATCHED_REPLY = bytes.fromhex(BATCHED_VECTOR["reply"])
BATCHED_NONCE = bytes.fromhex(BATCHED_VECTOR["nonce"])
