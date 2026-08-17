#!/usr/bin/env python3
"""Protocol and image tests that do not require privileged UDP ports."""

import importlib.util
import tempfile
import unittest
from pathlib import Path

from protocols import is_allowed_client, parse_rrq, resolve_tftp_file

MODULE_PATH = Path(__file__).with_name("rpi3-uimage.py")
SPEC = importlib.util.spec_from_file_location("rpi3_uimage", MODULE_PATH)
assert SPEC and SPEC.loader
UIMAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UIMAGE)


class TftpTests(unittest.TestCase):
    def test_only_static_pi_address_is_allowed(self) -> None:
        self.assertTrue(is_allowed_client(("192.168.42.2", 49152), "192.168.42.2"))
        self.assertFalse(is_allowed_client(("192.168.42.3", 49152), "192.168.42.2"))

    def test_rrq_options(self) -> None:
        packet = b"\x00\x01cellos.uimg\x00octet\x00blksize\x001024\x00tsize\x000\x00"
        name, options = parse_rrq(packet)
        self.assertEqual(name, "cellos.uimg")
        self.assertEqual(options, {"blksize": "1024", "tsize": "0"})

    def test_path_traversal_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                resolve_tftp_file(Path(temporary), "../secret")


class UimageTests(unittest.TestCase):
    def test_round_trip_preserves_raw_kernel(self) -> None:
        payload = b"Cellos raw kernel" * 257
        image = UIMAGE.create_uimage(payload)
        self.assertEqual(UIMAGE.verify_uimage(image), payload)

    def test_payload_corruption_is_rejected(self) -> None:
        image = bytearray(UIMAGE.create_uimage(b"kernel"))
        image[-1] ^= 1
        with self.assertRaisesRegex(ValueError, "payload CRC"):
            UIMAGE.verify_uimage(bytes(image))


if __name__ == "__main__":
    unittest.main()
