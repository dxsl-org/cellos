"""TLS context construction and manifest-driven bootstrap for the mTLS relay."""

from __future__ import annotations

import argparse
import asyncio
import logging
import ssl
from pathlib import Path

from relay import MAX_AUTHENTICATED_SESSIONS, MAX_FRAME_SIZE, RelayServer
from relay_identity import load_denylist
from relay_manifest import RelayServerConfig, load_server_manifest

log = logging.getLogger("relay")
TLS_HANDSHAKE_TIMEOUT_SECONDS = 10.0
TLS_SHUTDOWN_TIMEOUT_SECONDS = 5.0
SERVER_BACKLOG = 128
MAX_ACTIVE_CONNECTIONS = MAX_AUTHENTICATED_SESSIONS


class ConnectionGate:
    """Synchronous event-loop gate spanning TLS handshake and live session."""

    def __init__(self, limit: int) -> None:
        if limit <= 0:
            raise ValueError("connection limit must be positive")
        self.limit = limit
        self.active = 0

    def try_acquire(self) -> bool:
        if self.active >= self.limit:
            return False
        self.active += 1
        return True

    def release(self) -> None:
        if self.active <= 0:
            raise RuntimeError("connection gate underflow")
        self.active -= 1


async def start_relay_server(
    relay: RelayServer,
    context: ssl.SSLContext,
    host: str,
    port: int,
    *,
    connection_limit: int = MAX_ACTIVE_CONNECTIONS,
) -> tuple[asyncio.AbstractServer, ConnectionGate]:
    """Start a raw acceptor that bounds connections before starting TLS."""
    gate = ConnectionGate(connection_limit)

    async def connected(
        reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        if not gate.try_acquire():
            writer.close()
            await writer.wait_closed()
            return
        try:
            await writer.start_tls(
                context,
                ssl_handshake_timeout=TLS_HANDSHAKE_TIMEOUT_SECONDS,
                ssl_shutdown_timeout=TLS_SHUTDOWN_TIMEOUT_SECONDS,
            )
            await relay.handle(reader, writer)
        except (ConnectionError, ssl.SSLError, TimeoutError):
            writer.close()
            try:
                await writer.wait_closed()
            except (ConnectionError, ssl.SSLError):
                pass
        finally:
            gate.release()

    server = await asyncio.start_server(
        connected,
        host,
        port,
        limit=MAX_FRAME_SIZE + 4,
        backlog=SERVER_BACKLOG,
    )
    return server, gate


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
    server, _ = await start_relay_server(
        relay,
        context,
        config.bind_host,
        config.port,
    )
    addrs = ", ".join(str(sock.getsockname()) for sock in server.sockets or ())
    log.info("Cellos mTLS relay listening on %s", addrs)
    async with server:
        await server.serve_forever()


def run() -> None:
    args = parse_args()
    asyncio.run(serve(load_server_manifest(args.manifest)))
