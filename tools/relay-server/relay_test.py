from __future__ import annotations

import asyncio
import ssl
import struct
import tempfile
import unittest
from pathlib import Path

from relay import (
    ERR_DESTINATION_UNAVAILABLE,
    ERR_MALFORMED_FRAME,
    ERR_UNKNOWN_FRAME,
    FT_ERROR,
    FT_PING,
    FT_PONG,
    FT_RECV_PACKET,
    FT_SEND_PACKET,
    MAX_FRAME_SIZE,
)
from _relay_test_support import (
    CertificateSet,
    close_writer,
    connect,
    empty_denylist,
    make_certificates,
    read_frame,
    send_frame,
    start_relay,
)


class RelayWireTests(unittest.IsolatedAsyncioTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._temp = tempfile.TemporaryDirectory()
        cls.certificates: CertificateSet = make_certificates(Path(cls._temp.name))

    @classmethod
    def tearDownClass(cls) -> None:
        cls._temp.cleanup()

    async def asyncSetUp(self) -> None:
        self.server, _, self.port = await start_relay(
            self.certificates, empty_denylist()
        )
        self.writers: list[asyncio.StreamWriter] = []

    async def asyncTearDown(self) -> None:
        for writer in reversed(self.writers):
            await close_writer(writer)
        self.server.close()
        await self.server.wait_closed()
        await asyncio.sleep(0)

    async def open_client(self, credential):
        reader, writer = await connect(self.certificates, credential, self.port)
        self.writers.append(writer)
        return reader, writer

    async def assert_ping(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter, token: bytes
    ) -> None:
        self.assertEqual(len(token), 8)
        await send_frame(writer, bytes([FT_PING]) + token)
        self.assertEqual(await read_frame(reader), bytes([FT_PONG]) + token)

    async def test_valid_mtls_route_is_tls13_and_preserves_opaque_payload(self) -> None:
        reader_a, writer_a = await self.open_client(self.certificates.client_a)
        reader_b, writer_b = await self.open_client(self.certificates.client_b)
        await self.assert_ping(reader_a, writer_a, b"client-a")
        await self.assert_ping(reader_b, writer_b, b"client-b")

        tls_a = writer_a.get_extra_info("ssl_object")
        tls_b = writer_b.get_extra_info("ssl_object")
        self.assertEqual(tls_a.version(), "TLSv1.3")
        self.assertEqual(tls_b.version(), "TLSv1.3")

        opaque = b"\x00\xffnoise\x08\x09\x7fpayload"
        await send_frame(
            writer_a,
            bytes([FT_SEND_PACKET]) + self.certificates.client_b.node_id + opaque,
        )
        self.assertEqual(
            await read_frame(reader_b),
            bytes([FT_RECV_PACKET]) + self.certificates.client_a.node_id + opaque,
        )

    async def test_unavailable_destination_is_bounded_and_connection_survives(self) -> None:
        reader, writer = await self.open_client(self.certificates.client_a)
        await self.assert_ping(reader, writer, b"ready---")

        await send_frame(writer, bytes([FT_SEND_PACKET]) + bytes(32) + b"opaque")
        self.assertEqual(
            await read_frame(reader), bytes([FT_ERROR, ERR_DESTINATION_UNAVAILABLE])
        )
        await self.assert_ping(reader, writer, b"still-up")

    async def test_client_register_frame_is_rejected(self) -> None:
        reader, writer = await self.open_client(self.certificates.client_a)
        await send_frame(writer, b"\x01" + self.certificates.client_a.node_id)
        self.assertEqual(await read_frame(reader), bytes([FT_ERROR, ERR_UNKNOWN_FRAME]))
        self.assertEqual(await asyncio.wait_for(reader.read(1), 2), b"")

    async def test_unknown_frame_is_rejected_with_bounded_error(self) -> None:
        reader, writer = await self.open_client(self.certificates.client_a)
        await send_frame(writer, b"\xfeunrecognized")
        self.assertEqual(await read_frame(reader), bytes([FT_ERROR, ERR_UNKNOWN_FRAME]))
        self.assertEqual(await asyncio.wait_for(reader.read(1), 2), b"")

    async def test_malformed_send_packet_is_rejected(self) -> None:
        reader, writer = await self.open_client(self.certificates.client_a)
        await send_frame(writer, bytes([FT_SEND_PACKET]) + bytes(31))
        self.assertEqual(await read_frame(reader), bytes([FT_ERROR, ERR_MALFORMED_FRAME]))
        self.assertEqual(await asyncio.wait_for(reader.read(1), 2), b"")

    async def test_oversize_frame_header_is_rejected_without_reading_a_body(self) -> None:
        reader, writer = await self.open_client(self.certificates.client_a)
        writer.write(struct.pack(">I", MAX_FRAME_SIZE + 1))
        await writer.drain()
        self.assertEqual(await read_frame(reader), bytes([FT_ERROR, ERR_MALFORMED_FRAME]))
        self.assertEqual(await asyncio.wait_for(reader.read(1), 2), b"")

    async def test_duplicate_identity_stale_cleanup_keeps_replacement_routable(self) -> None:
        old_reader, old_writer = await self.open_client(self.certificates.client_a)
        sender_reader, sender_writer = await self.open_client(self.certificates.client_b)
        await self.assert_ping(old_reader, old_writer, b"old-live")
        await self.assert_ping(sender_reader, sender_writer, b"sender--")

        new_reader, new_writer = await self.open_client(self.certificates.client_a)
        self.assertEqual(await asyncio.wait_for(old_reader.read(1), 2), b"")
        await self.assert_ping(new_reader, new_writer, b"new-live")
        await asyncio.sleep(0)

        opaque = b"replacement-route"
        await send_frame(
            sender_writer,
            bytes([FT_SEND_PACKET]) + self.certificates.client_a.node_id + opaque,
        )
        self.assertEqual(
            await read_frame(new_reader),
            bytes([FT_RECV_PACKET]) + self.certificates.client_b.node_id + opaque,
        )


if __name__ == "__main__":
    unittest.main()
