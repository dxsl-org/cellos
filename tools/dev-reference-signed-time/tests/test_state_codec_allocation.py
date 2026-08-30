import copy
import unittest
from dataclasses import FrozenInstanceError
import path_bootstrap

from allocation import AllocationState
from protocol_models import MAX_UINT64
from receipt import SOURCE_STATE_KEY
from state_codec import (
    StateCodecError, decode_allocation_state, encode_allocation_state,
)
from state_codec_support import (
    DictChild, IntChild, StateChild, StrChild, malformed_avs, replace_av, state,
)


class AllocationStateCodecTests(unittest.TestCase):
    def setUp(self):
        self.value = state()
        self.item = encode_allocation_state(self.value)

    def assert_invalid(self, operation, value):
        with self.assertRaises(StateCodecError) as raised:
            operation(value)
        self.assertEqual(raised.exception.code, "invalid-allocation-state")
        self.assertEqual(str(raised.exception), "invalid-allocation-state")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_golden_schema_and_roundtrip_are_exact(self):
        expected = {
            "pk": {"S": SOURCE_STATE_KEY}, "schema_version": {"N": "1"},
            "record_type": {"S": "allocation_state"},
            "source_epoch": {"N": "7"}, "source_sequence": {"N": "42"},
            "last_unix_seconds": {"N": "1700000000"},
        }
        self.assertEqual(self.item, expected)
        self.assertEqual(decode_allocation_state(expected), self.value)

    def test_all_missing_and_extra_attributes_are_rejected(self):
        for field in self.item:
            with self.subTest(missing=field):
                changed = copy.deepcopy(self.item)
                del changed[field]
                self.assert_invalid(decode_allocation_state, changed)
        changed = copy.deepcopy(self.item)
        changed["extra"] = {"N": "0"}
        self.assert_invalid(decode_allocation_state, changed)

    def test_every_attribute_rejects_empty_multiple_and_wrong_av_types(self):
        for field, av in self.item.items():
            for malformed in malformed_avs(av):
                with self.subTest(field=field, malformed=malformed):
                    self.assert_invalid(
                        decode_allocation_state,
                        replace_av(self.item, field, malformed),
                    )

    def test_all_numeric_fields_reject_noncanonical_range_and_non_string_values(self):
        bad = (
            "", "00", "01", "-1", "+1", "1.0", " 1", "1 ", "١",
            str(MAX_UINT64 + 1), "9" * 5000, 1, True, StrChild("1"),
        )
        for field in ("source_epoch", "source_sequence", "last_unix_seconds"):
            for value in bad:
                with self.subTest(field=field, value=repr(value)):
                    self.assert_invalid(
                        decode_allocation_state,
                        replace_av(self.item, field, {"N": value}),
                    )

    def test_uint64_boundaries_roundtrip(self):
        for value in (0, MAX_UINT64):
            state_value = AllocationState(value, value, value)
            with self.subTest(value=value):
                self.assertEqual(
                    decode_allocation_state(encode_allocation_state(state_value)),
                    state_value,
                )

    def test_encode_rejects_numeric_range_bool_and_subclasses(self):
        for value in (-1, MAX_UINT64 + 1, True, IntChild(1)):
            for index in range(3):
                fields = [7, 42, 1_700_000_000]
                fields[index] = value
                with self.subTest(value=repr(value), index=index):
                    self.assert_invalid(
                        encode_allocation_state, AllocationState(*fields),
                    )
        self.assert_invalid(encode_allocation_state, StateChild(7, 42, 10))

    def test_key_schema_record_and_key_container_types_are_frozen(self):
        changes = {
            "pk": {"S": SOURCE_STATE_KEY + "x"}, "schema_version": {"N": "2"},
            "record_type": {"S": "request_receipt"},
        }
        for field, av in changes.items():
            self.assert_invalid(
                decode_allocation_state, replace_av(self.item, field, av),
            )
        self.assert_invalid(decode_allocation_state, DictChild(self.item))
        changed = copy.deepcopy(self.item)
        value = changed.pop("source_epoch")
        changed[StrChild("source_epoch")] = value
        self.assert_invalid(decode_allocation_state, changed)
        changed = replace_av(self.item, "source_epoch", {StrChild("N"): "7"})
        self.assert_invalid(decode_allocation_state, changed)

    def test_state_value_is_frozen(self):
        with self.assertRaises(FrozenInstanceError):
            self.value.source_epoch = 8


if __name__ == "__main__":
    unittest.main()
