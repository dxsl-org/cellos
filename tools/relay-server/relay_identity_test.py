from __future__ import annotations

import asyncio
import json
import tempfile
import unittest
from pathlib import Path

from relay import ERR_DESTINATION_UNAVAILABLE, FT_ERROR, FT_PING, FT_PONG, FT_SEND_PACKET
from relay_identity import Denylist, load_denylist
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


class RelayIdentityWireTests(unittest.IsolatedAsyncioTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls._temp.name)
        cls.certificates: CertificateSet = make_certificates(cls.root)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._temp.cleanup()

    async def asyncSetUp(self) -> None:
        self.servers: list[asyncio.AbstractServer] = []
        self.writers: list[asyncio.StreamWriter] = []

    async def asyncTearDown(self) -> None:
        for writer in reversed(self.writers):
            await close_writer(writer)
        for server in self.servers:
            server.close()
            await server.wait_closed()
        await asyncio.sleep(0)

    async def start(self, denylist: Denylist) -> int:
        server, _, port = await start_relay(self.certificates, denylist)
        self.servers.append(server)
        return port

    async def open(self, credential, port: int):
        reader, writer = await connect(self.certificates, credential, port)
        self.writers.append(writer)
        return reader, writer

    async def assert_application_rejection(self, credential, denylist: Denylist) -> None:
        port = await self.start(denylist)
        reader, _ = await self.open(credential, port)
        self.assertEqual(await asyncio.wait_for(reader.read(1), 2), b"")

    async def test_missing_node_id_extension_is_rejected_after_tls_authentication(self) -> None:
        await self.assert_application_rejection(
            self.certificates.missing_binding, empty_denylist()
        )

    async def test_wrong_node_id_extension_is_rejected_after_tls_authentication(self) -> None:
        await self.assert_application_rejection(
            self.certificates.wrong_binding, empty_denylist()
        )

    async def test_revoked_node_identity_is_never_registered(self) -> None:
        denylist_path = self.root / "deny-node.json"
        denylist_path.write_text(
            json.dumps(
                {
                    "revoked_node_ids": [self.certificates.client_a.node_id.hex()],
                    "revoked_serials": [],
                }
            ),
            encoding="utf-8",
        )
        port = await self.start(load_denylist(denylist_path))
        revoked_reader, _ = await self.open(self.certificates.client_a, port)
        self.assertEqual(await asyncio.wait_for(revoked_reader.read(1), 2), b"")

        sender_reader, sender_writer = await self.open(self.certificates.client_b, port)
        await send_frame(sender_writer, bytes([FT_PING]) + b"sender--")
        self.assertEqual(await read_frame(sender_reader), bytes([FT_PONG]) + b"sender--")
        await send_frame(
            sender_writer,
            bytes([FT_SEND_PACKET]) + self.certificates.client_a.node_id + b"data",
        )
        self.assertEqual(
            await read_frame(sender_reader),
            bytes([FT_ERROR, ERR_DESTINATION_UNAVAILABLE]),
        )

    async def test_revoked_certificate_serial_is_rejected(self) -> None:
        denylist_path = self.root / "deny-serial.json"
        denylist_path.write_text(
            json.dumps(
                {
                    "revoked_node_ids": [],
                    "revoked_serials": [hex(self.certificates.client_b.serial)],
                }
            ),
            encoding="utf-8",
        )
        await self.assert_application_rejection(
            self.certificates.client_b, load_denylist(denylist_path)
        )


if __name__ == "__main__":
    unittest.main()
