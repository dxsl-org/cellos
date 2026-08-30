import hashlib
import unittest
from dataclasses import FrozenInstanceError, replace
import path_bootstrap

from allocation import (
    AdmittedSample, AllocationResult, AllocationState, allocate_response,
)
from protocol import response_signing_bytes
from protocol_models import SignedRequest, SignedResponse, UnsignedResponse
from receipt import (
    SOURCE_STATE_KEY, Receipt, ReceiptError, authority_registration_key,
    construct_receipt, recover_receipt, request_receipt_key,
)
from vector_support import request_fixture, unsigned_request

class BytesChild(bytes):
    pass


class IntChild(int):
    pass


class StrChild(str):
    pass


class ReceiptTests(unittest.TestCase):
    def setUp(self):
        self.vector, self.request, _ = request_fixture()
        self.state = AllocationState(7, 41, 1_699_999_999)
        self.sample = AdmittedSample(1_700_000_000, 1_700_000_100, 1_700_000_090)
        self.allocation = allocate_response(
            configured_source_epoch=7,
            manifest_key_id="manifest-key",
            state=self.state,
            protected_server_floor=1_700_000_050,
            sample=self.sample,
            request=self.request,
        )
        self.receipt = construct_receipt(self.allocation)

    def recover(self, receipt=None, request=None, **changes):
        values = {
            "receipt": self.receipt if receipt is None else receipt,
            "request": self.request if request is None else request,
            "configured_source_epoch": 7,
            "manifest_key_id": "manifest-key",
        }
        values.update(changes)
        return recover_receipt(**values)

    def assert_error(self, code, **values):
        with self.assertRaises(ReceiptError) as caught:
            self.recover(**values)
        self.assertEqual((str(caught.exception), caught.exception.code), (code, code))
        self.assertIsNone(caught.exception.__cause__)
        self.assertIsNone(caught.exception.__context__)
    def test_keys_are_exact_lowerhex_and_unambiguous(self):
        authority = bytes(range(32))
        request_id = bytes(range(16))
        self.assertEqual(SOURCE_STATE_KEY, "source#cellos-dev-time-v1/state")
        self.assertEqual(
            authority_registration_key(authority),
            "authority#000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f/registration",
        )
        self.assertEqual(
            request_receipt_key(authority, request_id),
            "request#000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f/"
            "000102030405060708090a0b0c0d0e0f",
        )

    def test_key_identifiers_require_exact_bytes_and_lengths(self):
        for value in (b"x" * 31, b"x" * 33, bytearray(32), BytesChild(b"x" * 32), None, "00"):
            with self.subTest(authority=repr(value)):
                with self.assertRaisesRegex(ReceiptError, "^invalid-authority-id$"):
                    authority_registration_key(value)
                with self.assertRaisesRegex(ReceiptError, "^invalid-authority-id$"):
                    request_receipt_key(value, b"r" * 16)
        for value in (b"x" * 15, b"x" * 17, bytearray(16), BytesChild(b"x" * 16), None, "00"):
            with self.subTest(request_id=repr(value)):
                with self.assertRaisesRegex(ReceiptError, "^invalid-request-id$"):
                    request_receipt_key(b"a" * 32, value)

    def test_constructs_exact_digest_and_recovers_same_immutable_labels(self):
        canonical = bytes.fromhex(self.vector["canonical_cbor_hex"])
        before = response_signing_bytes(self.allocation.response)
        self.assertEqual(self.receipt.request_digest, hashlib.sha256(canonical).digest())
        self.assertEqual(len(self.receipt.request_digest), 32)
        self.assertIs(self.receipt.request_digest, self.allocation.request_digest)
        self.assertIs(self.receipt.response, self.allocation.response)
        recovered = self.recover()
        self.assertIs(recovered, self.receipt.response)
        self.assertEqual(response_signing_bytes(recovered), before)
        self.assertEqual(
            (recovered.source_sequence, recovered.unix_seconds, recovered.expires_at),
            (42, 1_700_000_000, 1_700_000_060),
        )

    def test_construction_requires_exact_consistent_allocation(self):
        class AllocationChild(AllocationResult):
            pass

        child = AllocationChild(
            self.allocation.state, self.allocation.response, self.allocation.request_digest,
        )
        bad = (
            object(), child, replace(self.allocation, state=replace(self.allocation.state, source_epoch=IntChild(7))),
            replace(self.allocation, state=object()),
            replace(self.allocation, request_digest=b"x" * 31),
            replace(self.allocation, request_digest=BytesChild(b"x" * 32)),
            replace(self.allocation, response=object()),
            replace(self.allocation, state=replace(self.allocation.state, source_sequence=99)),
        )
        for allocation in bad:
            with self.subTest(allocation=type(allocation).__name__):
                with self.assertRaisesRegex(ReceiptError, "^invalid-allocation$"):
                    construct_receipt(allocation)

    def test_missing_wrong_and_malformed_receipts_fail_locally(self):
        class ReceiptChild(Receipt):
            pass

        with self.assertRaisesRegex(ReceiptError, "^missing-receipt$"):
            recover_receipt(
                None, self.request, configured_source_epoch=7,
                manifest_key_id="manifest-key",
            )
        malformed = (
            object(), ReceiptChild(self.receipt.request_digest, self.receipt.response),
            Receipt(b"x" * 31, self.receipt.response),
            Receipt(BytesChild(b"x" * 32), self.receipt.response),
            Receipt(self.receipt.request_digest, object()),
            Receipt(self.receipt.request_digest, replace(self.receipt.response, request_id=b"x" * 15)),
        )
        for receipt in malformed:
            with self.subTest(receipt=type(receipt).__name__):
                self.assert_error("malformed-receipt", receipt=receipt)

    def test_unsigned_subclass_and_tampered_requests_are_rejected(self):
        class RequestChild(SignedRequest):
            pass

        child = RequestChild(*[getattr(self.request, field) for field in self.request.__dataclass_fields__])
        requests = (
            unsigned_request(self.request), child, object(),
            replace(self.request, signature=self.request.signature[:-1] + b"\x00"),
            replace(self.request, nonce=b"\x00" + self.request.nonce[1:]),
        )
        for request in requests:
            with self.subTest(request=type(request).__name__):
                self.assert_error("invalid-request", request=request)

    def test_digest_mismatch_rejects_reused_request_id(self):
        different_bytes_receipt = Receipt(b"\x00" * 32, self.receipt.response)
        self.assert_error("request-digest-mismatch", receipt=different_bytes_receipt)

    def test_every_response_request_binding_substitution_is_rejected(self):
        substitutions = {
            "device_id": b"d" * 32,
            "authority_id": b"a" * 32,
            "boot_epoch": self.request.boot_epoch + 1,
            "request_id": b"r" * 16,
            "purpose": 1 if self.request.purpose != 1 else 2,
            "nonce": b"n" * 32,
        }
        for field, value in substitutions.items():
            receipt = Receipt(self.receipt.request_digest, replace(self.receipt.response, **{field: value}))
            with self.subTest(field=field):
                self.assert_error("receipt-mismatch", receipt=receipt)

    def test_source_key_and_response_schema_substitutions_fail(self):
        self.assert_error("receipt-mismatch", configured_source_epoch=8)
        self.assert_error("receipt-mismatch", manifest_key_id="other-key")
        signed = SignedResponse(
            *[getattr(self.receipt.response, field) for field in self.receipt.response.__dataclass_fields__],
            signature=b"signature",
        )
        self.assert_error("malformed-receipt", receipt=Receipt(self.receipt.request_digest, signed))
        for field, value in (("expires_at", self.receipt.response.unix_seconds), ("source_sequence", True)):
            bad = replace(self.receipt.response, **{field: value})
            self.assert_error("malformed-receipt", receipt=Receipt(self.receipt.request_digest, bad))
        for value in (-1, True, IntChild(7), None):
            self.assert_error("invalid-source-epoch", configured_source_epoch=value)
        for value in ("", b"key", StrChild("key"), None):
            self.assert_error("invalid-key-id", manifest_key_id=value)

    def test_receipt_inputs_response_and_result_are_frozen_and_unchanged(self):
        originals = (self.state, self.sample, self.request, self.allocation, self.receipt)
        self.recover()
        self.assertEqual(originals, (self.state, self.sample, self.request, self.allocation, self.receipt))
        for target, name, value in (
            (self.receipt, "request_digest", b"x" * 32),
            (self.receipt.response, "expires_at", 99),
            (self.allocation, "response", object()),
        ):
            with self.assertRaises(FrozenInstanceError):
                setattr(target, name, value)
