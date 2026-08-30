import unittest
from dataclasses import replace

from vector_support import request_fixture, response_fixture, unsigned_request

import cbor_codec
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519, utils
from protocol import (
    ProtocolError, decode_response, encode_response, response_signing_bytes,
    response_signing_digest,
)
from protocol_crypto import (
    CryptoError, P256_ORDER, canonicalize_p256_der_signature,
    parse_p256_der_signature,
)
from protocol_models import MAX_RESPONSE_BYTES


class ResponseProtocolTests(unittest.TestCase):
    def setUp(self):
        self.vector, self.response = response_fixture()
        _, request, _ = request_fixture()
        self.request = unsigned_request(request)
        self.encoded = bytes.fromhex(self.vector["canonical_cbor_hex"])
        self.public_key = bytes.fromhex(self.vector["kms_public_key_der_hex"])
        self.values = cbor_codec.loads(self.encoded)

    def decode(self, encoded=None, request=None, key=None, key_id=None, source_epoch=None):
        return decode_response(
            self.encoded if encoded is None else encoded,
            self.public_key if key is None else key,
            self.response.key_id if key_id is None else key_id,
            self.response.source_epoch if source_epoch is None else source_epoch,
            self.request if request is None else request,
        )

    def reject_values(self, values):
        with self.assertRaises(ProtocolError):
            self.decode(cbor_codec.dumps(values))

    def test_golden_response_bytes_digest_and_signature_verify(self):
        decoded = self.decode()
        self.assertEqual(decoded, self.response)
        self.assertEqual(encode_response(decoded), self.encoded)
        self.assertEqual(response_signing_bytes(decoded), bytes.fromhex(self.vector["signing_cbor_hex"]))
        self.assertEqual(response_signing_digest(decoded).hex(), self.vector["signing_digest_sha256_hex"])
        self.assertEqual(decoded.signature.hex(), self.vector["signature_der_hex"])

    def test_every_response_label_substitution_is_rejected(self):
        replacements = {
            1: 2, 2: "other-source", 3: 8, 4: 43, 5: self.values[5] + 1,
            6: self.values[6] - 1, 7: b"x" * 32, 8: b"y" * 32, 9: self.values[9] + 1,
            10: b"z" * 16, 11: 3, 12: b"n" * 32, 13: "other-key",
            14: "OTHER_ALGORITHM", 15: self.values[15][:-1] + bytes([self.values[15][-1] ^ 1]),
        }
        for label, replacement in replacements.items():
            with self.subTest(label=label):
                changed = dict(self.values)
                changed[label] = replacement
                self.reject_values(changed)

    def test_missing_unknown_and_wrong_schema_source_algorithm_are_rejected(self):
        for label in range(1, 16):
            changed = dict(self.values)
            del changed[label]
            with self.subTest(missing=label):
                self.reject_values(changed)
        for label, value in ((16, 0), (1, "1"), (2, b"cellos-dev-time-v1"),
                             (14, b"ECDSA_SHA_256")):
            changed = dict(self.values)
            changed[label] = value
            with self.subTest(label=label):
                self.reject_values(changed)

    def test_exact_types_lengths_purpose_and_uint64_are_enforced(self):
        cases = {
            3: [-1, b"7"], 4: [-1, b"42"], 5: [-1, "0"], 6: [-1, "0"], 9: [-1, b"0"],
            7: [b"x" * 31, b"x" * 33, "x" * 32],
            8: [b"x" * 31, b"x" * 33, "x" * 32],
            10: [b"x" * 15, b"x" * 17, "x" * 16],
            11: [0, 4, b"2"], 12: [b"x" * 31, b"x" * 33, "x" * 32],
            13: [b"key", ""], 15: [b"", b"x" * 73, "signature"],
        }
        for label, candidates in cases.items():
            for candidate in candidates:
                changed = dict(self.values)
                changed[label] = candidate
                with self.subTest(label=label, value=repr(candidate)[:20]):
                    try:
                        encoded = cbor_codec.dumps(changed)
                    except cbor_codec.CborError:
                        continue
                    with self.assertRaises(ProtocolError):
                        self.decode(encoded)
        for name in ("source_epoch", "source_sequence", "unix_seconds", "expires_at", "boot_epoch"):
            with self.assertRaises(ProtocolError):
                response_signing_bytes(replace(self.response, **{name: 1 << 64}))

    def test_response_must_bind_every_request_claim(self):
        changes = {
            "device_id": b"x" * 32, "authority_id": b"y" * 32,
            "boot_epoch": self.request.boot_epoch + 1, "request_id": b"z" * 16,
            "purpose": 3, "nonce": b"n" * 32,
        }
        for name, value in changes.items():
            with self.subTest(name=name):
                with self.assertRaises(ProtocolError):
                    self.decode(request=replace(self.request, **{name: value}))

    def test_response_requires_validated_exact_source_epoch(self):
        for source_epoch in (self.response.source_epoch + 1, -1, 1 << 64, True, "7"):
            with self.subTest(source_epoch=source_epoch):
                with self.assertRaises(ProtocolError):
                    self.decode(source_epoch=source_epoch)

    def test_manifest_key_id_and_p256_key_are_exact(self):
        with self.assertRaises(ProtocolError):
            self.decode(key_id="other-key")
        wrong_p256 = ec.derive_private_key(2, ec.SECP256R1()).public_key().public_bytes(
            serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)
        p384 = ec.derive_private_key(2, ec.SECP384R1()).public_key().public_bytes(
            serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)
        ed_key = ed25519.Ed25519PrivateKey.from_private_bytes(b"\x02" * 32).public_key().public_bytes(
            serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)
        for key in (wrong_p256, p384, ed_key, b"not-der"):
            with self.subTest(key_length=len(key)):
                with self.assertRaises(ProtocolError):
                    self.decode(key=key)

    def test_strict_der_rejects_malleable_or_out_of_range_forms(self):
        valid = bytes.fromhex(self.vector["signature_der_hex"])
        parse_p256_der_signature(valid)
        invalid = [
            b"\x30\x06\x02\x01\x00\x02\x01\x01",
            b"\x30\x06\x02\x01\x80\x02\x01\x01",
            b"\x30\x07\x02\x02\x00\x01\x02\x01\x01",
            valid + b"\x00", b"\x30\x81" + valid[2:],
            utils.encode_dss_signature(P256_ORDER, 1),
        ]
        for signature in invalid:
            with self.subTest(signature=signature.hex()[:24]):
                with self.assertRaises(CryptoError):
                    parse_p256_der_signature(signature)
                with self.assertRaises(ProtocolError):
                    encode_response(replace(self.response, signature=signature))

    def test_high_s_equivalent_is_normalized_for_assembly_but_rejected_on_wire(self):
        low = bytes.fromhex(self.vector["signature_der_hex"])
        r, s = parse_p256_der_signature(low)
        high = utils.encode_dss_signature(r, P256_ORDER - s)
        self.assertEqual(canonicalize_p256_der_signature(high), low)
        with self.assertRaises(CryptoError):
            parse_p256_der_signature(high)
        with self.assertRaises(ProtocolError):
            encode_response(replace(self.response, signature=high))
        changed = dict(self.values)
        changed[15] = high
        self.reject_values(changed)
        self.assertEqual(
            response_signing_digest(self.response),
            response_signing_digest(replace(self.response, signature=high)),
        )

    def test_kms_digest_mode_signature_round_trip(self):
        private_key = ec.derive_private_key(1, ec.SECP256R1())
        digest = response_signing_digest(self.response)
        kms_signature = private_key.sign(
            digest, ec.ECDSA(utils.Prehashed(hashes.SHA256())))
        signature = canonicalize_p256_der_signature(kms_signature)
        encoded = encode_response(replace(self.response, signature=signature))
        public_der = private_key.public_key().public_bytes(
            serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)
        self.assertEqual(self.decode(encoded=encoded, key=public_der).signature, signature)

    def test_expiry_interval_is_strict_and_at_most_sixty_seconds(self):
        for expires in (self.response.unix_seconds, self.response.unix_seconds + 61):
            with self.assertRaises(ProtocolError):
                response_signing_digest(replace(self.response, expires_at=expires))

    def test_response_wire_size_bound_is_enforced_before_decode_and_after_encode(self):
        with self.assertRaises(ProtocolError):
            self.decode(encoded=b"\x00" * (MAX_RESPONSE_BYTES + 1))
        with self.assertRaises(ProtocolError):
            encode_response(replace(self.response, key_id="k" * MAX_RESPONSE_BYTES))

    def test_response_codec_errors_are_protocol_errors_at_public_boundaries(self):
        invalid = replace(self.response, key_id="\ud800")
        for operation in (response_signing_bytes, response_signing_digest, encode_response):
            with self.subTest(operation=operation.__name__):
                with self.assertRaises(ProtocolError):
                    operation(invalid)


if __name__ == "__main__":
    unittest.main()
