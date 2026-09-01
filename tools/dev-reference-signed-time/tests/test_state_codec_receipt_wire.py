import unittest
import path_bootstrap

import cbor_codec
from state_codec import StateCodecError, decode_receipt
from state_codec_support import receipt, replace_av, response_wire
from state_codec import encode_receipt


class ReceiptWireCodecTests(unittest.TestCase):
    def setUp(self):
        self.item = encode_receipt(receipt())
        self.wire = response_wire()
        self.response_map = cbor_codec.loads(self.wire)

    def assert_invalid_wire(self, wire):
        changed = replace_av(self.item, "response_signing_bytes", {"B": wire})
        with self.assertRaises(StateCodecError) as raised:
            decode_receipt(changed)
        self.assertEqual(raised.exception.code, "invalid-request-receipt")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def changed_wire(self, label, value):
        changed = dict(self.response_map)
        changed[label] = value
        return cbor_codec.dumps(changed)

    def test_every_truncated_prefix_is_rejected(self):
        for length in range(len(self.wire)):
            with self.subTest(length=length):
                self.assert_invalid_wire(self.wire[:length])

    def test_noncanonical_trailing_duplicate_and_out_of_order_cbor_are_rejected(self):
        pair1 = cbor_codec.dumps({1: self.response_map[1]})[1:]
        pair2 = cbor_codec.dumps({2: self.response_map[2]})[1:]
        rest = 1 + len(pair1) + len(pair2)
        malformed = (
            self.wire + b"\x00",
            self.wire[:1] + b"\x18\x01" + self.wire[2:],
            b"\xaf" + self.wire[1:] + b"\x01\x01",
            self.wire[:1] + pair2 + pair1 + self.wire[rest:],
        )
        for wire in malformed:
            with self.subTest(wire=wire[:8].hex()):
                self.assert_invalid_wire(wire)

    def test_exact_labels_one_through_fourteen_are_required(self):
        for label in range(1, 15):
            changed = dict(self.response_map)
            del changed[label]
            with self.subTest(missing=label):
                self.assert_invalid_wire(cbor_codec.dumps(changed))
        for label in (0, 15, "1", b"1"):
            changed = dict(self.response_map)
            changed[label] = 1
            with self.subTest(extra=repr(label)):
                self.assert_invalid_wire(cbor_codec.dumps(changed))

    def test_schema_source_and_algorithm_constants_require_exact_types_and_values(self):
        cases = {
            1: ("1", 2),
            2: (b"cellos-dev-time-v1", "other-source"),
            14: (b"ECDSA_SHA_256", "other-algorithm"),
        }
        for label, values in cases.items():
            for value in values:
                with self.subTest(label=label, value=repr(value)):
                    self.assert_invalid_wire(self.changed_wire(label, value))

    def test_every_response_claim_requires_its_exact_wire_type(self):
        wrong_types = {
            3: "7", 4: "42", 5: "1700000000", 6: "1700000060",
            7: "d" * 32, 8: "a" * 32, 9: "9", 10: "r" * 16,
            11: "2", 12: "n" * 32, 13: b"manifest-key",
        }
        for label, value in wrong_types.items():
            with self.subTest(label=label):
                self.assert_invalid_wire(self.changed_wire(label, value))

    def test_response_claim_lengths_ranges_and_relations_are_validated(self):
        cases = {
            3: -1, 4: -1, 5: -1, 6: 1_700_000_000,
            7: b"d" * 31, 8: b"a" * 31, 9: -1, 10: b"r" * 15,
            11: 4, 12: b"n" * 31, 13: "",
        }
        for label, value in cases.items():
            if value == -1:
                wire = self.changed_wire(label, "-1")
            else:
                wire = self.changed_wire(label, value)
            with self.subTest(label=label):
                self.assert_invalid_wire(wire)
        self.assert_invalid_wire(self.changed_wire(6, 1_700_000_061))

    def test_partition_key_binds_authority_and_request_labels(self):
        self.assert_invalid_wire(self.changed_wire(8, b"b" * 32))
        self.assert_invalid_wire(self.changed_wire(10, b"q" * 16))


if __name__ == "__main__":
    unittest.main()
