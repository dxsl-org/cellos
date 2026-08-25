"""TLS context construction and manifest-driven bootstrap for the mTLS relay."""

from __future__ import annotations

import argparse
import asyncio
import logging
import ssl
from pathlib import Path

from relay import MAX_FRAME_SIZE, RelayServer
from relay_identity import load_denylist
from relay_manifest import RelayServerConfig, load_server_manifest

log = logging.getLogger("relay")
TLS_HANDSHAKE_TIMEOUT_SECONDS = 10.0
TLS_SHUTDOWN_TIMEOUT_SECONDS = 5.0
SERVER_BACKLOG = 128


def build_ssl_context(
    server_cert: str | Path, server_key: str | Path, client_ca: str | Path
) -> ssl.SSLContext:
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_3
    context.maximum_version = ssl.TLSVersion.TLSv1_3
    context.verify_mode = ssl.CERT_REQUIRED
    context.load_cert_chain(str(server_cert), str(server_key))
    context.load_verify_locations(cafile=str(client_ca))
    return context


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Cellos mutual-TLS relay server")
    parser.add_argument("--manifest", required=True, type=Path)
    return parser.parse_args()


async def serve(config: RelayServerConfig) -> None:
    relay = RelayServer(load_denylist(config.relay_denylist))
    context = build_ssl_context(
        config.certificate_pem,
        config.private_key_pem,
        config.client_issuing_ca_pem,
    )
    server = await asyncio.start_server(
        relay.handle,
        config.bind_host,
        config.port,
        ssl=context,
        limit=MAX_FRAME_SIZE + 4,
        ssl_handshake_timeout=TLS_HANDSHAKE_TIMEOUT_SECONDS,
        ssl_shutdown_timeout=TLS_SHUTDOWN_TIMEOUT_SECONDS,
        backlog=SERVER_BACKLOG,
    )
    addrs = ", ".join(str(sock.getsockname()) for sock in server.sockets or ())
    log.info("Cellos mTLS relay listening on %s", addrs)
    async with server:
        await server.serve_forever()


def run() -> None:
    args = parse_args()
    asyncio.run(serve(load_server_manifest(args.manifest)))
