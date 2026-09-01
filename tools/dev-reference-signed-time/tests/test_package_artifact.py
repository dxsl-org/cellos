import hashlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from zipfile import ZipFile

import path_bootstrap  # noqa: F401

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from manifest import encode_manifest  # noqa: E402
from manifest_test_support import valid_manifest  # noqa: E402
from package_zip import package_tree  # noqa: E402
from verify_wheelhouse import verify_wheelhouse  # noqa: E402
from install_wheels import install_wheels  # noqa: E402


def make_wheel(
    directory,
    distribution,
    version,
    package,
    *,
    filename_tags="py3-none-any",
    metadata_name=None,
    metadata_version=None,
    declared_tags=None,
):
    filename = f"{distribution}-{version}-{filename_tags}.whl"
    path = directory / filename
    metadata = (
        "Metadata-Version: 2.1\n"
        f"Name: {distribution if metadata_name is None else metadata_name}\n"
        f"Version: {version if metadata_version is None else metadata_version}\n"
    )
    tags = filename_tags.split(".") if declared_tags is None else declared_tags
    wheel = "Wheel-Version: 1.0\nRoot-Is-Purelib: true\n" + "".join(
        f"Tag: {tag}\n" for tag in tags
    )
    dist_info = f"{distribution}-{version}.dist-info"
    with ZipFile(path, "w") as archive:
        archive.writestr(f"{package}/__init__.py", "")
        archive.writestr(f"{dist_info}/METADATA", metadata)
        archive.writestr(f"{dist_info}/WHEEL", wheel)
        archive.writestr(f"{dist_info}/RECORD", "")
    return path


def write_index(directory):
    lines = [
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}"
        for path in sorted(directory.glob("*.whl"))
    ]
    (directory / "SHA256SUMS").write_text(
        "\n".join(lines) + "\n", encoding="ascii"
    )


class PackageArtifactTests(unittest.TestCase):
    def test_normalized_zip_is_byte_deterministic(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / "stage"
            stage.mkdir()
            (stage / "manifest.json").write_bytes(b"{}")
            (stage / "module.py").write_text("value = 1\n", encoding="ascii")
            first, second = root / "first.zip", root / "second.zip"
            self.assertEqual(package_tree(stage, first), package_tree(stage, second))
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with ZipFile(first) as archive:
                self.assertEqual(archive.namelist(), ["manifest.json", "module.py"])
                self.assertEqual(
                    {entry.date_time for entry in archive.infolist()},
                    {(1980, 1, 1, 0, 0, 0)},
                )

    def test_wheelhouse_requires_complete_exact_digest_index(self):
        with tempfile.TemporaryDirectory() as temporary:
            wheelhouse = Path(temporary)
            wheel = make_wheel(wheelhouse, "example", "1.0", "example")
            write_index(wheelhouse)
            verify_wheelhouse(wheelhouse)
            wheel.write_bytes(wheel.read_bytes() + b"tampered")
            with self.assertRaises(SystemExit):
                verify_wheelhouse(wheelhouse)

    def test_rejects_unindexed_symlink_wheel(self):
        with tempfile.TemporaryDirectory() as temporary:
            wheelhouse = Path(temporary)
            indexed = make_wheel(
                wheelhouse,
                "example",
                "1.0",
                "example",
                filename_tags="cp311-cp311-manylinux_2_17_x86_64",
            )
            bypass = wheelhouse / "example-1.0-cp312-cp312-manylinux_2_17_x86_64.whl"
            bypass.symlink_to(indexed)
            digest = hashlib.sha256(indexed.read_bytes()).hexdigest()
            (wheelhouse / "SHA256SUMS").write_text(
                f"{digest}  {indexed.name}\n", encoding="ascii"
            )
            with self.assertRaises(SystemExit):
                verify_wheelhouse(wheelhouse)

    def test_rejects_substituted_metadata_and_internal_tags(self):
        cases = (
            {"metadata_name": "substituted"},
            {"metadata_version": "2.0"},
            {"declared_tags": ["cp311-cp311-manylinux_2_17_x86_64"]},
        )
        for changes in cases:
            with (
                self.subTest(changes=changes),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary)
                wheelhouse, target = root / "wheels", root / "target"
                wheelhouse.mkdir()
                target.mkdir()
                make_wheel(wheelhouse, "example", "1.0", "example", **changes)
                requirements = root / "requirements.txt"
                requirements.write_text("example==1.0\n", encoding="ascii")
                with self.assertRaises(SystemExit):
                    install_wheels(requirements, wheelhouse, target)

    def test_rejects_incompatible_python_abi_platform_tuples(self):
        for tags in ("py3-cp312-any", "cp37-abi3-any"):
            with self.subTest(tags=tags), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                wheelhouse, target = root / "wheels", root / "target"
                wheelhouse.mkdir()
                target.mkdir()
                make_wheel(
                    wheelhouse,
                    "example",
                    "1.0",
                    "example",
                    filename_tags=tags,
                )
                requirements = root / "requirements.txt"
                requirements.write_text("example==1.0\n", encoding="ascii")
                with self.assertRaises(SystemExit):
                    install_wheels(requirements, wheelhouse, target)

    def test_package_script_builds_only_from_manifest_and_local_wheels(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            wheelhouse = root / "wheels"
            wheelhouse.mkdir()
            make_wheel(wheelhouse, "cryptography", "41.0.7", "cryptography")
            make_wheel(wheelhouse, "cffi", "1.16.0", "cffi")
            make_wheel(wheelhouse, "pycparser", "2.21", "pycparser")
            write_index(wheelhouse)
            manifest = root / "manifest.json"
            manifest.write_bytes(encode_manifest(valid_manifest()))
            output = root / "artifact.zip"
            result = subprocess.run(
                [
                    str(ROOT / "scripts/package.sh"),
                    "--manifest", str(manifest),
                    "--wheelhouse", str(wheelhouse),
                    "--output", str(output),
                ],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertRegex(
                result.stdout, r"UnsignedZipSha256Base64=[A-Za-z0-9+/]{43}=\n"
            )
            self.assertRegex(result.stdout, r"UnsignedZipSha256Hex=[0-9a-f]{64}\n")
            self.assertNotIn("CodeSha256=", result.stdout)
            with ZipFile(output) as archive:
                names = set(archive.namelist())
            self.assertIn("lambda_entrypoint.py", names)
            self.assertIn("manifest.json", names)
            self.assertIn("cryptography/__init__.py", names)


if __name__ == "__main__":
    unittest.main()
