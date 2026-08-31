import json
import unittest

import path_bootstrap
from manifest import MAX_MANIFEST_BYTES, ManifestError, decode_manifest, encode_manifest
from manifest_test_support import GOLDEN, valid_manifest

class BytesChild(bytes):
    pass



class ManifestCanonicalTests(unittest.TestCase):
    def assert_rejected(self, data):
        with self.assertRaises(ManifestError) as raised:
            decode_manifest(data)
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_exact_golden_bytes_and_round_trip(self):
        manifest = valid_manifest()
        self.assertEqual(encode_manifest(manifest), GOLDEN)
        self.assertEqual(decode_manifest(GOLDEN), manifest)
        self.assertFalse(GOLDEN.endswith(b"\n"))

    def test_every_missing_field_and_every_extra_field_are_rejected(self):
        value = json.loads(GOLDEN)
        for field in tuple(value):
            with self.subTest(missing=field):
                candidate = dict(value)
                del candidate[field]
                self.assert_rejected(self.canonical(candidate))
        value["unexpected"] = 1
        self.assert_rejected(self.canonical(value))

    def test_every_duplicate_field_is_rejected(self):
        for field, value in json.loads(GOLDEN).items():
            pair = json.dumps(field) + ":" + json.dumps(value) + ","
            with self.subTest(field=field):
                self.assert_rejected(b"{" + pair.encode("ascii") + GOLDEN[1:])

    def test_noncanonical_order_spacing_escape_and_newline_are_rejected(self):
        value = json.loads(GOLDEN)
        variants = (
            json.dumps(
                dict(reversed(tuple(value.items()))),
                separators=(",", ":"),
            ).encode("ascii"),
            json.dumps(value, sort_keys=True).encode("ascii"),
            GOLDEN.replace(b'"DEV_REFERENCE"', b'"\\u0044EV_REFERENCE"'),
            GOLDEN.replace(b"/v1/time", b"\\/v1\\/time"),
            GOLDEN + b"\n",
        )
        for candidate in variants:
            with self.subTest(candidate=candidate[:40]):
                self.assert_rejected(candidate)

    def test_trailing_bytes_bom_and_non_utf8_are_rejected(self):
        variants = (
            GOLDEN + b"x",
            GOLDEN.replace(
                b"roughtime.cloudflare.com",
                "roughtime.cloudflaré.com".encode("utf-8"),
            ),
            b"\xef\xbb\xbf" + GOLDEN,
            GOLDEN[:-1] + b"\xff",
            GOLDEN + b"{}",
        )
        for candidate in variants:
            with self.subTest(candidate=candidate[-8:]):
                self.assert_rejected(candidate)

    def test_nan_and_infinity_tokens_are_rejected(self):
        marker = b'"source_epoch":7'
        for token in (b"NaN", b"Infinity", b"-Infinity"):
            with self.subTest(token=token):
                self.assert_rejected(
                    GOLDEN.replace(marker, b'"source_epoch":' + token),
                )

    def test_non_object_json_and_non_bytes_inputs_are_rejected(self):
        candidates = (
            b"null", b"[]", b'"text"', b"1", "{}", bytearray(b"{}"),
            BytesChild(GOLDEN),
        )
        for candidate in candidates:
            with self.subTest(value=repr(candidate)):
                self.assert_rejected(candidate)
    def test_oversized_input_is_rejected(self):
        self.assert_rejected(b" " * (MAX_MANIFEST_BYTES + 1))

    @staticmethod
    def canonical(value):
        return json.dumps(
            value, sort_keys=True, separators=(",", ":"), ensure_ascii=True,
        ).encode("ascii")


if __name__ == "__main__":
    unittest.main()
