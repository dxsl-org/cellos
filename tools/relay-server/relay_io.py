"""Bounded frame I/O shared by the relay protocol."""

from __future__ import annotations

import asyncio
import struct

MAX_FRAME_SIZE = 8192
IO_TIMEOUT_SECONDS = 30.0
ERR_MALFORMED_FRAME = 0x02


class ProtocolError(Exception):
    def __init__(self, code: int) -> None:
        super().__init__(code)
        self.code = code


async def read_frame(reader: asyncio.StreamReader, timeout: float) -> bytes | None:
    try:
        header = await asyncio.wait_for(reader.readexactly(4), timeout)
    except asyncio.IncompleteReadError as exc:
        if not exc.partial:
            return None
        raise ProtocolError(ERR_MALFORMED_FRAME) from exc
    length = struct.unpack(">I", header)[0]
    if length == 0 or length > MAX_FRAME_SIZE:
        raise ProtocolError(ERR_MALFORMED_FRAME)
    try:
        return await asyncio.wait_for(reader.readexactly(length), timeout)
    except asyncio.IncompleteReadError as exc:
        raise ProtocolError(ERR_MALFORMED_FRAME) from exc


async def send_frame(
    writer: asyncio.StreamWriter, data: bytes, timeout: float = IO_TIMEOUT_SECONDS
) -> None:
    if not 0 < len(data) <= MAX_FRAME_SIZE:
        raise ValueError("outbound frame exceeds protocol bounds")
    writer.write(struct.pack(">I", len(data)) + data)
    try:
        await asyncio.wait_for(writer.drain(), timeout)
    except asyncio.CancelledError:
        # Cancellation after `write` has queued bytes has an uncertain delivery
        # outcome. Close the destination so buffered work cannot outlive the
        # forwarding task.
        writer.close()
        raise
