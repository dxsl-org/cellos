from __future__ import annotations

import json
import tempfile
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

from _relay_test_support import CertificateSet, make_certificates
from relay_manifest import ManifestError, load_server_manifest


class RelayManifestTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls._temp.name)
        cls.certificates: CertificateSet = make_certificates(cls.root)
        cls.denylist = cls.root / "denylist.json"
        cls.denylist.write_text(
            '{"revoked_node_ids": [], "revoked_serials": []}', encoding="utf-8"
        )

    @classmethod
    def tearDownClass(cls) -> None:
        cls._temp.cleanup()

    def _text(self) -> str:
        quote = json.dumps
        certificates = self.certificates
        return f"""[relay]
bind_host = "0.0.0.0"
hostname = "localhost"
port = 443
min_tls_version = "1.3"

[trust]
relay_ca_der = "/mounted/relay-ca.der"
active_ca_sha256 = "{'0' * 64}"
next_ca_sha256 = ""

[client]
certificate_chain_der = "/mounted/client-chain.der"
key_handle = "silo:test"
node_id_sha256 = "{'1' * 64}"

[server]
certificate_pem = {quote(str(certificates.server.cert))}
private_key_pem = {quote(str(certificates.server.key))}
client_issuing_ca_pem = {quote(str(certificates.ca_cert))}

[authorization]
net_service_identity = "service-net"
policy_handle = "policy:test"
relay_denylist = {quote(str(self.denylist))}
"""

    def _write(self, text: str) -> Path:
        manifest = self.root / "relay-manifest.toml"
        manifest.write_text(text, encoding="utf-8")
        return manifest

    def test_valid_mounted_manifest_returns_immutable_server_config(self) -> None:
        config = load_server_manifest(self._write(self._text()))
        self.assertEqual(config.bind_host, "0.0.0.0")
        self.assertEqual(config.hostname, "localhost")
        self.assertEqual(config.port, 443)
        self.assertEqual(config.min_tls_version, "1.3")
        self.assertEqual(config.certificate_pem, self.certificates.server.cert)
        self.assertEqual(config.private_key_pem, self.certificates.server.key)
        self.assertEqual(config.client_issuing_ca_pem, self.certificates.ca_cert)
        self.assertEqual(config.relay_denylist, self.denylist)
        with self.assertRaises(FrozenInstanceError):
            config.port = 8443  # type: ignore[misc]

    def test_missing_server_fields_are_rejected(self) -> None:
        required_lines = (
            'bind_host = "0.0.0.0"\n',
            'hostname = "localhost"\n',
            "port = 443\n",
            'min_tls_version = "1.3"\n',
            f"certificate_pem = {json.dumps(str(self.certificates.server.cert))}\n",
            f"private_key_pem = {json.dumps(str(self.certificates.server.key))}\n",
            f"client_issuing_ca_pem = {json.dumps(str(self.certificates.ca_cert))}\n",
            f"relay_denylist = {json.dumps(str(self.denylist))}\n",
        )
        for line in required_lines:
            with self.subTest(field=line.partition(" =")[0]):
                manifest = self._write(self._text().replace(line, ""))
                with self.assertRaises(ManifestError):
                    load_server_manifest(manifest)

    def test_wrong_field_types_are_rejected(self) -> None:
        replacements = (
            ('bind_host = "0.0.0.0"', "bind_host = 1"),
            ('hostname = "localhost"', "hostname = false"),
            ("port = 443", 'port = "443"'),
            ('min_tls_version = "1.3"', "min_tls_version = 1.3"),
            (
                f"certificate_pem = {json.dumps(str(self.certificates.server.cert))}",
                "certificate_pem = 7",
            ),
            (
                f"relay_denylist = {json.dumps(str(self.denylist))}",
                "relay_denylist = []",
            ),
        )
        for original, replacement in replacements:
            with self.subTest(field=original.partition(" =")[0]):
                manifest = self._write(self._text().replace(original, replacement))
                with self.assertRaises(ManifestError):
                    load_server_manifest(manifest)

    def test_relative_missing_and_nonregular_paths_are_rejected(self) -> None:
        paths = (
            self.certificates.server.cert,
            self.certificates.server.key,
            self.certificates.ca_cert,
            self.denylist,
        )
        for path in paths:
            original = json.dumps(str(path))
            replacements = (
                '"relative.pem"',
                json.dumps(str(self.root / "missing")),
                json.dumps(str(self.root)),
            )
            for replacement in replacements:
                with self.subTest(path=path.name, replacement=replacement):
                    manifest = self._write(self._text().replace(original, replacement))
                    with self.assertRaises(ManifestError):
                        load_server_manifest(manifest)

    def test_tls_version_other_than_exactly_13_is_rejected(self) -> None:
        manifest = self._write(
            self._text().replace('min_tls_version = "1.3"', 'min_tls_version = "1.2"')
        )
        with self.assertRaises(ManifestError):
            load_server_manifest(manifest)

    def test_certificate_common_name_cannot_replace_matching_dns_san(self) -> None:
        manifest = self._write(
            self._text().replace('hostname = "localhost"', 'hostname = "server"')
        )
        with self.assertRaises(ManifestError):
            load_server_manifest(manifest)

    def test_malformed_extra_and_invalid_network_fields_are_rejected(self) -> None:
        invalid_documents = (
            self._text() + "unexpected = [\n",
            self._text().replace("port = 443", "port = 0"),
            self._text().replace('hostname = "localhost"', 'hostname = "127.0.0.1"'),
            self._text().replace('hostname = "localhost"', 'hostname = "bad_name"'),
            self._text().replace("port = 443", "port = 443\nunexpected = true"),
        )
        for document in invalid_documents:
            with self.subTest(document=document[-30:]):
                with self.assertRaises(ManifestError):
                    load_server_manifest(self._write(document))


if __name__ == "__main__":
    unittest.main()
