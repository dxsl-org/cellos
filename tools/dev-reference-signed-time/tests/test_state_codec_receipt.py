import copy
import unittest
from dataclasses import FrozenInstanceError, replace
from unittest.mock import patch
import path_bootstrap

from protocol import response_signing_bytes
from protocol_models import UnsignedResponse
from receipt import Receipt, request_receipt_key
from state_codec import (
    StateCodecError, decode_allocation_state, decode_authority_registration,
    decode_receipt, encode_allocation_state, encode_authority_registration,
    encode_receipt,
)
from state_codec_support import (
    BytesChild, DictChild, ReceiptChild, StrChild, malformed_avs, receipt,
    registration, replace_av, response, state,
)


class ResponseChild(UnsignedResponse):
    pass


class ReceiptCodecTests(unittest.TestCase):
    def setUp(self):
        self.value = receipt()
        self.item = encode_receipt(self.value)

    def assert_invalid(self, operation, value):
        with self.assertRaises(StateCodecError) as raised:
            operation(value)
        self.assertEqual(raised.exception.code, "invalid-request-receipt")
        self.assertEqual(str(raised.exception), "invalid-request-receipt")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_golden_schema_and_roundtrip_are_exact(self):
        expected = {
            "pk": {"S": request_receipt_key(b"a" * 32, b"r" * 16)},
            "schema_version": {"N": "1"},
            "record_type": {"S": "request_receipt"},
            "request_digest": {"B": b"h" * 32},
            "response_signing_bytes": {"B": response_signing_bytes(response())},
        }
        self.assertEqual(self.item, expected)
        self.assertEqual(decode_receipt(expected), self.value)
        self.assertIsNot(decode_receipt(expected), self.value)

    def test_all_missing_and_extra_attributes_are_rejected(self):
        for field in self.item:
            with self.subTest(missing=field):
                changed = copy.deepcopy(self.item)
                del changed[field]
                self.assert_invalid(decode_receipt, changed)
        changed = copy.deepcopy(self.item)
        changed["extra"] = {"B": b"x"}
        self.assert_invalid(decode_receipt, changed)

    def test_every_attribute_rejects_empty_multiple_and_wrong_av_types(self):
        for field, av in self.item.items():
            for malformed in malformed_avs(av):
                with self.subTest(field=field, malformed=malformed):
                    self.assert_invalid(
                        decode_receipt, replace_av(self.item, field, malformed),
                    )

    def test_digest_length_and_exact_bytes_type_are_strict(self):
        for digest in (b"h" * 31, b"h" * 33, bytearray(b"h" * 32), BytesChild(b"h" * 32)):
            with self.subTest(direction="encode", digest=type(digest).__name__):
                self.assert_invalid(encode_receipt, Receipt(digest, response()))
            with self.subTest(direction="decode", digest=type(digest).__name__):
                self.assert_invalid(
                    decode_receipt,
                    replace_av(self.item, "request_digest", {"B": digest}),
                )

    def test_partition_schema_record_and_container_types_are_frozen(self):
        changes = {
            "pk": {"S": request_receipt_key(b"b" * 32, b"r" * 16)},
            "schema_version": {"N": "2"},
            "record_type": {"S": "allocation_state"},
        }
        for field, av in changes.items():
            with self.subTest(field=field):
                self.assert_invalid(decode_receipt, replace_av(self.item, field, av))
        self.assert_invalid(decode_receipt, DictChild(self.item))
        changed = copy.deepcopy(self.item)
        value = changed.pop("request_digest")
        changed[StrChild("request_digest")] = value
        self.assert_invalid(decode_receipt, changed)
        changed = replace_av(self.item, "request_digest", {StrChild("B"): b"h" * 32})
        self.assert_invalid(decode_receipt, changed)

    def test_receipt_and_response_subclasses_are_rejected(self):
        self.assert_invalid(
            encode_receipt, ReceiptChild(self.value.request_digest, self.value.response),
        )
        source = self.value.response
        child = ResponseChild(
            source.source_epoch, source.source_sequence, source.unix_seconds,
            source.expires_at, source.device_id, source.authority_id,
            source.boot_epoch, source.request_id, source.purpose, source.nonce,
            source.key_id,
        )
        self.assert_invalid(encode_receipt, Receipt(self.value.request_digest, child))

    def test_values_are_immutable(self):
        decoded = decode_receipt(self.item)
        with self.assertRaises(FrozenInstanceError):
            decoded.request_digest = b"x" * 32
        with self.assertRaises(FrozenInstanceError):
            decoded.response.source_epoch = 8

    def test_codec_performs_no_file_or_network_io(self):
        with patch("builtins.open", side_effect=AssertionError("file I/O")), patch(
            "socket.socket", side_effect=AssertionError("network I/O")
        ):
            self.assertEqual(decode_receipt(encode_receipt(self.value)), self.value)
            registration_value = registration()
            self.assertEqual(
                decode_authority_registration(
                    encode_authority_registration(registration_value)
                ),
                registration_value,
            )
            state_value = state()
            self.assertEqual(
                decode_allocation_state(encode_allocation_state(state_value)),
                state_value,
            )

    def test_response_wire_size_is_bounded_on_both_sides(self):
        large = Receipt(b"h" * 32, replace(response(), key_id="k" * 1024))
        self.assert_invalid(encode_receipt, large)
        self.assert_invalid(
            decode_receipt,
            replace_av(self.item, "response_signing_bytes", {"B": b"x" * 1025}),
        )


if __name__ == "__main__":
    unittest.main()
