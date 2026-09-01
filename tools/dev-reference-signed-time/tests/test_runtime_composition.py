import hashlib
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec

import path_bootstrap  # noqa: F401

from handler import SignedTimeHandler
from lineage import encode_transition
from lineage_test_support import LINEAGE_KEY_ID, LINEAGE_PUBLIC_DER, signed_transition
from manifest import encode_manifest
from manifest_test_support import valid_manifest
from runtime_composition import (
    RuntimeCompositionError, _default_client_factory, compose_runtime,
)


def environment(manifest):
    return {
        "AWS_REGION": manifest.aws_region,
        "SIGNED_TIME_TABLE_NAME": manifest.allocator_table_name,
        "SIGNED_TIME_LINEAGE_TABLE_NAME": manifest.lineage_table_name,
        "SIGNED_TIME_KMS_KEY_ARN": manifest.kms_key_id,
        "SIGNED_TIME_LINEAGE_KMS_KEY_ARN": manifest.lineage_kms_key_id,
    }


def public_der(private_key):
    return private_key.public_key().public_bytes(
        serialization.Encoding.DER,
        serialization.PublicFormat.SubjectPublicKeyInfo,
    )


class FakeKms:
    def __init__(self, keys):
        self.keys = keys
        self.calls = []

    def get_public_key(self, *, KeyId):
        self.calls.append(KeyId)
        return {
            "KeyId": KeyId,
            "PublicKey": self.keys[KeyId],
            "KeySpec": "ECC_NIST_P256",
            "KeyUsage": "SIGN_VERIFY",
            "SigningAlgorithms": ["ECDSA_SHA_256"],
            "ResponseMetadata": {"HTTPStatusCode": 200, "RequestId": "kms-request"},
        }

    def sign(self, **_kwargs):
        raise AssertionError("composition must not sign")


class FakeDynamo:
    def __init__(self, manifest, substituted=False):
        self.manifest = manifest
        self.substituted = substituted
        self.calls = []

    def describe_table(self, *, TableName):
        self.calls.append(TableName)
        identities = {
            self.manifest.allocator_table_name: self.manifest.allocator_table_id,
            self.manifest.lineage_table_name: self.manifest.lineage_table_id,
        }
        table_id = identities[TableName]
        if self.substituted:
            table_id = "ffffffff-ffff-4fff-8fff-ffffffffffff"
        return {
            "Table": {
                "TableName": TableName,
                "TableId": table_id,
                "TableStatus": "ACTIVE",
                "DeletionProtectionEnabled": True,
            },
            "ResponseMetadata": {"HTTPStatusCode": 200, "RequestId": "ddb-request"},
        }

    def transact_get_items(self, **_kwargs):
        raise AssertionError("composition must not read state")

    def transact_write_items(self, **_kwargs):
        raise AssertionError("composition must not write state")


class RuntimeCompositionTests(unittest.TestCase):
    def setUp(self):
        response_der = public_der(ec.derive_private_key(9, ec.SECP256R1()))
        response_digest = hashlib.sha256(response_der).digest()
        transition = signed_transition(response_key_digest=response_digest)
        self.manifest = valid_manifest(
            kms_public_key_der_sha256=response_digest,
            lineage_transition=encode_transition(transition),
        )
        self.kms = FakeKms(
            {self.manifest.kms_key_id: response_der, LINEAGE_KEY_ID: LINEAGE_PUBLIC_DER}
        )
        self.dynamo = FakeDynamo(self.manifest)
        self.directory = tempfile.TemporaryDirectory()
        self.path = Path(self.directory.name, "manifest.json")
        self.path.write_bytes(encode_manifest(self.manifest))
        self.factory_calls = []

    def tearDown(self):
        self.directory.cleanup()

    def factory(self, service, region):
        self.factory_calls.append((service, region))
        return {"dynamodb": self.dynamo, "kms": self.kms}[service]

    def test_composes_exact_clients_keys_lineage_tables_and_handler(self):
        runtime = compose_runtime(self.path, environment(self.manifest), self.factory)
        self.assertIs(type(runtime), SignedTimeHandler)
        self.assertEqual(
            self.factory_calls,
            [("dynamodb", self.manifest.aws_region), ("kms", self.manifest.aws_region)],
        )
        self.assertEqual(self.kms.calls, [self.manifest.kms_key_id, LINEAGE_KEY_ID])
        self.assertEqual(
            self.dynamo.calls,
            [self.manifest.allocator_table_name, self.manifest.lineage_table_name],
        )

    def test_substituted_live_table_identity_fails_closed(self):
        self.dynamo.substituted = True
        with self.assertRaisesRegex(
            RuntimeCompositionError, "^signed-time runtime composition failed$"
        ):
            compose_runtime(self.path, environment(self.manifest), self.factory)

    def test_invalid_factory_fails_before_client_construction(self):
        with self.assertRaisesRegex(
            RuntimeCompositionError, "^signed-time runtime composition failed$"
        ):
            compose_runtime(self.path, environment(self.manifest), None)

    def test_default_clients_ignore_ambient_endpoint_overrides(self):
        calls = []

        def client(service, **arguments):
            calls.append((service, arguments))
            return object()

        config_module = SimpleNamespace(Config=lambda **arguments: arguments)
        modules = {
            "boto3": SimpleNamespace(client=client),
            "botocore": SimpleNamespace(config=config_module),
            "botocore.config": config_module,
        }
        with patch.dict(sys.modules, modules):
            result = _default_client_factory("kms", "us-east-1")
        self.assertIs(type(result), object)
        self.assertEqual(calls[0][0], "kms")
        self.assertEqual(calls[0][1]["region_name"], "us-east-1")
        self.assertEqual(
            calls[0][1]["config"],
            {
                "connect_timeout": 1,
                "ignore_configured_endpoint_urls": True,
                "read_timeout": 3,
                "retries": {"total_max_attempts": 1, "mode": "standard"},
            },
        )


if __name__ == "__main__":
    unittest.main()
