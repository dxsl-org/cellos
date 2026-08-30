import json
import unittest
from dataclasses import FrozenInstanceError, fields, replace
from unittest.mock import patch

import path_bootstrap
from manifest import ManifestError, SignedTimeManifest, decode_manifest, encode_manifest
from manifest_test_support import GOLDEN, KMS_UUID, kms_arn, valid_manifest
from protocol_models import MAX_UINT64
from manifest_validation import (
    MAX_AWS_REGION_CHARS,
    MAX_ENDPOINT_URL_CHARS,
    MAX_KMS_KEY_ID_CHARS,
    MAX_UPSTREAM_IDENTITY_CHARS,
)


class IntChild(int):
    pass


class StrChild(str):
    pass


class BytesChild(bytes):
    pass


class ManifestFieldTests(unittest.TestCase):
    def assert_encode_rejected(self, **changes):
        with self.assertRaises(ManifestError) as raised:
            encode_manifest(valid_manifest(**changes))
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_every_frozen_constant_rejects_mutation_and_inexact_type(self):
        cases = {
            "schema_version": (2, True, IntChild(1)),
            "classification": ("PRODUCTION", "", StrChild("DEV_REFERENCE")),
            "protocol_version": (2, True, IntChild(1)),
            "source_id": ("other", "", StrChild("cellos-dev-time-v1")),
            "signing_algorithm": ("RSA", "", StrChild("ECDSA_SHA_256")),
        }
        for field, values in cases.items():
            for value in values:
                with self.subTest(field=field, value=repr(value)):
                    self.assert_encode_rejected(**{field: value})

    def test_every_uint64_rejects_bad_range_and_inexact_type(self):
        names = (
            "source_epoch", "max_sample_age_seconds",
            "max_uncertainty_seconds",
        )
        bad = (-1, MAX_UINT64 + 1, True, IntChild(1), 1.0, "1", None)
        for name in names:
            for value in bad:
                with self.subTest(field=name, value=repr(value)):
                    self.assert_encode_rejected(**{name: value})

    def test_uint64_boundaries_are_preserved(self):
        for name in (
            "source_epoch", "max_sample_age_seconds",
            "max_uncertainty_seconds",
        ):
            for value in (0, MAX_UINT64):
                with self.subTest(field=name, value=value):
                    manifest = valid_manifest(**{name: value})
                    self.assertEqual(
                        getattr(decode_manifest(encode_manifest(manifest)), name),
                        value,
                    )

    def test_both_binary_digests_require_exact_bytes_of_length_32(self):
        for name in ("endpoint_spki_sha256", "kms_public_key_der_sha256"):
            bad = (b"", b"x" * 31, b"x" * 33, BytesChild(b"x" * 32),
                   bytearray(b"x" * 32), "11" * 32, None)
            for value in bad:
                with self.subTest(field=name, value=repr(value)):
                    self.assert_encode_rejected(**{name: value})

    def test_json_digests_require_lowercase_exact_hex(self):
        value = json.loads(GOLDEN)
        for name in ("endpoint_spki_sha256", "kms_public_key_der_sha256"):
            valid = value[name]
            for digest in (valid[:-1] + "A", valid[:-1], valid + "0", "g" * 64, 1):
                with self.subTest(field=name, digest=repr(digest)):
                    candidate = dict(value)
                    candidate[name] = digest
                    with self.assertRaises(ManifestError):
                        decode_manifest(self.canonical(candidate))

    def test_region_and_upstream_identity_require_nonempty_exact_strings(self):
        for name in ("aws_region", "upstream_identity"):
            original = getattr(valid_manifest(), name)
            for value in ("", StrChild(original), b"value", None, True):
                with self.subTest(field=name, value=repr(value)):
                    self.assert_encode_rejected(**{name: value})

    def test_pre_serialization_string_bounds_precede_parsing_and_json(self):
        cases = (
            ("aws_region", "r" * (MAX_AWS_REGION_CHARS + 1)),
            ("endpoint_url", "e" * (MAX_ENDPOINT_URL_CHARS + 1)),
            ("kms_key_id", "k" * (MAX_KMS_KEY_ID_CHARS + 1)),
            (
                "upstream_identity",
                "i" * (MAX_UPSTREAM_IDENTITY_CHARS + 1),
            ),
        )
        for name, value in cases:
            with self.subTest(name=name):
                with (
                    patch("manifest.json.dumps") as dumps,
                    patch("manifest_validation.urlsplit") as split,
                    patch("manifest.validation.kms_arn_is_valid") as arn,
                ):
                    self.assert_encode_rejected(**{name: value})
                dumps.assert_not_called()
                split.assert_not_called()
                arn.assert_not_called()

    def test_upstream_identity_exact_safe_bound_including_astral_unicode(self):
        for scalar in ("x", "\U0001f4a9"):
            with self.subTest(scalar=repr(scalar)):
                identity = scalar * MAX_UPSTREAM_IDENTITY_CHARS
                manifest = valid_manifest(upstream_identity=identity)
                self.assertEqual(
                    decode_manifest(encode_manifest(manifest)),
                    manifest,
                )
                self.assert_encode_rejected(
                    upstream_identity=(
                        scalar * (MAX_UPSTREAM_IDENTITY_CHARS + 1)
                    ),
                )

    def test_worst_case_valid_manifest_remains_below_byte_limit(self):
        region = "us-east-" + "1" + "0" * 23
        host = ".".join(("a" * 63, "b" * 63, "c" * 63, "d" * 61))
        manifest = valid_manifest(
            aws_region=region,
            endpoint_url=f"https://{host}/v1/time",
            kms_key_id=kms_arn(region=region, resource=f"key/{KMS_UUID}"),
            upstream_identity="\U0001f4a9" * MAX_UPSTREAM_IDENTITY_CHARS,
            source_epoch=MAX_UINT64,
            max_sample_age_seconds=MAX_UINT64,
            max_uncertainty_seconds=MAX_UINT64,
        )
        self.assertEqual(len(encode_manifest(manifest)), 4085)

    def test_manifest_is_frozen_slotted_and_exact_container_type_is_required(self):
        manifest = valid_manifest()
        with self.assertRaises(FrozenInstanceError):
            manifest.source_epoch = 8
        self.assertFalse(hasattr(manifest, "__dict__"))

        class ManifestChild(SignedTimeManifest):
            pass

        child = ManifestChild(*(getattr(manifest, f.name) for f in fields(manifest)))
        for value in (child, object(), None):
            with self.subTest(value_type=type(value).__name__):
                with self.assertRaises(ManifestError):
                    encode_manifest(value)

    @staticmethod
    def canonical(value):
        return json.dumps(
            value, sort_keys=True, separators=(",", ":"), ensure_ascii=True,
        ).encode("ascii")


if __name__ == "__main__":
    unittest.main()
