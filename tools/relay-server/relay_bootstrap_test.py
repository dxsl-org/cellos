from __future__ import annotations

import asyncio
import contextlib
import io
import ssl
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from relay_bootstrap import build_ssl_context, parse_args
from _relay_test_support import (
    CertificateSet,
    close_writer,
    connect,
    empty_denylist,
    make_certificates,
    start_relay,
)


class RelayBootstrapTests(unittest.IsolatedAsyncioTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._temp = tempfile.TemporaryDirectory()
        cls.certificates: CertificateSet = make_certificates(Path(cls._temp.name))

    @classmethod
    def tearDownClass(cls) -> None:
        cls._temp.cleanup()

    def test_server_context_requires_client_certificates_and_tls13(self) -> None:
        context = build_ssl_context(
            self.certificates.server.cert,
            self.certificates.server.key,
            self.certificates.ca_cert,
        )
        self.assertEqual(context.verify_mode, ssl.CERT_REQUIRED)
        self.assertEqual(context.minimum_version, ssl.TLSVersion.TLSv1_3)
        self.assertEqual(context.maximum_version, ssl.TLSVersion.TLSv1_3)

    def test_manifest_cli_path_is_mandatory(self) -> None:
        with mock.patch("sys.argv", ["relay_bootstrap.py"]):
            with contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as raised:
                    parse_args()
        self.assertEqual(raised.exception.code, 2)

    def test_manifest_is_the_only_bootstrap_input(self) -> None:
        manifest = Path("/run/cellos/relay.toml")
        with mock.patch(
            "sys.argv", ["relay_bootstrap.py", "--manifest", str(manifest)]
        ):
            args = parse_args()
        self.assertEqual(args.manifest, manifest)
        self.assertEqual(vars(args), {"manifest": manifest})

    async def test_untrusted_client_certificate_is_rejected_on_loopback_tls(self) -> None:
        server, _, port = await start_relay(self.certificates, empty_denylist())
        writer = None
        try:
            try:
                reader, writer = await connect(
                    self.certificates, self.certificates.untrusted_client, port
                )
            except (ConnectionResetError, ssl.SSLError):
                return
            try:
                result = await asyncio.wait_for(reader.read(1), 2)
            except (ConnectionResetError, ssl.SSLError):
                return
            self.assertEqual(result, b"")
        finally:
            if writer is not None:
                await close_writer(writer)
            server.close()
            await server.wait_closed()


if __name__ == "__main__":
    unittest.main()
