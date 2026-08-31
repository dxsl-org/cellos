import json
import unittest
from dataclasses import FrozenInstanceError, fields
from unittest.mock import patch

import path_bootstrap
from manifest import ManifestError, decode_manifest, encode_manifest
from manifest_model import MAX_MANIFEST_BYTES, SignedTimeManifest
from manifest_test_support import GOLDEN, valid_manifest
from protocol_models import MAX_UINT64
from manifest_validation import (
    MAX_AWS_REGION_CHARS,
    MAX_ENDPOINT_URL_CHARS,
    MAX_KMS_KEY_ID_CHARS,
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
            "schema_version": (1, True, IntChild(2)),
            "classification": ("PRODUCTION", "", StrChild("DEV_REFERENCE")),
            "protocol_version": (2, True, IntChild(1)),
            "source_id": ("other", "", StrChild("cellos-dev-time-v1")),
            "signing_algorithm": ("RSA", "", StrChild("ECDSA_SHA_256")),
            "upstream_identity": ("other", "", StrChild("roughtime.cloudflare.com")),
            "upstream_protocol": ("roughtime", "", StrChild("roughtime-draft-11")),
            "upstream_transport": ("tcp", "", StrChild("udp")),
            "upstream_host": ("example.com", "", StrChild("roughtime.cloudflare.com")),
            "upstream_port": (2004, True, IntChild(2003)),
            "upstream_version": (1, True, IntChild(0x8000000B)),
            "upstream_timeout_milliseconds": (1, True, IntChild(2000)),
            "upstream_request_message_bytes": (512, True, IntChild(1012)),
            "upstream_max_packet_bytes": (2048, True, IntChild(1024)),
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
        for name in ("max_sample_age_seconds", "max_uncertainty_seconds"):
            for value in (0, MAX_UINT64):
                with self.subTest(field=name, value=value):
                    manifest = valid_manifest(**{name: value})
                    self.assertEqual(
                        getattr(decode_manifest(encode_manifest(manifest)), name),
                        value,
                    )
        for value in (1, MAX_UINT64):
            manifest = valid_manifest(source_epoch=value)
            self.assertEqual(
                decode_manifest(encode_manifest(manifest)).source_epoch, value,
            )
        self.assert_encode_rejected(source_epoch=0)

    def test_every_binary_pin_requires_exact_bytes_of_length_32(self):
        for name in (
            "endpoint_spki_sha256", "kms_public_key_der_sha256",
            "lineage_public_key_der_sha256", "upstream_public_key",
        ):
            bad = (b"", b"x" * 31, b"x" * 33, BytesChild(b"x" * 32),
                   bytearray(b"x" * 32), "11" * 32, None)
            if name == "upstream_public_key":
                bad += (b"x" * 32,)
            for value in bad:
                with self.subTest(field=name, value=repr(value)):
                    self.assert_encode_rejected(**{name: value})

    def test_json_digests_require_lowercase_exact_hex(self):
        value = json.loads(GOLDEN)
        for name in (
            "endpoint_spki_sha256", "kms_public_key_der_sha256",
            "lineage_public_key_der_sha256",
        ):
            valid = value[name]
            for digest in (valid[:-1] + "A", valid[:-1], valid + "0", "g" * 64, 1):
                with self.subTest(field=name, digest=repr(digest)):
                    candidate = dict(value)
                    candidate[name] = digest
                    with self.assertRaises(ManifestError):
                        decode_manifest(self.canonical(candidate))

    def test_json_provider_key_requires_exact_canonical_base64(self):
        value = json.loads(GOLDEN)
        valid = value["upstream_public_key"]
        for key in (valid[:-1], valid + "=", valid[:-2] + "==", "!" * 44, 1):
            with self.subTest(key=repr(key)):
                candidate = dict(value)
                candidate["upstream_public_key"] = key
                with self.assertRaises(ManifestError):
                    decode_manifest(self.canonical(candidate))

    def test_region_requires_nonempty_exact_string(self):
        original = valid_manifest().aws_region
        for value in ("", StrChild(original), b"value", None, True):
            with self.subTest(value=repr(value)):
                self.assert_encode_rejected(aws_region=value)

    def test_pre_serialization_string_bounds_precede_parsing_and_json(self):
        cases = (
            ("aws_region", "r" * (MAX_AWS_REGION_CHARS + 1)),
            ("endpoint_url", "e" * (MAX_ENDPOINT_URL_CHARS + 1)),
            ("kms_key_id", "k" * (MAX_KMS_KEY_ID_CHARS + 1)),
            ("lineage_kms_key_id", "k" * (MAX_KMS_KEY_ID_CHARS + 1)),
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

    def test_valid_manifest_remains_below_byte_limit(self):
        self.assertLess(len(encode_manifest(valid_manifest())), MAX_MANIFEST_BYTES)
        self.assertEqual(
            decode_manifest(encode_manifest(valid_manifest())),
            valid_manifest(),
        )

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
