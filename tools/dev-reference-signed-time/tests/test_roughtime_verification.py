import unittest
from base64 import b64decode
from dataclasses import FrozenInstanceError, replace
from unittest.mock import patch

import path_bootstrap
from roughtime_config import RoughtimeConfigError, provider_config
from roughtime_verify import (
    RoughtimeVerificationError, VerifiedRoughtime, build_request, verify_response,
)
from roughtime_vector_support import (
    CONFIG, NONCE, SOURCE_NONCE, SOURCE_REPLY, SOURCE_REQUEST, SOURCE_VECTOR,
    ResponseOptions, exact_request, response_packet, vector,
)


class RoughtimeVerificationTests(unittest.TestCase):
    def verify(self, options=ResponseOptions()):
        config, request, response = vector(options)
        with patch("roughtime_verify.validate_provider_config"):
            return verify_response(response, request, NONCE, config)

    def assert_rejected(self, options=ResponseOptions(), *, response=None, request=None):
        request = exact_request() if request is None else request
        response = response_packet(options) if response is None else response
        with patch("roughtime_verify.validate_provider_config"):
            with self.assertRaises(RoughtimeVerificationError) as caught:
                verify_response(response, request, NONCE, CONFIG)
        self.assertEqual(str(caught.exception), "invalid roughtime response")
        self.assertIsNone(caught.exception.__cause__)
        self.assertIsNone(caught.exception.__context__)

    def test_generated_keys_verify_complete_positive_path(self):
        result = self.verify()
        self.assertEqual(result, VerifiedRoughtime(1_700_000_000, 3))
        with self.assertRaises(FrozenInstanceError):
            result.radius = 4
        self.assertFalse(hasattr(result, "__dict__"))

    def test_pinned_source_capture_verifies_without_configuration_bypass(self):
        result = verify_response(SOURCE_REPLY, SOURCE_REQUEST, SOURCE_NONCE)
        self.assertEqual(
            result, VerifiedRoughtime(SOURCE_VECTOR["midpoint"], SOURCE_VECTOR["radius"]),
        )
        changed_nonce = bytes((SOURCE_NONCE[0] ^ 1,)) + SOURCE_NONCE[1:]
        with self.assertRaises(RoughtimeVerificationError):
            verify_response(SOURCE_REPLY, build_request(changed_nonce), changed_nonce)

    def test_four_leaf_proof_verifies_both_branch_directions(self):
        # Fixed complete tree over NONCE, b"a"*32, b"b"*32, b"c"*32.
        # The expected root is independent of the response fixture's path helper.
        root = bytes.fromhex(
            "caade35580e5c06cd4fc2134a6ca77c52a2a9880c53fab0c10d504693e34c63b"
        )
        cases = (
            (NONCE, 0,
             "046803cf80ef7feb18c4b5aa5168d6895c24ab3aa2f1b4071aa1662aa965df1fc"
             "930e98afcc6ae47aa69cb577edcd9b8f6cca5eeb5249d494de3bfa4835d5c15"),
            (b"b" * 32, 2,
             "bedfbddb926658cb21ab09e4b9dbc6226bba59602d1da9e5c4a9635b597803d29"
             "c112ac8272a6842561cf018c1de1098267707bd69e8e06f4ec64417a8ad1993"),
        )
        for nonce, index, path_hex in cases:
            with self.subTest(index=index), patch("roughtime_verify.validate_provider_config"):
                request = exact_request(nonce)
                path = bytes.fromhex(path_hex)
                options = ResponseOptions(nonce=nonce, index=index, path=path, root=root)
                result = verify_response(response_packet(options), request, nonce, CONFIG)
                self.assertEqual(result, VerifiedRoughtime(1_700_000_000, 3))
                swapped = replace(options, path=path[32:] + path[:32])
                with self.assertRaises(RoughtimeVerificationError):
                    verify_response(response_packet(swapped), request, nonce, CONFIG)

    def test_nonce_version_and_exact_request_binding_fail_independently(self):
        self.assert_rejected(ResponseOptions(nonce=b"N" * 32))
        self.assert_rejected(ResponseOptions(root_version=0x8000000C))
        request = bytearray(exact_request())
        request[-1] = 1
        self.assert_rejected(request=bytes(request), response=response_packet())

    def test_radius_and_delegation_window_are_strict(self):
        cases = (
            ResponseOptions(radius=0),
            ResponseOptions(radius=1),
            ResponseOptions(radius=2),
            ResponseOptions(minimum=1_700_000_001),
            ResponseOptions(maximum=1_699_999_999),
            ResponseOptions(minimum=1_700_000_001, maximum=1_699_999_999),
        )
        for options in cases:
            with self.subTest(options=options):
                self.assert_rejected(options)

    def test_each_signature_fails_independently(self):
        self.assert_rejected(ResponseOptions(bad_delegation_signature=True))
        self.assert_rejected(ResponseOptions(bad_response_signature=True))

    def test_merkle_root_index_and_path_fail_independently(self):
        self.assert_rejected(ResponseOptions(root=b"R" * 32))
        self.assert_rejected(ResponseOptions(index=1))
        self.assert_rejected(ResponseOptions(path=b"P" * 32, root=b"R" * 32))
        self.assert_rejected(ResponseOptions(path=b"P" * 32, index=2))

    def test_bounded_extra_response_tags_are_ignored_after_authentication(self):
        cases = (
            ResponseOptions(root_extra=(("SRV", b"x" * 32),)),
            ResponseOptions(cert_extra=(("VER", b"x" * 4),)),
            ResponseOptions(dele_extra=(("VER", b"x" * 4),)),
            ResponseOptions(srep_extra=(("NONC", b"x" * 32),)),
        )
        for options in cases:
            with self.subTest(options=options):
                self.assertEqual(
                    self.verify(options), VerifiedRoughtime(1_700_000_000, 3),
                )

    def test_every_nested_level_rejects_each_missing_required_tag(self):
        cases = (
            *(ResponseOptions(omit_root=tag) for tag in (
                "SIG", "VER", "NONC", "PATH", "SREP", "CERT", "INDX",
            )),
            ResponseOptions(omit_cert="DELE"),
            ResponseOptions(omit_cert="SIG"),
            *(ResponseOptions(omit_dele=tag) for tag in ("MINT", "MAXT", "PUBK")),
            *(ResponseOptions(omit_srep=tag) for tag in ("ROOT", "MIDP", "RADI")),
        )
        for options in cases:
            with self.subTest(options=options):
                self.assert_rejected(options)

    def test_malformed_and_packet_bounds_fail_before_releasing_time(self):
        request = exact_request()
        for response in (
            b"",
            b"ROUGHTIM\0\0\0\0",
            response_packet() + b"x" * 600,
            response_packet()[:-1],
        ):
            with self.subTest(length=len(response)):
                self.assert_rejected(response=response, request=request)

    def test_runtime_rejects_every_alternate_key_or_provider(self):
        alternate = replace(provider_config(), public_key=b"x" * 32)
        with self.assertRaises(RoughtimeConfigError):
            build_request(NONCE, alternate)
        with self.assertRaises(RoughtimeConfigError):
            verify_response(b"", b"", NONCE, alternate)
        for change in (
            {"host": "example.com"}, {"transport": "tcp"},
            {"host": "retired.invalid", "port": 2003,
             "public_key": b"r" * 32},
            {"host": "roughtime.se", "port": 2002,
             "public_key": b64decode("S3AzfZJ5CjSdkJ21ZJGbxqdYP/SoE8fXKY0+aicsehI=")},
            {"protocol": "roughtime"}, {"port": 2004}, {"version": 1},
            {"timeout_milliseconds": 1}, {"request_message_bytes": 512},
            {"max_packet_bytes": 2048},
        ):
            with self.subTest(change=change):
                with self.assertRaises(RoughtimeConfigError):
                    build_request(NONCE, replace(provider_config(), **change))


if __name__ == "__main__":
    unittest.main()
