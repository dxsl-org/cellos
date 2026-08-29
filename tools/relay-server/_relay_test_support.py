from __future__ import annotations

import asyncio
import ssl
import struct

from _relay_certificate_support import CertificateSet, Credential, make_certificates
from relay import RelayServer
from relay_bootstrap import build_ssl_context, start_relay_server
from relay_identity import Denylist


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
    server, _ = await start_relay_server(relay, context, "127.0.0.1", 0)
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
