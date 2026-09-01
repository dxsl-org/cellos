import unittest

import path_bootstrap
from manifest import ManifestError, decode_manifest, encode_manifest
from manifest_test_support import KMS_ARN, KMS_MRK, KMS_UUID, kms_arn, valid_manifest


class StrChild(str):
    pass


class ManifestKmsArnTests(unittest.TestCase):
    def assert_arn_rejected(self, arn, *, region="us-east-1"):
        with self.assertRaises(ManifestError) as raised:
            encode_manifest(valid_manifest(kms_key_id=arn, aws_region=region))
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_only_documented_partitions_are_accepted(self):
        for partition in ("", "AWS", "aws-commercial", "aws-iso", "aws-us-govx"):
            with self.subTest(partition=partition):
                self.assert_arn_rejected(kms_arn(partition=partition))

    def test_arn_prefix_and_kms_service_are_exact(self):
        candidates = (
            KMS_ARN.replace("arn:", "xrn:", 1),
            KMS_ARN.replace(":kms:", "::", 1),
            KMS_ARN.replace(":kms:", ":KMS:", 1),
            KMS_ARN.replace(":kms:", ":s3:", 1),
            KMS_ARN.replace(":kms:", ":kms-fips:", 1),
        )
        for arn in candidates:
            with self.subTest(arn=arn):
                self.assert_arn_rejected(arn)

    def test_partition_and_region_families_must_match(self):
        cases = (
            ("aws", "us-gov-west-1"),
            ("aws", "cn-north-1"),
            ("aws-us-gov", "us-east-1"),
            ("aws-us-gov", "cn-northwest-1"),
            ("aws-cn", "eu-west-1"),
            ("aws-cn", "us-gov-east-1"),
        )
        for partition, region in cases:
            with self.subTest(partition=partition, region=region):
                self.assert_arn_rejected(
                    kms_arn(partition=partition, region=region),
                    region=region,
                )

    def test_malformed_commercial_region_syntax_is_rejected(self):
        for region in (
            "", "US-EAST-1", "us_east_1", "us-east", "us-east-one",
            "us-east-0", "us-east-01", "us--east-1", "moon-east-1",
            "région-1",
        ):
            with self.subTest(region=region):
                self.assert_arn_rejected(kms_arn(region=region), region=region)

    def test_region_must_exactly_match_manifest(self):
        self.assert_arn_rejected(kms_arn(region="us-west-2"))

    def test_account_requires_exactly_twelve_ascii_digits(self):
        for account in (
            "", "0" * 11, "0" * 13, "00000000000a", "0000 0000 000",
            "１２３４５６７８９０１２",
        ):
            with self.subTest(account=account):
                self.assert_arn_rejected(kms_arn(account=account))

    def test_resource_requires_lowercase_uuid_or_multi_region_key(self):
        uppercase_uuid = KMS_UUID[:-1] + "A"
        bad_uuid = KMS_UUID.replace("-", "", 1)
        bad_hex_uuid = KMS_UUID[:-1] + "g"
        resources = (
            "", "key", "key/", f"alias/{KMS_UUID}", "alias/documentation",
            "key/documentation", f"Key/{KMS_UUID}", f"key/{uppercase_uuid}",
            f"key/{bad_uuid}", f"key/{bad_hex_uuid}", f"key/MRK-{'0' * 32}",
            f"key/mrk-{'A' * 32}", f"key/mrk-{'0' * 31}",
        )
        for resource in resources:
            with self.subTest(resource=resource):
                self.assert_arn_rejected(kms_arn(resource=resource))

    def test_valid_commercial_govcloud_china_and_mrk_arns(self):
        cases = (
            ("aws", "us-east-1", f"key/{KMS_UUID}"),
            ("aws", "ap-southeast-7", f"key/{KMS_MRK}"),
            ("aws-us-gov", "us-gov-west-1", f"key/{KMS_UUID}"),
            ("aws-us-gov", "us-gov-east-1", f"key/{KMS_MRK}"),
            ("aws-cn", "cn-northwest-1", f"key/{KMS_MRK}"),
            ("aws-cn", "cn-north-1", f"key/{KMS_UUID}"),
        )
        for partition, region, resource in cases:
            arn = kms_arn(
                partition=partition,
                region=region,
                resource=resource,
            )
            with self.subTest(partition=partition, region=region):
                manifest = valid_manifest(aws_region=region, kms_key_id=arn)
                self.assertEqual(
                    decode_manifest(encode_manifest(manifest)),
                    manifest,
                )

    def test_malformed_component_counts_are_rejected(self):
        for arn in ("arn", "arn:aws:kms", KMS_ARN.replace(":key/", "key/", 1)):
            with self.subTest(arn=arn):
                self.assert_arn_rejected(arn)

    def test_key_arn_requires_exact_string_type(self):
        for value in (StrChild(KMS_ARN), KMS_ARN.encode("ascii"), None, True):
            with self.subTest(value=repr(value)):
                self.assert_arn_rejected(value)

    def test_documentation_key_arn_round_trips_unchanged(self):
        manifest = valid_manifest()
        decoded = decode_manifest(encode_manifest(manifest))
        self.assertEqual(decoded.kms_key_id, KMS_ARN)


if __name__ == "__main__":
    unittest.main()
