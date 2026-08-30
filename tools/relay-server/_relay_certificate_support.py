from __future__ import annotations

import datetime as dt
import hashlib
import ipaddress
from dataclasses import dataclass
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

from relay_identity import NODE_ID_OID

_NOT_BEFORE = dt.datetime(2020, 1, 1, tzinfo=dt.timezone.utc)
_NOT_AFTER = dt.datetime(2040, 1, 1, tzinfo=dt.timezone.utc)


@dataclass(frozen=True)
class Credential:
    cert: Path
    key: Path
    der: bytes
    node_id: bytes
    serial: int


@dataclass(frozen=True)
class CertificateSet:
    ca_cert: Path
    server: Credential
    client_a: Credential
    client_b: Credential
    missing_binding: Credential
    wrong_binding: Credential
    untrusted_client: Credential
    wrong_eku: Credential
    missing_eku: Credential
    non_p256: Credential

def _node_id(key: ec.EllipticCurvePrivateKey) -> bytes:
    spki = key.public_key().public_bytes(
        serialization.Encoding.DER,
        serialization.PublicFormat.SubjectPublicKeyInfo,
    )
    return hashlib.sha256(spki).digest()


def _write_credential(
    root: Path,
    name: str,
    key: ec.EllipticCurvePrivateKey,
    certificate: x509.Certificate,
) -> Credential:
    cert_path = root / f"{name}.pem"
    key_path = root / f"{name}-key.pem"
    cert_path.write_bytes(certificate.public_bytes(serialization.Encoding.PEM))
    key_path.write_bytes(
        key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
    )
    return Credential(
        cert_path,
        key_path,
        certificate.public_bytes(serialization.Encoding.DER),
        _node_id(key),
        certificate.serial_number,
    )


def _make_ca(
    root: Path, name: str, serial: int
) -> tuple[Path, ec.EllipticCurvePrivateKey, x509.Certificate]:
    key = ec.generate_private_key(ec.SECP256R1())
    subject = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, name)])
    certificate = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(subject)
        .public_key(key.public_key())
        .serial_number(serial)
        .not_valid_before(_NOT_BEFORE)
        .not_valid_after(_NOT_AFTER)
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .add_extension(
            x509.KeyUsage(
                digital_signature=False,
                content_commitment=False,
                key_encipherment=False,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=True,
                crl_sign=True,
                encipher_only=False,
                decipher_only=False,
            ),
            critical=True,
        )
        .sign(key, hashes.SHA256())
    )
    path = root / f"{name}.pem"
    path.write_bytes(certificate.public_bytes(serialization.Encoding.PEM))
    return path, key, certificate


def _make_leaf(
    root: Path,
    name: str,
    serial: int,
    ca_key: ec.EllipticCurvePrivateKey,
    ca_certificate: x509.Certificate,
    *,
    server: bool = False,
    binding: str = "correct",
    client_auth: bool = True,
    include_eku: bool = True,
    curve: ec.EllipticCurve | None = None,
) -> Credential:
    key = ec.generate_private_key(curve or ec.SECP256R1())
    node_id = _node_id(key)
    builder = (
        x509.CertificateBuilder()
        .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, name)]))
        .issuer_name(ca_certificate.subject)
        .public_key(key.public_key())
        .serial_number(serial)
        .not_valid_before(_NOT_BEFORE)
        .not_valid_after(_NOT_AFTER)
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .add_extension(
            x509.KeyUsage(
                digital_signature=True,
                content_commitment=False,
                key_encipherment=False,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=False,
                crl_sign=False,
                encipher_only=False,
                decipher_only=False,
            ),
            critical=True,
        )
    )
    if include_eku:
        eku = (
            ExtendedKeyUsageOID.SERVER_AUTH
            if server or not client_auth
            else ExtendedKeyUsageOID.CLIENT_AUTH
        )
        builder = builder.add_extension(
            x509.ExtendedKeyUsage([eku]), critical=False
        )
    if server:
        builder = builder.add_extension(
            x509.SubjectAlternativeName(
                [
                    x509.DNSName("localhost"),
                    x509.IPAddress(ipaddress.ip_address("127.0.0.1")),
                ]
            ),
            critical=False,
        )
    elif binding != "missing":
        value = node_id if binding == "correct" else bytes([node_id[0] ^ 1]) + node_id[1:]
        builder = builder.add_extension(
            x509.UnrecognizedExtension(NODE_ID_OID, value), critical=False
        )
    certificate = builder.sign(ca_key, hashes.SHA256())
    return _write_credential(root, name, key, certificate)


def make_certificates(root: Path) -> CertificateSet:
    ca_path, ca_key, ca = _make_ca(root, "trusted-ca", 1)
    server = _make_leaf(root, "server", 10, ca_key, ca, server=True)
    client_a = _make_leaf(root, "client-a", 20, ca_key, ca)
    client_b = _make_leaf(root, "client-b", 21, ca_key, ca)
    missing = _make_leaf(root, "client-missing", 22, ca_key, ca, binding="missing")
    wrong = _make_leaf(root, "client-wrong", 23, ca_key, ca, binding="wrong")
    wrong_eku = _make_leaf(root, "client-wrong-eku", 24, ca_key, ca, client_auth=False)
    missing_eku = _make_leaf(root, "client-missing-eku", 25, ca_key, ca, include_eku=False)
    non_p256 = _make_leaf(
        root, "client-non-p256", 26, ca_key, ca, curve=ec.SECP384R1()
    )
    _, other_key, other_ca = _make_ca(root, "untrusted-ca", 2)
    untrusted = _make_leaf(root, "client-untrusted", 27, other_key, other_ca)
    return CertificateSet(
        ca_path, server, client_a, client_b, missing, wrong, untrusted,
        wrong_eku, missing_eku, non_p256,
    )
