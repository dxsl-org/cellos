import unittest

from kms_public_key_support import (
    BytesChild,
    DictChild,
    HostileMapping,
    IntChild,
    ListChild,
    PublicKeyLoaderTestCase,
    StrChild,
)


class KmsPublicKeyResponseTests(PublicKeyLoaderTestCase):
    def test_provider_exception_and_non_exact_response_dict_fail_once(self):
        self.assert_load_fails(error=RuntimeError("secret provider detail"))
        for result in (None, [], {}, DictChild(self.response()), HostileMapping()):
            with self.subTest(response_type=type(result).__name__):
                self.assert_load_fails(result=result)

    def test_response_metadata_requires_exact_success_fields(self):
        result = self.response()
        result.pop("ResponseMetadata")
        cases = [result]
        for metadata in (
            None,
            {},
            DictChild({"HTTPStatusCode": 200, "RequestId": "request"}),
            {"HTTPStatusCode": 201, "RequestId": "request"},
            {"HTTPStatusCode": True, "RequestId": "request"},
            {"HTTPStatusCode": IntChild(200), "RequestId": "request"},
            {"HTTPStatusCode": 200},
            {"HTTPStatusCode": 200, "RequestId": ""},
            {"HTTPStatusCode": 200, "RequestId": b"request"},
            {"HTTPStatusCode": 200, "RequestId": StrChild("request")},
        ):
            candidate = self.response()
            candidate["ResponseMetadata"] = metadata
            cases.append(candidate)
        for candidate in cases:
            with self.subTest(metadata_type=type(candidate.get("ResponseMetadata")).__name__):
                self.assert_load_fails(result=candidate)

    def test_key_id_and_public_key_require_present_nonempty_exact_types(self):
        for field, values in (
            ("KeyId", ("", "other-key", b"key", StrChild(self.key_id))),
            ("PublicKey", (b"", bytearray(self.public_key_der),
                           BytesChild(self.public_key_der))),
        ):
            missing = self.response()
            missing.pop(field)
            cases = [missing]
            for value in values:
                candidate = self.response()
                candidate[field] = value
                cases.append(candidate)
            for candidate in cases:
                with self.subTest(field=field, value_type=type(candidate.get(field)).__name__):
                    self.assert_load_fails(result=candidate)

    def test_key_spec_requires_at_least_one_exact_consistent_p256_value(self):
        no_spec = self.response()
        no_spec.pop("KeySpec")
        cases = [no_spec]
        for field in ("KeySpec", "CustomerMasterKeySpec"):
            for value in ("", "ECC_NIST_P384", b"ECC_NIST_P256",
                          StrChild("ECC_NIST_P256")):
                candidate = self.response("both")
                candidate[field] = value
                cases.append(candidate)
        for candidate in cases:
            with self.subTest(fields=sorted(candidate)):
                self.assert_load_fails(result=candidate)

    def test_usage_requires_exact_sign_verify(self):
        missing = self.response()
        missing.pop("KeyUsage")
        cases = [missing]
        for value in ("", "ENCRYPT_DECRYPT", b"SIGN_VERIFY", StrChild("SIGN_VERIFY")):
            candidate = self.response()
            candidate["KeyUsage"] = value
            cases.append(candidate)
        for candidate in cases:
            with self.subTest(value_type=type(candidate.get("KeyUsage")).__name__):
                self.assert_load_fails(result=candidate)

    def test_algorithms_require_exact_list_and_strings_containing_sha256(self):
        missing = self.response()
        missing.pop("SigningAlgorithms")
        cases = [missing]
        for value in (
            None,
            [],
            ("ECDSA_SHA_256",),
            ListChild(["ECDSA_SHA_256"]),
            [StrChild("ECDSA_SHA_256")],
            ["ECDSA_SHA_384"],
            ["ECDSA_SHA_256", b"ECDSA_SHA_384"],
        ):
            candidate = self.response()
            candidate["SigningAlgorithms"] = value
            cases.append(candidate)
        for candidate in cases:
            with self.subTest(value_type=type(candidate.get("SigningAlgorithms")).__name__):
                self.assert_load_fails(result=candidate)


if __name__ == "__main__":
    unittest.main()
