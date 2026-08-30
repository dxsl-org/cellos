#!/usr/bin/env python3
"""TLS 1.3 mutual-authentication relay for opaque Noise packets."""

from __future__ import annotations

import asyncio
import logging
import ssl
from dataclasses import dataclass

from relay_admission import (
    AdmissionError,
    AdmissionLease,
    AdmissionTable,
    AuthenticatedSessionIdentity,
)
from relay_identity import Denylist, PeerCertificateError, peer_identity
from relay_io import (
    ERR_MALFORMED_FRAME,
    IO_TIMEOUT_SECONDS,
    MAX_FRAME_SIZE,
    ProtocolError,
    read_frame as _read_frame,
    send_frame as _send_frame,
)

MAX_AUTHENTICATED_SESSIONS = 128
FT_SEND_PACKET = 0x08
FT_RECV_PACKET = 0x09
FT_PING = 0x0B
FT_PONG = 0x0C
FT_ERROR = 0x7F
ERR_DESTINATION_UNAVAILABLE = 0x01
ERR_UNKNOWN_FRAME = 0x03
ERR_DELIVERY_UNCERTAIN = 0x04

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("relay")

@dataclass(frozen=True)
class ClientEntry:
    writer: asyncio.StreamWriter
    handler: asyncio.Task[None]
    send_lock: asyncio.Lock


class RelayServer:
    def __init__(
        self,
        denylist: Denylist,
        *,
        max_sessions: int = MAX_AUTHENTICATED_SESSIONS,
        io_timeout: float = IO_TIMEOUT_SECONDS,
    ) -> None:
        self.denylist = denylist
        self.io_timeout = io_timeout
        self.admission: AdmissionTable[ClientEntry] = AdmissionTable(max_sessions)

    async def handle(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        node_id: bytes | None = None
        generation: int | None = None
        addr = writer.get_extra_info("peername")
        try:
            ssl_object = writer.get_extra_info("ssl_object")
            if ssl_object is None:
                raise PeerCertificateError("TLS is required")
            node_id = peer_identity(ssl_object.getpeercert(binary_form=True), self.denylist)
            handler = asyncio.current_task()
            assert handler is not None
            admitted = self._register(
                AuthenticatedSessionIdentity(node_id), node_id, writer, handler
            )
            if isinstance(admitted, AdmissionError):
                log.warning("relay admission rejected for %s: %s", addr, admitted.name)
                return
            generation = admitted.generation
            log.info("authenticated node %s from %s", node_id.hex()[:8], addr)
            await self._dispatch(reader, writer, node_id, generation)
        except PeerCertificateError as exc:
            log.warning("rejected peer %s: %s", addr, exc)
        except ProtocolError as exc:
            if node_id is not None and generation is not None:
                await self._send_error(node_id, generation, exc.code)
        except (asyncio.IncompleteReadError, ConnectionError, TimeoutError):
            pass
        finally:
            if node_id is not None and generation is not None:
                self._unregister(node_id, generation)
            writer.close()
            try:
                await writer.wait_closed()
            except (ConnectionError, ssl.SSLError):
                pass

    def _register(
        self,
        authenticated: AuthenticatedSessionIdentity | None,
        claimed_node_id: bytes,
        writer: asyncio.StreamWriter,
        handler: asyncio.Task[None],
    ) -> AdmissionLease[ClientEntry] | AdmissionError:
        entry = ClientEntry(writer, handler, asyncio.Lock())
        return self.admission.admit(authenticated, claimed_node_id, entry)

    def _unregister(self, node_id: bytes, generation: int) -> None:
        if self.admission.release(node_id, generation) is None:
            log.info("node %s disconnected", node_id.hex()[:8])

    def _current(
        self, node_id: bytes, generation: int, writer: asyncio.StreamWriter | None = None
    ) -> ClientEntry | None:
        lease = self.admission.current(node_id, generation)
        if lease is None:
            return None
        entry = lease.session
        if writer is not None and entry.writer is not writer:
            return None
        return entry

    async def _dispatch(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        src_id: bytes,
        generation: int,
    ) -> None:
        while self._current(src_id, generation, writer) is not None:
            frame = await _read_frame(reader, self.io_timeout)
            if frame is None or self._current(src_id, generation, writer) is None:
                return
            frame_type = frame[0]
            if frame_type == FT_SEND_PACKET:
                if len(frame) < 33:
                    raise ProtocolError(ERR_MALFORMED_FRAME)
                await self._forward(src_id, generation, frame[1:33], frame[33:])
            elif frame_type == FT_PING:
                if len(frame) != 9:
                    raise ProtocolError(ERR_MALFORMED_FRAME)
                await self._send_current(src_id, generation, bytes([FT_PONG]) + frame[1:])
            else:
                raise ProtocolError(ERR_UNKNOWN_FRAME)

    async def _send_current(
        self,
        node_id: bytes,
        generation: int,
        data: bytes,
        source: tuple[bytes, int] | None = None,
    ) -> bool:
        entry = self._current(node_id, generation)
        if entry is None:
            return False
        async with entry.send_lock:
            if self._current(node_id, generation) is not entry or entry.writer.is_closing():
                return False
            if source is not None and self._current(*source) is None:
                return False
            await _send_frame(entry.writer, data, self.io_timeout)
            return True

    async def _forward(
        self, src_id: bytes, generation: int, dest_id: bytes, payload: bytes
    ) -> None:
        if self._current(src_id, generation) is None:
            return
        destination = self.admission.lookup(dest_id)
        if destination is None:
            await self._send_error(src_id, generation, ERR_DESTINATION_UNAVAILABLE)
            return
        destination_entry = destination.session
        try:
            sent = await self._send_current(
                dest_id,
                destination.generation,
                bytes([FT_RECV_PACKET]) + src_id + payload,
                (src_id, generation),
            )
        except (ConnectionError, ssl.SSLError, TimeoutError):
            destination_entry.writer.close()
            if self._current(src_id, generation) is not None:
                await self._send_error(src_id, generation, ERR_DELIVERY_UNCERTAIN)
            return
        if not sent and self._current(src_id, generation) is not None:
            await self._send_error(src_id, generation, ERR_DESTINATION_UNAVAILABLE)

    async def _send_error(self, node_id: bytes, generation: int, code: int) -> None:
        try:
            await self._send_current(node_id, generation, bytes([FT_ERROR, code]))
        except (ConnectionError, ssl.SSLError, TimeoutError):
            entry = self._current(node_id, generation)
            if entry is not None:
                entry.writer.close()

if __name__ == "__main__":
    from relay_bootstrap import run

    run()
