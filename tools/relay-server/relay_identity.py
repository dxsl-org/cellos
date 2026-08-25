"""Certificate identity and denylist validation for the relay server."""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from cryptography import x509
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, ObjectIdentifier

NODE_ID_OID = ObjectIdentifier("1.3.6.1.4.1.55555.1.1")
MAX_DENYLIST_ENTRIES = 4096
_MAX_SERIAL = (1 << 159) - 1


class PeerCertificateError(ValueError):
    """The authenticated certificate cannot be used as a relay identity."""


@dataclass(frozen=True)
class Denylist:
    revoked_node_ids: frozenset[bytes]
    revoked_serials: frozenset[int]

    def check(self, node_id: bytes, serial: int) -> None:
        if node_id in self.revoked_node_ids or serial in self.revoked_serials:
            raise PeerCertificateError("peer certificate is revoked")


def load_denylist(path: str | Path) -> Denylist:
    try:
        document: Any = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot load denylist: {exc}") from exc
    if not isinstance(document, dict) or set(document) != {
        "revoked_node_ids",
        "revoked_serials",
    }:
        raise ValueError("denylist must contain only the two required arrays")
    node_values = _bounded_array(document["revoked_node_ids"], "revoked_node_ids")
    serial_values = _bounded_array(document["revoked_serials"], "revoked_serials")
    return Denylist(
        frozenset(_parse_node_id(value) for value in node_values),
        frozenset(_parse_serial(value) for value in serial_values),
    )


def peer_identity(peer_der: bytes, denylist: Denylist) -> bytes:
    if not peer_der:
        raise PeerCertificateError("verified peer certificate is required")
    try:
        certificate = x509.load_der_x509_certificate(peer_der)
    except ValueError as exc:
        raise PeerCertificateError("invalid peer certificate") from exc
    public_key = certificate.public_key()
    if not isinstance(public_key, ec.EllipticCurvePublicKey) or not isinstance(
        public_key.curve, ec.SECP256R1
    ):
        raise PeerCertificateError("peer key must be P-256")
    spki = public_key.public_bytes(
        serialization.Encoding.DER,
        serialization.PublicFormat.SubjectPublicKeyInfo,
    )
    node_id = hashlib.sha256(spki).digest()
    try:
        binding = certificate.extensions.get_extension_for_oid(NODE_ID_OID).value
        eku = certificate.extensions.get_extension_for_class(x509.ExtendedKeyUsage).value
    except x509.ExtensionNotFound as exc:
        raise PeerCertificateError("required certificate extension is missing") from exc
    if not isinstance(binding, x509.UnrecognizedExtension) or binding.value != node_id:
        raise PeerCertificateError("certificate NodeId binding does not match its key")
    if ExtendedKeyUsageOID.CLIENT_AUTH not in eku:
        raise PeerCertificateError("clientAuth EKU is required")
    denylist.check(node_id, certificate.serial_number)
    return node_id


def _bounded_array(value: Any, name: str) -> list[Any]:
    if not isinstance(value, list) or len(value) > MAX_DENYLIST_ENTRIES:
        raise ValueError(f"{name} must be an array of at most {MAX_DENYLIST_ENTRIES} items")
    return value


def _parse_node_id(value: Any) -> bytes:
    if not isinstance(value, str) or len(value) != 64:
        raise ValueError("revoked_node_ids entries must be 64 hexadecimal characters")
    try:
        return bytes.fromhex(value)
    except ValueError as exc:
        raise ValueError("revoked_node_ids contains invalid hexadecimal") from exc


def _parse_serial(value: Any) -> int:
    if type(value) is int:
        serial = value
    elif isinstance(value, str):
        try:
            serial = int(value, 0)
        except ValueError as exc:
            raise ValueError("revoked_serials contains an invalid integer") from exc
    else:
        raise ValueError("revoked_serials entries must be integers or integer strings")
    if not 0 < serial <= _MAX_SERIAL:
        raise ValueError("revoked_serials entries must be positive 159-bit integers")
    return serial
