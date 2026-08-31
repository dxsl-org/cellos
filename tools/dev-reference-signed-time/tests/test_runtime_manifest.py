import tempfile
import unittest
from pathlib import Path

import path_bootstrap  # noqa: F401

from manifest import encode_manifest
from manifest_test_support import valid_manifest
from runtime_manifest import RuntimeManifestError, load_runtime_manifest


def environment(manifest):
    return {
        "AWS_REGION": manifest.aws_region,
        "SIGNED_TIME_TABLE_NAME": manifest.allocator_table_name,
        "SIGNED_TIME_LINEAGE_TABLE_NAME": manifest.lineage_table_name,
        "SIGNED_TIME_KMS_KEY_ARN": manifest.kms_key_id,
        "SIGNED_TIME_LINEAGE_KMS_KEY_ARN": manifest.lineage_kms_key_id,
    }


class RuntimeManifestTests(unittest.TestCase):
    def setUp(self):
        self.manifest = valid_manifest()
        self.directory = tempfile.TemporaryDirectory()
        self.path = Path(self.directory.name, "manifest.json")
        self.path.write_bytes(encode_manifest(self.manifest))

    def tearDown(self):
        self.directory.cleanup()

    def test_loads_only_canonical_manifest_with_exact_environment(self):
        self.assertEqual(
            load_runtime_manifest(self.path, environment(self.manifest)),
            self.manifest,
        )

    def test_rejects_every_missing_or_substituted_binding(self):
        expected = environment(self.manifest)
        for name in tuple(expected):
            for change in (None, expected[name] + "-substituted"):
                with self.subTest(name=name, change=change):
                    candidate = dict(expected)
                    if change is None:
                        candidate.pop(name)
                    else:
                        candidate[name] = change
                    with self.assertRaisesRegex(
                        RuntimeManifestError, "^runtime manifest admission failed$"
                    ):
                        load_runtime_manifest(self.path, candidate)

    def test_rejects_missing_noncanonical_and_oversized_files(self):
        candidates = (
            Path(self.directory.name, "missing.json"),
            self.path,
            self.path,
        )
        contents = (None, encode_manifest(self.manifest) + b"\n", b"x" * 4097)
        for index, (path, content) in enumerate(zip(candidates, contents)):
            with self.subTest(index=index):
                if content is not None:
                    path.write_bytes(content)
                with self.assertRaisesRegex(
                    RuntimeManifestError, "^runtime manifest admission failed$"
                ):
                    load_runtime_manifest(path, environment(self.manifest))


if __name__ == "__main__":
    unittest.main()
