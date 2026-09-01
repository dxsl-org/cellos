import hashlib
import hmac
import unittest
from unittest.mock import patch

import path_bootstrap

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec

from kms_public_key import KmsPublicKeyError, KmsPublicKeyLoader
from kms_public_key_support import (
    BytesChild,
    FakeGetPublicKey,
    PublicKeyLoaderTestCase,
    StrChild,
)


class KmsPublicKeyCryptoTests(PublicKeyLoaderTestCase):
    def test_constructor_rejects_invalid_callable_key_id_and_digest_without_calling(self):
        valid_getter = FakeGetPublicKey(self.response())
        invalid = [
            (None, self.key_id, self.manifest_sha256),
            (object(), self.key_id, self.manifest_sha256),
            (valid_getter, "", self.manifest_sha256),
            (valid_getter, b"key", self.manifest_sha256),
            (valid_getter, StrChild(self.key_id), self.manifest_sha256),
            (valid_getter, self.key_id, b""),
            (valid_getter, self.key_id, b"x" * 31),
            (valid_getter, self.key_id, b"x" * 33),
            (valid_getter, self.key_id, bytearray(self.manifest_sha256)),
            (valid_getter, self.key_id, BytesChild(self.manifest_sha256)),
        ]
        for get_public_key, key_id, digest in invalid:
            with self.subTest(callable=callable(get_public_key), key_type=type(key_id).__name__,
                              digest_type=type(digest).__name__, digest_length=len(digest)):
                with self.assertRaisesRegex(
                    KmsPublicKeyError,
                    "^invalid KMS public key loader configuration$",
                ) as caught:
                    KmsPublicKeyLoader(get_public_key, key_id, digest)
                self.assertIsNone(caught.exception.__cause__)
                self.assertIsNone(caught.exception.__context__)
        self.assertEqual(valid_getter.calls, [])

    def test_constructor_requires_the_injected_method_not_a_generic_client(self):
        class Client:
            get_public_key = FakeGetPublicKey(self.response())

        with self.assertRaisesRegex(
            KmsPublicKeyError, "^invalid KMS public key loader configuration$"
        ):
            KmsPublicKeyLoader(Client(), self.key_id, self.manifest_sha256)

    def test_malformed_noncanonical_and_wrong_curve_keys_fail_once(self):
        p384_private = ec.generate_private_key(ec.SECP384R1())
        p384_der = p384_private.public_key().public_bytes(
            serialization.Encoding.DER,
            serialization.PublicFormat.SubjectPublicKeyInfo,
        )
        noncanonical = b"\x30\x81" + self.public_key_der[1:]
        for public_key in (b"not-der", noncanonical, p384_der):
            result = self.response()
            result["PublicKey"] = public_key
            with self.subTest(key_length=len(public_key)):
                self.assert_load_fails(result=result)

    def test_valid_wrong_p256_key_fails_manifest_digest_pin_once(self):
        other_private = ec.generate_private_key(ec.SECP256R1())
        other_der = other_private.public_key().public_bytes(
            serialization.Encoding.DER,
            serialization.PublicFormat.SubjectPublicKeyInfo,
        )
        result = self.response()
        result["PublicKey"] = other_der
        self.assert_load_fails(result=result)

    def test_digest_comparison_uses_constant_time_primitive(self):
        getter = FakeGetPublicKey(self.response())
        with patch(
            "kms_public_key.hmac.compare_digest", wraps=hmac.compare_digest
        ) as compare_digest:
            loaded = KmsPublicKeyLoader(
                getter, self.key_id, self.manifest_sha256
            ).load()
        compare_digest.assert_called_once_with(
            hashlib.sha256(self.public_key_der).digest(), self.manifest_sha256
        )
        self.assertEqual(loaded, self.public_key_der)

    def test_provider_detail_is_suppressed_without_retry_or_exception_chain(self):
        getter = self.assert_load_fails(
            error=RuntimeError("secret account, region, request, and credential detail")
        )
        self.assertEqual(getter.calls, [{"KeyId": self.key_id}])


if __name__ == "__main__":
    unittest.main()
