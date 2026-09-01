import unittest

from kms_public_key_support import FakeGetPublicKey, PublicKeyLoaderTestCase
from kms_public_key import KmsPublicKeyLoader
from kms_signer import KmsSigner


class SurfaceGuardKmsClient:
    def __init__(self, result):
        self.result = result
        self.get_calls = []
        self.forbidden_calls = []

    def get_public_key(self, **kwargs):
        self.get_calls.append(kwargs)
        return self.result

    def sign(self, **kwargs):
        self.forbidden_calls.append(("sign", kwargs))
        raise AssertionError("sign must not be called by key retrieval")

    def decrypt(self, **kwargs):
        self.forbidden_calls.append(("decrypt", kwargs))
        raise AssertionError("decrypt must not be called")

    def encrypt(self, **kwargs):
        self.forbidden_calls.append(("encrypt", kwargs))
        raise AssertionError("encrypt must not be called")


class KmsPublicKeySuccessTests(PublicKeyLoaderTestCase):
    def test_modern_legacy_and_consistent_dual_spec_return_exact_bytes_once(self):
        for spec in ("modern", "legacy", "both"):
            with self.subTest(spec=spec):
                getter = FakeGetPublicKey(self.response(spec))
                loaded = KmsPublicKeyLoader(
                    getter, self.key_id, self.manifest_sha256
                ).load()
                self.assertIs(loaded, self.public_key_der)
                self.assertEqual(loaded, self.public_key_der)
                self.assertEqual(getter.calls, [{"KeyId": self.key_id}])

    def test_exact_list_may_contain_other_exact_algorithm_strings(self):
        result = self.response()
        result["SigningAlgorithms"] = ["ECDSA_SHA_384", "ECDSA_SHA_256"]
        getter = FakeGetPublicKey(result)
        self.assertEqual(
            KmsPublicKeyLoader(getter, self.key_id, self.manifest_sha256).load(),
            self.public_key_der,
        )
        self.assertEqual(getter.calls, [{"KeyId": self.key_id}])

    def test_realistic_extra_response_metadata_is_accepted(self):
        result = self.response()
        result["ResponseMetadata"].update({"RetryAttempts": 0, "HTTPHeaders": {}})
        result["EncryptionAlgorithms"] = []
        getter = FakeGetPublicKey(result)
        self.assertEqual(
            KmsPublicKeyLoader(getter, self.key_id, self.manifest_sha256).load(),
            self.public_key_der,
        )

    def test_boto3_shaped_method_is_composable_with_signer_without_other_calls(self):
        client = SurfaceGuardKmsClient(self.response())
        loaded = KmsPublicKeyLoader(
            client.get_public_key, self.key_id, self.manifest_sha256
        ).load()
        signer = KmsSigner(client, self.key_id, loaded)
        self.assertIsInstance(signer, KmsSigner)
        self.assertEqual(client.get_calls, [{"KeyId": self.key_id}])
        self.assertEqual(client.forbidden_calls, [])
        self.assertFalse(hasattr(KmsPublicKeyLoader, "sign"))
        self.assertFalse(hasattr(KmsPublicKeyLoader, "decrypt"))
        self.assertFalse(hasattr(KmsPublicKeyLoader, "encrypt"))


if __name__ == "__main__":
    unittest.main()
