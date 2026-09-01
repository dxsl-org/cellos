import unittest

import path_bootstrap
from manifest import ManifestError, decode_manifest, encode_manifest
from manifest_test_support import valid_manifest


class StrChild(str):
    pass


class ManifestEndpointTests(unittest.TestCase):
    def assert_endpoint_rejected(self, value):
        with self.assertRaises(ManifestError) as raised:
            encode_manifest(valid_manifest(endpoint_url=value))
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_only_lowercase_https_scheme_is_accepted(self):
        for value in (
            "http://time.example.com/v1/time",
            "HTTPS://time.example.com/v1/time",
            "Https://time.example.com/v1/time",
            "time.example.com/v1/time",
            "//time.example.com/v1/time",
        ):
            with self.subTest(value=value):
                self.assert_endpoint_rejected(value)

    def test_host_is_nonempty_ascii_lowercase_and_canonical(self):
        for value in (
            "https:///v1/time",
            "https://[invalid/v1/time",
            "https://TIME.example.com/v1/time",
            "https://tíme.example.com/v1/time",
            "https://%74ime.example.com/v1/time",
            "https://time_example.com/v1/time",
            "https://-time.example.com/v1/time",
            "https://time.example.com./v1/time",
            "https://time..example.com/v1/time",
        ):
            with self.subTest(value=value):
                self.assert_endpoint_rejected(value)

    def test_exact_path_is_required(self):
        for path in (
            "", "/", "/v1", "/v1/time/", "/V1/time", "/v1/Time",
            "/v1/%74ime", "//v1/time", "/v1/time;parameter",
        ):
            with self.subTest(path=path):
                self.assert_endpoint_rejected("https://time.example.com" + path)

    def test_ports_and_userinfo_are_rejected(self):
        for authority in (
            "time.example.com:443",
            "time.example.com:8443",
            "user@time.example.com",
            "user:password@time.example.com",
            ":password@time.example.com",
        ):
            with self.subTest(authority=authority):
                self.assert_endpoint_rejected(f"https://{authority}/v1/time")

    def test_query_fragment_and_noncanonical_suffixes_are_rejected(self):
        for suffix in ("?x=1", "?", "#fragment", "#", "?x=1#fragment"):
            with self.subTest(suffix=suffix):
                self.assert_endpoint_rejected(
                    "https://time.example.com/v1/time" + suffix,
                )

    def test_total_hostname_length_accepts_253_and_rejects_254(self):
        host_253 = ".".join(("a" * 63, "b" * 63, "c" * 63, "d" * 61))
        host_254 = ".".join(("a" * 63, "b" * 63, "c" * 63, "d" * 62))
        endpoint = f"https://{host_253}/v1/time"
        manifest = valid_manifest(endpoint_url=endpoint)
        self.assertEqual(decode_manifest(encode_manifest(manifest)), manifest)
        self.assert_endpoint_rejected(f"https://{host_254}/v1/time")

    def test_endpoint_requires_exact_string_type(self):
        endpoint = "https://time.example.com/v1/time"
        for value in (StrChild(endpoint), endpoint.encode("ascii"), None, True):
            with self.subTest(value=repr(value)):
                self.assert_endpoint_rejected(value)

    def test_canonical_endpoint_round_trips_unchanged(self):
        manifest = valid_manifest()
        decoded = decode_manifest(encode_manifest(manifest))
        self.assertEqual(decoded.endpoint_url, manifest.endpoint_url)


if __name__ == "__main__":
    unittest.main()
