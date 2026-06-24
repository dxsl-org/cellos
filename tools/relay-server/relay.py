#!/usr/bin/env python3
"""
Cellos relay server — minimal raw TCP relay for G1 internet testing.

Forwards packets between registered Cellos net-broker nodes.
The relay sees only NodeIds and byte counts; payload is Noise-encrypted end-to-end.

Usage:
    python3 relay.py [--host 0.0.0.0] [--port 8765]

Frame format (all integers big-endian):
    [4B length (u32)][1B frame_type][data]

Frame types:
    0x01 CLIENT_REGISTER  data = node_id(32)        → register caller's NodeId
    0x02 SERVER_ACK       data = status(1)           ← 0x00 = ok, 0x01 = err
    0x08 SEND_PACKET      data = dest_node_id(32) + payload(N)
    0x09 RECV_PACKET      data = src_node_id(32)  + payload(N)  ← forwarded to dest
    0x0b PING             data = timestamp(8)
    0x0c PONG             data = timestamp(8)       ← echoed back
"""

import asyncio
import struct
import argparse
import logging
from typing import Dict, Optional

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("relay")

FT_CLIENT_REGISTER = 0x01
FT_SERVER_ACK      = 0x02
FT_SEND_PACKET     = 0x08
FT_RECV_PACKET     = 0x09
FT_PING            = 0x0b
FT_PONG            = 0x0c


class RelayServer:
    def __init__(self) -> None:
        # node_id bytes (32B) → asyncio writer
        self.clients: Dict[bytes, asyncio.StreamWriter] = {}

    async def handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        addr = writer.get_extra_info("peername")
        log.info("new connection from %s", addr)
        node_id: Optional[bytes] = None
        try:
            node_id = await self._register(reader, writer)
            if node_id is None:
                return
            log.info("registered node %s from %s", node_id.hex()[:8], addr)
            await self._dispatch(reader, writer, node_id)
        except (asyncio.IncompleteReadError, ConnectionResetError, BrokenPipeError):
            pass
        finally:
            if node_id and self.clients.get(node_id) is writer:
                del self.clients[node_id]
                log.info("node %s disconnected", node_id.hex()[:8] if node_id else "?")
            writer.close()

    async def _register(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> Optional[bytes]:
        frame = await _read_frame(reader)
        if not frame or frame[0] != FT_CLIENT_REGISTER or len(frame) < 33:
            _send_ack(writer, ok=False)
            return None
        node_id = frame[1:33]
        self.clients[node_id] = writer
        _send_ack(writer, ok=True)
        await writer.drain()
        return node_id

    async def _dispatch(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        src_id: bytes,
    ) -> None:
        while True:
            frame = await _read_frame(reader)
            if frame is None:
                break
            ft = frame[0]
            if ft == FT_SEND_PACKET:
                if len(frame) < 33:
                    continue
                dest_id = frame[1:33]
                payload = frame[33:]
                dest_writer = self.clients.get(dest_id)
                if dest_writer is None:
                    log.debug("drop: dest %s not connected", dest_id.hex()[:8])
                    continue
                # Forward as RECV_PACKET with src_id prepended.
                out_data = bytes([FT_RECV_PACKET]) + src_id + payload
                out_frame = struct.pack(">I", len(out_data)) + out_data
                try:
                    dest_writer.write(out_frame)
                    await dest_writer.drain()
                except Exception:
                    pass
            elif ft == FT_PING:
                pong = bytes([FT_PONG]) + frame[1:9]
                writer.write(struct.pack(">I", len(pong)) + pong)
                await writer.drain()


async def _read_frame(reader: asyncio.StreamReader) -> Optional[bytes]:
    """Read one length-prefixed frame. Returns None on EOF."""
    hdr = await reader.readexactly(4)
    length = struct.unpack(">I", hdr)[0]
    if length == 0 or length > 8192:
        return None
    return await reader.readexactly(length)


def _send_ack(writer: asyncio.StreamWriter, ok: bool) -> None:
    status = 0x00 if ok else 0x01
    data = bytes([FT_SERVER_ACK, status])
    writer.write(struct.pack(">I", len(data)) + data)


async def main(host: str, port: int) -> None:
    server = RelayServer()
    srv = await asyncio.start_server(server.handle, host, port)
    addrs = ", ".join(str(s.getsockname()) for s in srv.sockets)
    log.info("Cellos relay listening on %s", addrs)
    async with srv:
        await srv.serve_forever()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Cellos relay server")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    asyncio.run(main(args.host, args.port))
