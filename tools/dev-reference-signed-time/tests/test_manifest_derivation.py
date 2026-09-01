import builtins
import os
import socket
import unittest
from dataclasses import replace
from unittest.mock import patch

import path_bootstrap
from clock_policy import ClockPolicy
from manifest import (
    ManifestError, decode_manifest, derive_clock_policy,
    derive_kms_key_pins, encode_manifest,
)
from manifest_test_support import GOLDEN, KMS_ARN, KMS_DIGEST, valid_manifest


class ManifestDerivationTests(unittest.TestCase):
    def test_clock_policy_derives_only_the_four_pinned_policy_values(self):
        manifest = valid_manifest()
        policy = derive_clock_policy(manifest)
        self.assertEqual(
            policy,
            ClockPolicy(
                upstream_identity="authenticated-clock.example.com",
                source_epoch=7,
                max_sample_age_seconds=5,
                max_uncertainty_seconds=2,
            ),
        )

    def test_kms_key_pins_derive_exact_key_id_and_binary_digest(self):
        key_id, digest = derive_kms_key_pins(valid_manifest())
        self.assertEqual(key_id, KMS_ARN)
        self.assertEqual(digest, KMS_DIGEST)
        self.assertIs(type(key_id), str)
        self.assertIs(type(digest), bytes)

    def test_derivations_revalidate_the_manifest(self):
        invalid = replace(valid_manifest(), source_epoch=True)
        for helper in (derive_clock_policy, derive_kms_key_pins):
            with self.subTest(helper=helper.__name__):
                with self.assertRaises(ManifestError):
                    helper(invalid)

    def test_codec_and_derivations_have_no_ambient_access(self):
        denied = AssertionError("ambient access")
        with (
            patch.object(builtins, "open", side_effect=denied),
            patch.object(os, "getenv", side_effect=denied),
            patch.object(socket, "socket", side_effect=denied),
            patch.object(builtins, "__import__", side_effect=denied),
        ):
            manifest = decode_manifest(GOLDEN)
            self.assertEqual(encode_manifest(manifest), GOLDEN)
            self.assertIsInstance(derive_clock_policy(manifest), ClockPolicy)
            self.assertEqual(derive_kms_key_pins(manifest)[0], KMS_ARN)

    def test_all_failures_have_one_value_free_detail_and_no_exception_chain(self):
        failures = (
            lambda: decode_manifest(b"\xff"),
            lambda: decode_manifest(b"{"),
            lambda: decode_manifest(
                b'{"schema_version":1,"schema_version":1}',
            ),
            lambda: encode_manifest(
                valid_manifest(endpoint_url="https://host.example:bad/v1/time"),
            ),
            lambda: encode_manifest(
                valid_manifest(kms_key_id="not-an-arn"),
            ),
        )
        for fail in failures:
            with self.subTest(failure=fail):
                with self.assertRaises(ManifestError) as caught:
                    fail()
                error = caught.exception
                self.assertEqual(str(error), "invalid signed-time manifest")
                self.assertEqual(error.args, ("invalid signed-time manifest",))
                self.assertIsNone(error.__cause__)
                self.assertIsNone(error.__context__)


if __name__ == "__main__":
    unittest.main()
