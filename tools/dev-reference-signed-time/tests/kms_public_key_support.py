import hashlib
from collections.abc import Mapping
import unittest

import path_bootstrap
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec

from kms_public_key import KmsPublicKeyError, KmsPublicKeyLoader


class FakeGetPublicKey:
    def __init__(self, result=None, error=None):
        self.result = result
        self.error = error
        self.calls = []

    def __call__(self, **kwargs):
        self.calls.append(kwargs)
        if self.error is not None:
            raise self.error
        return self.result


class DictChild(dict):
    pass


class StrChild(str):
    pass


class BytesChild(bytes):
    pass


class IntChild(int):
    pass


class ListChild(list):
    pass


class HostileMapping(Mapping):
    def __getitem__(self, key):
        raise RuntimeError("secret hostile mapping detail")

    def __iter__(self):
        raise RuntimeError("secret hostile mapping detail")

    def __len__(self):
        raise RuntimeError("secret hostile mapping detail")


class PublicKeyLoaderTestCase(unittest.TestCase):
    def setUp(self):
        self.key_id = "arn:aws:kms:us-east-1:123456789012:key/test-key"
        private_key = ec.generate_private_key(ec.SECP256R1())
        self.public_key_der = private_key.public_key().public_bytes(
            serialization.Encoding.DER,
            serialization.PublicFormat.SubjectPublicKeyInfo,
        )
        self.manifest_sha256 = hashlib.sha256(self.public_key_der).digest()

    def response(self, spec="modern"):
        result = {
            "KeyId": self.key_id,
            "PublicKey": self.public_key_der,
            "KeyUsage": "SIGN_VERIFY",
            "SigningAlgorithms": ["ECDSA_SHA_256"],
            "ResponseMetadata": {
                "HTTPStatusCode": 200,
                "RequestId": "kms-request-1",
            },
        }
        if spec in ("modern", "both"):
            result["KeySpec"] = "ECC_NIST_P256"
        if spec in ("legacy", "both"):
            result["CustomerMasterKeySpec"] = "ECC_NIST_P256"
        return result

    def assert_load_fails(self, result=None, error=None, calls=1):
        getter = FakeGetPublicKey(result=result, error=error)
        loader = KmsPublicKeyLoader(getter, self.key_id, self.manifest_sha256)
        with self.assertRaisesRegex(
            KmsPublicKeyError, "^KMS public key retrieval failed$"
        ) as caught:
            loader.load()
        self.assertEqual(getter.calls, [{"KeyId": self.key_id}] * calls)
        self.assertIsNone(caught.exception.__cause__)
        self.assertIsNone(caught.exception.__context__)
        self.assertNotIn("secret", str(caught.exception))
        return getter
