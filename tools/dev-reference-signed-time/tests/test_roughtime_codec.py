import hashlib
import struct
import unittest

import path_bootstrap
from roughtime_codec import (
    PACKET_MAGIC, RoughtimeCodecError, decode_message, decode_packet,
    encode_message, encode_packet,
)
from roughtime_config import PROVIDER_PUBLIC_KEY
from roughtime_verify import build_request


class RoughtimeCodecTests(unittest.TestCase):
    def assert_rejected(self, operation):
        with self.assertRaises(RoughtimeCodecError) as caught:
            operation()
        self.assertEqual(str(caught.exception), "invalid roughtime encoding")
        self.assertIsNone(caught.exception.__cause__)
        self.assertIsNone(caught.exception.__context__)

    def test_request_is_exact_pinned_1024_byte_draft11_packet(self):
        nonce = bytes(range(32))
        packet = build_request(nonce)
        self.assertEqual(len(packet), 1024)
        self.assertEqual(packet[:8], PACKET_MAGIC)
        self.assertEqual(packet[8:12], struct.pack("<I", 1012))
        message = decode_message(
            decode_packet(packet, max_packet_bytes=1024),
            max_pairs=4,
            max_bytes=1012,
        )
        self.assertEqual(message.names(), ("VER", "SRV", "NONC", "ZZZZ"))
        self.assertEqual(message.value("VER"), struct.pack("<I", 0x8000000B))
        self.assertEqual(message.value("NONC"), nonce)
        expected_srv = hashlib.sha512(b"\xff" + PROVIDER_PUBLIC_KEY).digest()[:32]
        self.assertEqual(message.value("SRV"), expected_srv)
        self.assertEqual(message.value("ZZZZ"), b"\0" * 912)

    def test_encoder_sorts_tags_and_preserves_zero_length_values(self):
        encoded = encode_message(
            (("ZZZZ", b"z" * 4), ("PATH", b""), ("VER", b"v" * 4)),
            max_pairs=3,
            max_bytes=64,
        )
        decoded = decode_message(encoded, max_pairs=3, max_bytes=64)
        self.assertEqual(frozenset(decoded.entries), {
            ("ZZZZ", b"z" * 4), ("PATH", b""), ("VER", b"v" * 4),
        })

    def test_encoder_rejects_duplicate_invalid_or_unaligned_pairs(self):
        cases = (
            (("VER", b"a" * 4), ("VER", b"b" * 4)),
            (("bad", b"a" * 4),),
            (("ABCDE", b"a" * 4),),
            (("A", b"x"), ("B", b"y")),
        )
        for entries in cases:
            with self.subTest(entries=entries):
                self.assert_rejected(lambda e=entries: encode_message(
                    e, max_pairs=4, max_bytes=64,
                ))

    def test_decoder_rejects_unsorted_duplicate_and_invalid_tags(self):
        encoded = bytearray(encode_message(
            (("A", b"a" * 4), ("B", b"b" * 4), ("C", b"c" * 4)),
            max_pairs=3,
            max_bytes=64,
        ))
        variants = []
        unsorted = bytearray(encoded)
        unsorted[12:16], unsorted[16:20] = encoded[16:20], encoded[12:16]
        variants.append(unsorted)
        duplicate = bytearray(encoded)
        duplicate[16:20] = duplicate[12:16]
        variants.append(duplicate)
        invalid = bytearray(encoded)
        invalid[12:16] = b"a\0\0\0"
        variants.append(invalid)
        for candidate in variants:
            self.assert_rejected(lambda c=bytes(candidate): decode_message(
                c, max_pairs=3, max_bytes=64,
            ))

    def test_decoder_rejects_bad_counts_offsets_headers_and_bounds(self):
        encoded = bytearray(encode_message(
            (("A", b"a" * 4), ("B", b"b" * 4), ("C", b"c" * 4)),
            max_pairs=3,
            max_bytes=64,
        ))
        variants = (
            b"\0\0\0\0",
            struct.pack("<I", 4) + bytes(encoded[4:]),
            bytes(encoded[:4] + struct.pack("<I", 3) + encoded[8:]),
            bytes(encoded[:4] + struct.pack("<I", 12) + struct.pack("<I", 4) + encoded[12:]),
            bytes(encoded[:4] + struct.pack("<I", 40) + encoded[8:]),
        )
        for candidate in variants:
            self.assert_rejected(lambda c=candidate: decode_message(
                c, max_pairs=3, max_bytes=64,
            ))
        self.assert_rejected(lambda: decode_message(bytes(encoded), max_pairs=2, max_bytes=64))
        self.assert_rejected(lambda: decode_message(bytes(encoded), max_pairs=3, max_bytes=20))

    def test_packet_requires_exact_magic_length_consumption_and_bound(self):
        message = encode_message((("A", b"x"),), max_pairs=1, max_bytes=16)
        packet = encode_packet(message, max_packet_bytes=64)
        variants = (
            b"BADMAGIC" + packet[8:],
            packet[:8] + struct.pack("<I", len(message) + 1) + message,
            packet + b"x",
            packet[:-1],
        )
        for candidate in variants:
            self.assert_rejected(lambda c=candidate: decode_packet(
                c, max_packet_bytes=64,
            ))
        self.assert_rejected(lambda: decode_packet(packet, max_packet_bytes=len(packet) - 1))


if __name__ == "__main__":
    unittest.main()
