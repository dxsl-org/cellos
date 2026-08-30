import unittest

from vector_support import vector
from cbor_codec import CborError, dumps, loads


class DeterministicCborTests(unittest.TestCase):
    def test_round_trip_and_deterministic_map_order(self):
        value = {24: b"twenty-four", 1: {"z": b"", "aa": 23}, b"k": "text"}
        encoded = dumps(value)
        self.assertTrue(encoded.startswith(bytes.fromhex("a301a2617a")))
        self.assertEqual(loads(encoded), value)
        self.assertEqual(dumps({24: 0, 1: 0}), bytes.fromhex("a20100181800"))
        self.assertEqual(dumps({1000: 0, "": 0}), bytes.fromhex("a21903e8006000"))

    def test_uint_and_length_boundaries_use_shortest_form(self):
        cases = {
            0: "00", 23: "17", 24: "1818", 255: "18ff", 256: "190100",
            65535: "19ffff", 65536: "1a00010000",
            0xFFFFFFFF: "1affffffff", 0x100000000: "1b0000000100000000",
            (1 << 64) - 1: "1bffffffffffffffff",
        }
        for value, expected in cases.items():
            with self.subTest(value=value):
                self.assertEqual(dumps(value).hex(), expected)
                self.assertEqual(loads(bytes.fromhex(expected)), value)

    def test_committed_malformed_vectors_are_rejected(self):
        for case in vector("malformed-v1.json")["cases"]:
            with self.subTest(case=case["name"]):
                with self.assertRaises(CborError):
                    loads(bytes.fromhex(case["cbor_hex"]))

    def test_encoder_rejects_every_unsupported_value(self):
        unsupported = [True, False, None, -1, 1 << 64, 1.5, [], (), bytearray(b"x")]
        for value in unsupported:
            with self.subTest(value=repr(value)):
                with self.assertRaises(CborError):
                    dumps(value)

    def test_encoder_reports_lone_surrogate_as_cbor_error(self):
        with self.assertRaises(CborError):
            dumps("\ud800")

    def test_map_key_restrictions_and_depth_limit(self):
        with self.assertRaises(CborError):
            dumps({(1,): 2})
        nested = 0
        for _ in range(34):
            nested = {0: nested}
        with self.assertRaises(CborError):
            dumps(nested)

    def test_input_type_size_truncation_and_trailing_are_rejected(self):
        with self.assertRaises(CborError):
            loads(bytearray(b"\x00"))
        with self.assertRaises(CborError):
            loads(b"\x41")
        with self.assertRaises(CborError):
            loads(b"\x00", max_size=0)
        with self.assertRaises(CborError):
            loads(b"\x00\x01")


if __name__ == "__main__":
    unittest.main()
