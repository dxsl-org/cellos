import unittest
from dataclasses import replace
import path_bootstrap

from vector_support import request_fixture

import cbor_codec
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519
from protocol_errors import ProtocolError
from protocol_models import RegisteredAuthority, SignedRequest, UnsignedRequest
from request_protocol import (
    authenticate_request, decode_request, encode_request, parse_request,
    request_signing_bytes,
)


class RequestProtocolTests(unittest.TestCase):
    def setUp(self):
        self.vector, self.request, self.registration = request_fixture()
        self.encoded = bytes.fromhex(self.vector["canonical_cbor_hex"])
        self.values = cbor_codec.loads(self.encoded)

    def reject_values(self, values):
        with self.assertRaises(ProtocolError):
            parse_request(cbor_codec.dumps(values))

    def test_golden_request_round_trips_through_both_explicit_stages(self):
        parsed = parse_request(self.encoded)
        authenticated = authenticate_request(parsed, self.registration)
        self.assertEqual(parsed, self.request)
        self.assertIs(authenticated, parsed)
        self.assertEqual(decode_request(self.encoded, self.registration), parsed)
        self.assertEqual(encode_request(authenticated), self.encoded)
        self.assertEqual(
            request_signing_bytes(parsed), bytes.fromhex(self.vector["signing_cbor_hex"])
        )
        self.assertEqual(parsed.signature.hex(), self.vector["signature_hex"])

    def test_every_signed_label_substitution_is_rejected(self):
        replacements = {
            1: 2,
            2: bytes([self.values[2][0] ^ 1]) + self.values[2][1:],
            3: bytes([self.values[3][0] ^ 1]) + self.values[3][1:],
            4: self.values[4] + 1,
            5: bytes([self.values[5][0] ^ 1]) + self.values[5][1:],
            6: 3,
            7: bytes([self.values[7][0] ^ 1]) + self.values[7][1:],
            8: self.values[8][:-1] + bytes([self.values[8][-1] ^ 1]),
            9: bytes([self.values[9][0] ^ 1]) + self.values[9][1:],
        }
        for label, replacement in replacements.items():
            with self.subTest(label=label):
                changed = dict(self.values)
                changed[label] = replacement
                self.reject_values(changed)

    def test_missing_and_unknown_labels_are_rejected(self):
        for label in range(1, 10):
            with self.subTest(missing=label):
                changed = dict(self.values)
                del changed[label]
                self.reject_values(changed)
        changed = dict(self.values)
        changed[10] = 0
        self.reject_values(changed)

    def test_exact_field_types_and_lengths_are_enforced(self):
        bad_values = {
            2: [b"x" * 31, b"x" * 33, "x" * 32],
            3: [b"x" * 31, b"x" * 33, "x" * 32],
            4: [b"0", "0"],
            5: [b"x" * 15, b"x" * 17, "x" * 16],
            6: [0, 4, b"\x02"],
            7: [b"x" * 31, b"x" * 33, "x" * 32],
            8: [b"x" * 43, b"x" * 45, b"x" * 44],
            9: [b"x" * 63, b"x" * 65, "x" * 64],
        }
        for label, candidates in bad_values.items():
            for candidate in candidates:
                with self.subTest(label=label, value=repr(candidate)[:20]):
                    changed = dict(self.values)
                    changed[label] = candidate
                    self.reject_values(changed)

    def test_uint64_model_boundaries_are_enforced_before_encoding(self):
        for value in (-1, 1 << 64, b"1"):
            with self.subTest(value=value):
                with self.assertRaises(ProtocolError):
                    request_signing_bytes(replace(self.request, boot_epoch=value))
        for purpose in (0, 4, "2"):
            with self.assertRaises(ProtocolError):
                request_signing_bytes(replace(self.request, purpose=purpose))

    def test_registered_tuple_key_and_type_are_exact_bindings(self):
        wrong_device = RegisteredAuthority(
            b"x" * 32, self.registration.authority_id, self.registration.public_key_der
        )
        wrong_authority = RegisteredAuthority(
            self.registration.device_id, b"x" * 32, self.registration.public_key_der
        )
        wrong_key = RegisteredAuthority(
            self.registration.device_id,
            self.registration.authority_id,
            self.registration.public_key_der[:-1] + b"\x00",
        )
        malformed_key = RegisteredAuthority(
            self.registration.device_id, self.registration.authority_id, b"x" * 44
        )
        for registration in (wrong_device, wrong_authority, wrong_key, malformed_key, object()):
            with self.subTest(registration=registration):
                with self.assertRaises(ProtocolError):
                    authenticate_request(self.request, registration)

    def test_arbitrary_self_signed_key_cannot_pass_a_different_registration(self):
        private_key = ed25519.Ed25519PrivateKey.generate()
        public_key = private_key.public_key().public_bytes(
            serialization.Encoding.DER,
            serialization.PublicFormat.SubjectPublicKeyInfo,
        )
        unsigned = UnsignedRequest(
            self.request.device_id,
            self.request.authority_id,
            self.request.boot_epoch,
            self.request.request_id,
            self.request.purpose,
            self.request.nonce,
            public_key,
        )
        request = SignedRequest(
            unsigned.device_id,
            unsigned.authority_id,
            unsigned.boot_epoch,
            unsigned.request_id,
            unsigned.purpose,
            unsigned.nonce,
            unsigned.authority_pubkey,
            private_key.sign(request_signing_bytes(unsigned)),
        )
        self.assertEqual(parse_request(encode_request(request)), request)
        with self.assertRaises(ProtocolError):
            authenticate_request(request, self.registration)

    def test_noncanonical_trailing_and_oversize_requests_are_rejected(self):
        duplicate = bytes.fromhex("aa01010101") + self.encoded[3:]
        for encoded in (duplicate, self.encoded + b"\x00", self.encoded + b"\x00" * 1024):
            with self.subTest(length=len(encoded)):
                with self.assertRaises(ProtocolError):
                    parse_request(encoded)
        for non_bytes in (bytearray(self.encoded), memoryview(self.encoded), None):
            with self.assertRaises(ProtocolError):
                parse_request(non_bytes)

    def test_invalid_signature_cannot_be_encoded(self):
        with self.assertRaises(ProtocolError):
            encode_request(replace(self.request, signature=b"\x00" * 64))


if __name__ == "__main__":
    unittest.main()
