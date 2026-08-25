from __future__ import annotations

import asyncio
import datetime as dt
import hashlib
import ipaddress
import ssl
import struct
from dataclasses import dataclass
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

from relay import MAX_FRAME_SIZE, RelayServer
from relay_bootstrap import build_ssl_context
from relay_identity import Denylist, NODE_ID_OID

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


def _make_ca(root: Path, name: str, serial: int) -> tuple[Path, ec.EllipticCurvePrivateKey, x509.Certificate]:
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
) -> Credential:
    key = ec.generate_private_key(ec.SECP256R1())
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
        .add_extension(
            x509.ExtendedKeyUsage(
                [ExtendedKeyUsageOID.SERVER_AUTH if server else ExtendedKeyUsageOID.CLIENT_AUTH]
            ),
            critical=False,
        )
    )
    if server:
        builder = builder.add_extension(
            x509.SubjectAlternativeName(
                [x509.DNSName("localhost"), x509.IPAddress(ipaddress.ip_address("127.0.0.1"))]
            ),
            critical=False,
        )
    elif binding != "missing":
        value = node_id if binding == "correct" else bytes([node_id[0] ^ 1]) + node_id[1:]
        builder = builder.add_extension(x509.UnrecognizedExtension(NODE_ID_OID, value), critical=False)
    certificate = builder.sign(ca_key, hashes.SHA256())
    return _write_credential(root, name, key, certificate)


def make_certificates(root: Path) -> CertificateSet:
    ca_path, ca_key, ca = _make_ca(root, "trusted-ca", 1)
    server = _make_leaf(root, "server", 10, ca_key, ca, server=True)
    client_a = _make_leaf(root, "client-a", 20, ca_key, ca)
    client_b = _make_leaf(root, "client-b", 21, ca_key, ca)
    missing = _make_leaf(root, "client-missing", 22, ca_key, ca, binding="missing")
    wrong = _make_leaf(root, "client-wrong", 23, ca_key, ca, binding="wrong")
    _, other_key, other_ca = _make_ca(root, "untrusted-ca", 2)
    untrusted = _make_leaf(root, "client-untrusted", 24, other_key, other_ca)
    return CertificateSet(ca_path, server, client_a, client_b, missing, wrong, untrusted)


def empty_denylist() -> Denylist:
    return Denylist(frozenset(), frozenset())


def client_context(certificates: CertificateSet, credential: Credential | None) -> ssl.SSLContext:
    context = ssl.create_default_context(ssl.Purpose.SERVER_AUTH, cafile=str(certificates.ca_cert))
    context.minimum_version = ssl.TLSVersion.TLSv1_3
    context.maximum_version = ssl.TLSVersion.TLSv1_3
    if credential is not None:
        context.load_cert_chain(str(credential.cert), str(credential.key))
    return context


async def start_relay(
    certificates: CertificateSet, denylist: Denylist
) -> tuple[asyncio.AbstractServer, RelayServer, int]:
    relay = RelayServer(denylist)
    context = build_ssl_context(
        certificates.server.cert, certificates.server.key, certificates.ca_cert
    )
    server = await asyncio.start_server(
        relay.handle,
        "127.0.0.1",
        0,
        ssl=context,
        limit=MAX_FRAME_SIZE + 4,
    )
    port = server.sockets[0].getsockname()[1]
    return server, relay, port


async def connect(
    certificates: CertificateSet,
    credential: Credential | None,
    port: int,
) -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
    return await asyncio.open_connection(
        "127.0.0.1",
        port,
        ssl=client_context(certificates, credential),
        server_hostname="localhost",
    )


def encode_frame(data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + data


async def send_frame(writer: asyncio.StreamWriter, data: bytes) -> None:
    writer.write(encode_frame(data))
    await writer.drain()


async def read_frame(reader: asyncio.StreamReader) -> bytes:
    header = await asyncio.wait_for(reader.readexactly(4), 2)
    length = struct.unpack(">I", header)[0]
    return await asyncio.wait_for(reader.readexactly(length), 2)


async def close_writer(writer: asyncio.StreamWriter) -> None:
    writer.close()
    try:
        await writer.wait_closed()
    except (ConnectionError, OSError, ssl.SSLError):
        pass
