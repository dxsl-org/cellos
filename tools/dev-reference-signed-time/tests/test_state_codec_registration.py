import copy
import unittest
from dataclasses import FrozenInstanceError, replace
import path_bootstrap

from receipt import authority_registration_key
from state_codec import (
    AuthorityRegistration, StateCodecError, decode_authority_registration,
    encode_authority_registration,
)
from state_codec_support import (
    BytesChild, DictChild, RegistrationChild, StrChild, malformed_avs,
    registration, replace_av,
)


class RegistrationCodecTests(unittest.TestCase):
    def setUp(self):
        self.value = registration()
        self.item = encode_authority_registration(self.value)

    def assert_invalid(self, operation, value):
        with self.assertRaises(StateCodecError) as raised:
            operation(value)
        self.assertEqual(raised.exception.code, "invalid-authority-registration")
        self.assertEqual(str(raised.exception), "invalid-authority-registration")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_golden_schema_and_roundtrip_are_exact(self):
        expected = {
            "pk": {"S": authority_registration_key(b"a" * 32)},
            "schema_version": {"N": "1"},
            "record_type": {"S": "authority_registration"},
            "device_id": {"B": b"d" * 32}, "authority_id": {"B": b"a" * 32},
            "public_key_der": {"B": self.value.public_key_der},
            "revoked": {"BOOL": False},
        }
        self.assertEqual(self.item, expected)
        self.assertEqual(decode_authority_registration(expected), self.value)
        self.assertIsNot(decode_authority_registration(expected), self.value)

    def test_all_missing_and_extra_attributes_are_rejected(self):
        for field in self.item:
            with self.subTest(missing=field):
                changed = copy.deepcopy(self.item)
                del changed[field]
                self.assert_invalid(decode_authority_registration, changed)
        changed = copy.deepcopy(self.item)
        changed["extra"] = {"S": "x"}
        self.assert_invalid(decode_authority_registration, changed)

    def test_every_attribute_rejects_empty_multiple_and_wrong_av_types(self):
        for field, av in self.item.items():
            for malformed in malformed_avs(av):
                with self.subTest(field=field, malformed=malformed):
                    self.assert_invalid(
                        decode_authority_registration,
                        replace_av(self.item, field, malformed),
                    )

    def test_schema_record_and_partition_key_are_frozen(self):
        changes = {
            "schema_version": {"N": "2"},
            "record_type": {"S": "allocation_state"},
            "pk": {"S": authority_registration_key(b"b" * 32)},
        }
        for field, av in changes.items():
            with self.subTest(field=field):
                self.assert_invalid(
                    decode_authority_registration, replace_av(self.item, field, av),
                )

    def test_exact_container_and_attribute_key_types_prevent_substitution(self):
        self.assert_invalid(decode_authority_registration, DictChild(self.item))
        changed = copy.deepcopy(self.item)
        value = changed.pop("device_id")
        changed[StrChild("device_id")] = value
        self.assert_invalid(decode_authority_registration, changed)
        changed = copy.deepcopy(self.item)
        changed["device_id"] = {StrChild("B"): b"d" * 32}
        self.assert_invalid(decode_authority_registration, changed)

    def test_registration_lengths_key_form_and_revocation_are_strict(self):
        changes = {
            "device_id": b"d" * 31, "authority_id": b"a" * 31,
            "public_key_der": b"x" * 44, "revoked": 0,
        }
        for field, value in changes.items():
            with self.subTest(field=field, direction="encode"):
                self.assert_invalid(
                    encode_authority_registration, replace(self.value, **{field: value}),
                )
            kind = "BOOL" if field == "revoked" else "B"
            with self.subTest(field=field, direction="decode"):
                self.assert_invalid(
                    decode_authority_registration,
                    replace_av(self.item, field, {kind: value}),
                )
        self.assertTrue(decode_authority_registration(
            replace_av(self.item, "revoked", {"BOOL": True})
        ).revoked)

    def test_subclasses_are_rejected_and_value_is_frozen(self):
        child = RegistrationChild(
            self.value.device_id, self.value.authority_id,
            self.value.public_key_der, self.value.revoked,
        )
        self.assert_invalid(encode_authority_registration, child)
        for field in ("device_id", "authority_id", "public_key_der"):
            for constructor in (BytesChild, bytearray):
                changed = replace(
                    self.value, **{field: constructor(getattr(self.value, field))}
                )
                self.assert_invalid(encode_authority_registration, changed)
        with self.assertRaises(FrozenInstanceError):
            self.value.revoked = True


if __name__ == "__main__":
    unittest.main()
