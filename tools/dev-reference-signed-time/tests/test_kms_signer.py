import unittest
from dataclasses import replace
import path_bootstrap

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, utils

from kms_signer import KmsSigner, KmsSignerError
from protocol import encode_response, response_signing_digest
from protocol_crypto import P256_ORDER, canonicalize_p256_der_signature
from protocol_models import SIGNING_ALGORITHM, SignedResponse, UnsignedResponse


class FakeKmsClient:
    def __init__(self, result=None, error=None):
        self.result = result
        self.error = error
        self.calls = []

    def sign(self, **kwargs):
        self.calls.append(kwargs)
        if self.error is not None:
            raise self.error
        return self.result


class KmsSignerTests(unittest.TestCase):
    def setUp(self):
        self.key_id = "arn:aws:kms:us-east-1:123456789012:key/test-key"
        self.private_key = ec.generate_private_key(ec.SECP256R1())
        self.public_key_der = self.private_key.public_key().public_bytes(
            serialization.Encoding.DER,
            serialization.PublicFormat.SubjectPublicKeyInfo,
        )
        self.unsigned = UnsignedResponse(
            source_epoch=7,
            source_sequence=42,
            unix_seconds=1_700_000_000,
            expires_at=1_700_000_030,
            device_id=b"d" * 32,
            authority_id=b"a" * 32,
            boot_epoch=3,
            request_id=b"r" * 16,
            purpose=2,
            nonce=b"n" * 32,
            key_id=self.key_id,
        )
        self.digest = response_signing_digest(self.unsigned)

    def kms_result(self, signature, key_id=None, algorithm=None):
        return {
            "KeyId": self.key_id if key_id is None else key_id,
            "SigningAlgorithm": SIGNING_ALGORITHM if algorithm is None else algorithm,
            "Signature": signature,
            "ResponseMetadata": {"HTTPStatusCode": 200},
        }

    def kms_signature(self, key=None):
        key = self.private_key if key is None else key
        return key.sign(
            self.digest,
            ec.ECDSA(utils.Prehashed(hashes.SHA256())),
        )

    def expected_response(self, signature):
        return SignedResponse(
            source_epoch=self.unsigned.source_epoch,
            source_sequence=self.unsigned.source_sequence,
            unix_seconds=self.unsigned.unix_seconds,
            expires_at=self.unsigned.expires_at,
            device_id=self.unsigned.device_id,
            authority_id=self.unsigned.authority_id,
            boot_epoch=self.unsigned.boot_epoch,
            request_id=self.unsigned.request_id,
            purpose=self.unsigned.purpose,
            nonce=self.unsigned.nonce,
            key_id=self.unsigned.key_id,
            signature=signature,
        )

    def assert_failed_closed(self, result=None, error=None, calls=1, response=None):
        client = FakeKmsClient(result=result, error=error)
        signer = KmsSigner(client, self.key_id, self.public_key_der)
        with self.assertRaisesRegex(KmsSignerError, "^KMS signing failed$") as caught:
            signer.sign_response(self.unsigned if response is None else response)
        self.assertIsNone(caught.exception.__cause__)
        self.assertIsNone(caught.exception.__context__)
        self.assertEqual(len(client.calls), calls)

    def test_exact_one_call_and_exact_low_or_high_s_result(self):
        low = canonicalize_p256_der_signature(self.kms_signature())
        r, s = utils.decode_dss_signature(low)
        high = utils.encode_dss_signature(r, P256_ORDER - s)
        original = self.unsigned
        for kms_signature in (low, high):
            with self.subTest(high_s=kms_signature == high):
                client = FakeKmsClient(self.kms_result(kms_signature))
                signed = KmsSigner(client, self.key_id, self.public_key_der).sign_response(
                    self.unsigned
                )
                expected = self.expected_response(low)
                self.assertEqual(signed, expected)
                self.assertEqual(encode_response(signed), encode_response(expected))
                self.assertEqual(self.unsigned, original)
                self.assertEqual(client.calls, [{
                    "KeyId": self.key_id,
                    "Message": self.digest,
                    "MessageType": "DIGEST",
                    "SigningAlgorithm": "ECDSA_SHA_256",
                }])

    def test_constructor_rejects_invalid_client_key_id_and_public_key(self):
        class NonCallableClient:
            sign = None

        p384_der = ec.generate_private_key(ec.SECP384R1()).public_key().public_bytes(
            serialization.Encoding.DER,
            serialization.PublicFormat.SubjectPublicKeyInfo,
        )
        cases = [
            (object(), self.key_id, self.public_key_der),
            (NonCallableClient(), self.key_id, self.public_key_der),
            (FakeKmsClient(), "", self.public_key_der),
            (FakeKmsClient(), b"key", self.public_key_der),
            (FakeKmsClient(), self.key_id, b"not-der"),
            (FakeKmsClient(), self.key_id, p384_der),
        ]
        for client, key_id, public_key in cases:
            with self.subTest(key_id=key_id, key_length=len(public_key)):
                with self.assertRaisesRegex(
                        KmsSignerError, "^invalid KMS signer configuration$") as caught:
                    KmsSigner(client, key_id, public_key)
                self.assertIsNone(caught.exception.__cause__)
                self.assertIsNone(caught.exception.__context__)

    def test_client_exception_and_non_mapping_or_missing_fields_fail_once(self):
        self.assert_failed_closed(error=RuntimeError("secret remote detail"))
        valid = self.kms_result(self.kms_signature())
        cases = [None, [], {}, {key: value for key, value in valid.items() if key != "KeyId"},
                 {key: value for key, value in valid.items() if key != "SigningAlgorithm"},
                 {key: value for key, value in valid.items() if key != "Signature"}]
        for result in cases:
            with self.subTest(result=result):
                self.assert_failed_closed(result=result)

    def test_wrong_key_algorithm_and_field_types_fail_once(self):
        signature = self.kms_signature()
        cases = [
            self.kms_result(signature, key_id="other-key"),
            self.kms_result(signature, key_id=b"key"),
            self.kms_result(signature, algorithm="RSASSA_PSS_SHA_256"),
            self.kms_result(signature, algorithm=b"ECDSA_SHA_256"),
        ]
        for result in cases:
            with self.subTest(result=result):
                self.assert_failed_closed(result=result)

    def test_non_bytes_malformed_out_of_range_and_wrong_key_signatures_fail_once(self):
        wrong_key = ec.generate_private_key(ec.SECP256R1())
        cases = [
            bytearray(self.kms_signature()),
            b"not-der",
            utils.encode_dss_signature(P256_ORDER, 1),
            self.kms_signature(wrong_key),
        ]
        for signature in cases:
            with self.subTest(signature=repr(signature)[:32]):
                self.assert_failed_closed(result=self.kms_result(signature))

    def test_encoded_response_size_failure_is_closed_after_one_call(self):
        oversized = replace(self.unsigned, key_id="k" * 1024)
        digest = response_signing_digest(oversized)
        signature = self.private_key.sign(
            digest, ec.ECDSA(utils.Prehashed(hashes.SHA256())))
        client = FakeKmsClient({
            "KeyId": oversized.key_id,
            "SigningAlgorithm": SIGNING_ALGORITHM,
            "Signature": signature,
        })
        with self.assertRaisesRegex(KmsSignerError, "^KMS signing failed$"):
            KmsSigner(client, oversized.key_id, self.public_key_der).sign_response(oversized)
        self.assertEqual(len(client.calls), 1)

    def test_invalid_or_mismatched_unsigned_response_never_calls_kms(self):
        cases = [
            replace(self.unsigned, expires_at=self.unsigned.unix_seconds),
            replace(self.unsigned, key_id="other-key"),
            self.expected_response(canonicalize_p256_der_signature(self.kms_signature())),
            object(),
        ]
        for response in cases:
            with self.subTest(response_type=type(response).__name__):
                self.assert_failed_closed(result=self.kms_result(self.kms_signature()),
                                          calls=0, response=response)


if __name__ == "__main__":
    unittest.main()
