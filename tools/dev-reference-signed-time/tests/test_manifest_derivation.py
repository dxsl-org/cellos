import builtins
import os
import socket
import unittest
from dataclasses import replace
from unittest.mock import patch

import path_bootstrap
from clock_policy import ClockPolicy
from manifest import ManifestError, decode_manifest, encode_manifest
from manifest_derivation import (
    derive_clock_policy, derive_kms_key_pins, derive_lineage_contract,
    derive_lineage_key_pins, derive_roughtime_config,
)
from lineage import encode_transition
from manifest_model import PRODUCTION_REJECTION_MARKERS
from manifest_test_support import GOLDEN, KMS_ARN, KMS_DIGEST, valid_manifest
from lineage_test_support import (
    LINEAGE_KEY_ID, LINEAGE_PUBLIC_DER, LINEAGE_PUBLIC_DIGEST, contract,
    signed_transition,
)
from roughtime_config import provider_config


class ManifestDerivationTests(unittest.TestCase):
    def test_clock_policy_derives_only_the_four_pinned_policy_values(self):
        manifest = valid_manifest()
        policy = derive_clock_policy(manifest)
        self.assertEqual(
            policy,
            ClockPolicy(
                upstream_identity="roughtime.cloudflare.com",
                source_epoch=1,
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

    def test_lineage_key_and_transition_derive_exact_pins(self):
        manifest = valid_manifest()
        self.assertEqual(
            derive_lineage_key_pins(manifest),
            (LINEAGE_KEY_ID, LINEAGE_PUBLIC_DIGEST),
        )
        contract = derive_lineage_contract(manifest, LINEAGE_PUBLIC_DER)
        self.assertEqual(contract.transition.source_epoch, manifest.source_epoch)
        self.assertEqual(
            contract.transition.allocator_table_id, manifest.allocator_table_id,
        )

    def test_lineage_derivation_rejects_every_manifest_binding_substitution(self):
        substitutions = (
            {"source_epoch": 2},
            {"allocator_table_id": "99999999-2222-4333-8444-555555555555"},
            {"kms_key_id": (
                "arn:aws:kms:us-east-1:000000000000:key/"
                "99999999-2222-4333-8444-555555555555"
            )},
            {"kms_public_key_der_sha256": bytes.fromhex("99" * 32)},
        )
        for changes in substitutions:
            with self.subTest(changes=changes):
                with self.assertRaises(ManifestError):
                    derive_lineage_contract(
                        valid_manifest(**changes), LINEAGE_PUBLIC_DER,
                    )
        with self.assertRaises(ManifestError):
            derive_lineage_contract(valid_manifest(), LINEAGE_PUBLIC_DER + b"x")

    def test_post_genesis_manifest_cold_start_authenticates_current_head(self):
        parent = contract()
        transition = signed_transition(
            epoch=2,
            parent_digest=parent.transition_digest,
            table_name="cellos-dev-signed-time-allocator-restored",
            table_id="88888888-2222-4333-8444-555555555555",
            response_key_id=(
                "arn:aws:kms:us-east-1:000000000000:key/"
                "77777777-2222-4333-8444-555555555555"
            ),
            response_key_digest=bytes.fromhex("77" * 32),
            reason="restore",
        )
        manifest = valid_manifest(
            allocator_table_name=transition.allocator_table_name,
            allocator_table_id=transition.allocator_table_id,
            source_epoch=2,
            kms_key_id=transition.response_key_id,
            kms_public_key_der_sha256=transition.response_public_key_der_sha256,
            lineage_transition=encode_transition(transition),
        )
        selected = derive_lineage_contract(manifest, LINEAGE_PUBLIC_DER)
        self.assertEqual(selected.transition, transition)


    def test_roughtime_config_derives_every_exact_provider_pin(self):
        self.assertEqual(
            derive_roughtime_config(valid_manifest()), provider_config(),
        )

    def test_production_rejection_handoff_is_exact_and_immutable(self):
        self.assertIs(type(PRODUCTION_REJECTION_MARKERS), frozenset)
        self.assertEqual(
            PRODUCTION_REJECTION_MARKERS,
            {
                "AWS_DEV_SIGNED_TIME",
                "DEV_REFERENCE",
                "SOFTWARE_HARNESS",
                "aws-dev-signed-time",
                "cellos-dev-time-v1",
            },
        )

    def test_derivations_revalidate_the_manifest(self):
        invalid = replace(valid_manifest(), source_epoch=True)
        for helper in (
            derive_clock_policy, derive_kms_key_pins, derive_lineage_key_pins,
            derive_roughtime_config,
        ):
            with self.subTest(helper=helper.__name__):
                with self.assertRaises(ManifestError):
                    helper(invalid)
        with self.assertRaises(ManifestError):
            derive_lineage_contract(invalid, LINEAGE_PUBLIC_DER)

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
            self.assertEqual(
                derive_roughtime_config(manifest), provider_config(),
            )

    def test_lineage_derivation_has_no_file_environment_or_network_access(self):
        denied = AssertionError("ambient access")
        with (
            patch.object(builtins, "open", side_effect=denied),
            patch.object(os, "getenv", side_effect=denied),
            patch.object(socket, "socket", side_effect=denied),
        ):
            contract = derive_lineage_contract(valid_manifest(), LINEAGE_PUBLIC_DER)
        self.assertEqual(contract.transition.source_epoch, 1)


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
