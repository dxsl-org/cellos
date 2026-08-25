from __future__ import annotations

import asyncio
import unittest

from relay import RelayServer
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
    async def test_source_replacement_closes_blocked_destination_buffer(self) -> None:
        relay = RelayServer(Denylist(frozenset(), frozenset()), io_timeout=60)
        source_id = b"s" * 32
        destination_id = b"d" * 32
        destination = FakeWriter(block_drain=True)
        destination_handler = asyncio.create_task(asyncio.sleep(60))
        relay._register(destination_id, destination, destination_handler)

        async def forward(writer: FakeWriter) -> None:
            handler = asyncio.current_task()
            assert handler is not None
            entry = relay._register(source_id, writer, handler)
            assert entry is not None
            try:
                await relay._forward(source_id, entry.generation, destination_id, b"noise")
            finally:
                relay._unregister(source_id, entry.generation)

        first = asyncio.create_task(forward(FakeWriter()))
        await asyncio.wait_for(destination.drain_started.wait(), 1)

        replacements = []
        for _ in range(4):
            replacement = asyncio.create_task(forward(FakeWriter()))
            replacements.append(replacement)
            await asyncio.sleep(0)

        await asyncio.gather(first, *replacements, return_exceptions=True)
        self.assertTrue(destination.closed)
        self.assertEqual(len(destination.frames), 1)

        destination_handler.cancel()
        await asyncio.gather(destination_handler, return_exceptions=True)


if __name__ == "__main__":
    unittest.main()
