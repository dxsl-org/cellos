from __future__ import annotations

import asyncio
import struct
import unittest

from relay_admission import (
    AdmissionError,
    AdmissionLease,
    AuthenticatedSessionIdentity,
)
from relay import ERR_DELIVERY_UNCERTAIN, FT_PACKET_ERROR, RelayServer
from relay_identity import Denylist


class FakeWriter:
    def __init__(self, block_drain: bool = False) -> None:
        self.block_drain = block_drain
        self.drain_started = asyncio.Event()
        self.release_drain = asyncio.Event()
        self.frames: list[bytes] = []
        self.closed = False

    def write(self, data: bytes) -> None:
        self.frames.append(data)

    async def drain(self) -> None:
        self.drain_started.set()
        if self.block_drain:
            await self.release_drain.wait()

    def close(self) -> None:
        self.closed = True
        self.release_drain.set()

    def is_closing(self) -> bool:
        return self.closed


class RelayCancellationTests(unittest.IsolatedAsyncioTestCase):
    async def test_duplicate_identity_does_not_cancel_or_replace_live_source(self) -> None:
        relay = RelayServer(Denylist(frozenset(), frozenset()), io_timeout=60)
        source_id = b"s" * 32
        destination_id = b"d" * 32
        destination = FakeWriter(block_drain=True)
        destination_handler = asyncio.create_task(asyncio.sleep(60))
        destination_lease = relay._register(
            AuthenticatedSessionIdentity(destination_id),
            destination_id,
            destination,
            destination_handler,
        )
        self.assertIsInstance(destination_lease, AdmissionLease)

        source = FakeWriter()

        async def forward() -> None:
            handler = asyncio.current_task()
            assert handler is not None
            admitted = relay._register(
                AuthenticatedSessionIdentity(source_id), source_id, source, handler
            )
            assert isinstance(admitted, AdmissionLease)
            try:
                await relay._forward(
                    source_id, admitted.generation, 11, destination_id, b"noise"
                )
            finally:
                relay._unregister(source_id, admitted.generation)

        first = asyncio.create_task(forward())
        await asyncio.wait_for(destination.drain_started.wait(), 1)
        handler = asyncio.current_task()
        assert handler is not None
        for _ in range(4):
            rejected = relay._register(
                AuthenticatedSessionIdentity(source_id),
                source_id,
                FakeWriter(),
                handler,
            )
            self.assertEqual(rejected, AdmissionError.DUPLICATE_LIVE)
        self.assertFalse(source.closed)
        self.assertFalse(destination.closed)

        destination.release_drain.set()
        await first
        self.assertEqual(len(destination.frames), 1)

        destination_handler.cancel()
        await asyncio.gather(destination_handler, return_exceptions=True)

    async def test_destination_drain_timeout_is_sender_visible_uncertain(self) -> None:
        relay = RelayServer(Denylist(frozenset(), frozenset()), io_timeout=0.01)
        source_id = b"s" * 32
        destination_id = b"d" * 32
        source = FakeWriter()
        destination = FakeWriter(block_drain=True)
        handler = asyncio.current_task()
        assert handler is not None
        source_lease = relay._register(
            AuthenticatedSessionIdentity(source_id), source_id, source, handler
        )
        destination_lease = relay._register(
            AuthenticatedSessionIdentity(destination_id),
            destination_id,
            destination,
            handler,
        )
        assert isinstance(source_lease, AdmissionLease)
        assert isinstance(destination_lease, AdmissionLease)

        await relay._forward(
            source_id, source_lease.generation, 13, destination_id, b"opaque"
        )

        self.assertTrue(destination.closed)
        self.assertEqual(len(destination.frames), 1)
        error = (
            bytes([FT_PACKET_ERROR])
            + (13).to_bytes(8, "big")
            + bytes([ERR_DELIVERY_UNCERTAIN])
        )
        self.assertEqual(source.frames, [struct.pack(">I", len(error)) + error])
        self.assertFalse(source.closed)


if __name__ == "__main__":
    unittest.main()
