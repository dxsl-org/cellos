import hashlib
import unittest
from dataclasses import FrozenInstanceError, replace
import path_bootstrap

from allocation import (
    AdmittedSample, AllocationError, AllocationState, allocate_response,
)
from protocol_models import MAX_UINT64, SignedRequest
from vector_support import request_fixture, unsigned_request

class IntChild(int):
    pass


class StrChild(str):
    pass


class BytesChild(bytes):
    pass


class AllocationTests(unittest.TestCase):
    def setUp(self):
        self.state = AllocationState(7, 41, 1_699_999_999)
        self.sample = AdmittedSample(1_700_000_000, 1_700_000_100, 1_700_000_090)
        self.vector, self.request, _ = request_fixture()

    def allocate(self, **changes):
        values = {
            "configured_source_epoch": 7,
            "manifest_key_id": "manifest-key",
            "state": self.state,
            "protected_server_floor": 1_700_000_050,
            "sample": self.sample,
            "request": self.request,
        }
        values.update(changes)
        return allocate_response(**values)

    def assert_error(self, code, **changes):
        with self.assertRaises(AllocationError) as caught:
            self.allocate(**changes)
        self.assertEqual(str(caught.exception), code)
        self.assertEqual(caught.exception.code, code)
        self.assertIsNone(caught.exception.__cause__)

    def test_allocates_at_sample_floor_and_binds_exact_request(self):
        result = self.allocate()
        self.assertEqual(result.state, AllocationState(7, 42, 1_700_000_000))
        response = result.response
        self.assertEqual(
            (response.source_epoch, response.source_sequence, response.unix_seconds),
            (7, 42, 1_700_000_000),
        )
        self.assertEqual(response.expires_at, 1_700_000_060)
        for name in ("device_id", "authority_id", "request_id", "nonce"):
            self.assertIs(getattr(response, name), getattr(self.request, name))
        self.assertEqual(response.boot_epoch, self.request.boot_epoch)
        self.assertEqual(response.purpose, self.request.purpose)
        self.assertEqual(response.key_id, "manifest-key")
        canonical = bytes.fromhex(self.vector["canonical_cbor_hex"])
        self.assertEqual(result.request_digest, hashlib.sha256(canonical).digest())
        self.assertEqual(len(result.request_digest), 32)

    def test_allocates_at_last_plus_one_and_valid_until_expiry(self):
        result = self.allocate(
            state=replace(self.state, last_unix_seconds=1_700_000_040),
            sample=replace(self.sample, sample_valid_until=1_700_000_075),
        )
        self.assertEqual(result.response.unix_seconds, 1_700_000_041)
        self.assertEqual(result.response.expires_at, 1_700_000_075)
        self.assertEqual(result.state.last_unix_seconds, 1_700_000_041)

    def test_uint64_zero_and_maximum_boundaries_succeed(self):
        result = self.allocate(
            configured_source_epoch=MAX_UINT64,
            state=AllocationState(MAX_UINT64, MAX_UINT64 - 1, MAX_UINT64 - 61),
            protected_server_floor=MAX_UINT64,
            sample=AdmittedSample(0, MAX_UINT64, MAX_UINT64),
        )
        self.assertEqual(result.state.source_sequence, MAX_UINT64)
        self.assertEqual(result.response.unix_seconds, MAX_UINT64 - 60)
        self.assertEqual(result.response.expires_at, MAX_UINT64)
        zero = self.allocate(
            configured_source_epoch=0,
            state=AllocationState(0, 0, 0),
            protected_server_floor=0,
            sample=AdmittedSample(0, 61, 61),
        )
        self.assertEqual((zero.state.source_sequence, zero.response.unix_seconds), (1, 1))

    def test_every_allocation_uint_rejects_bad_range_and_exact_type(self):
        bad_values = (-1, MAX_UINT64 + 1, True, IntChild(1), None, 1.0, "1")
        for value in bad_values:
            with self.subTest(field="configured_source_epoch", value=repr(value)):
                self.assert_error("invalid-source-epoch", configured_source_epoch=value)
            with self.subTest(field="protected_server_floor", value=repr(value)):
                self.assert_error("invalid-protected-floor", protected_server_floor=value)
            for field in ("source_epoch", "source_sequence", "last_unix_seconds"):
                with self.subTest(field=field, value=repr(value)):
                    self.assert_error("invalid-state", state=replace(self.state, **{field: value}))
            for field in ("sample_floor", "sample_ceiling", "sample_valid_until"):
                with self.subTest(field=field, value=repr(value)):
                    self.assert_error("invalid-sample", sample=replace(self.sample, **{field: value}))
            with self.subTest(field="boot_epoch", value=repr(value)):
                self.assert_error("invalid-request", request=replace(self.request, boot_epoch=value))

    def test_exact_container_request_key_and_request_field_types(self):
        class StateChild(AllocationState):
            pass

        class SampleChild(AdmittedSample):
            pass

        class RequestChild(SignedRequest):
            pass

        self.assert_error("invalid-state-type", state=StateChild(7, 41, 1_699_999_999))
        self.assert_error(
            "invalid-sample-type",
            sample=SampleChild(
                self.sample.sample_floor, self.sample.sample_ceiling,
                self.sample.sample_valid_until,
            ),
        )
        self.assert_error("invalid-state-type", state=object())
        self.assert_error("invalid-sample-type", sample=object())
        request_child = RequestChild(
            self.request.device_id, self.request.authority_id, self.request.boot_epoch,
            self.request.request_id, self.request.purpose, self.request.nonce,
            self.request.authority_pubkey, self.request.signature,
        )
        for request in (unsigned_request(self.request), object(), request_child):
            self.assert_error("invalid-request", request=request)
        for key_id in ("", b"key", StrChild("key"), None):
            self.assert_error("invalid-key-id", manifest_key_id=key_id)
        byte_fields = {
            "device_id": 32, "authority_id": 32, "request_id": 16,
            "nonce": 32, "authority_pubkey": 44,
        }
        for field, length in byte_fields.items():
            for value in (bytearray(length), BytesChild(b"x" * length), b"x" * (length - 1)):
                with self.subTest(field=field, value_type=type(value).__name__):
                    self.assert_error("invalid-request", request=replace(self.request, **{field: value}))
        for purpose in (True, IntChild(2), 0, 4):
            self.assert_error("invalid-request", request=replace(self.request, purpose=purpose))

    def test_tampered_signature_field_and_key_are_invalid_requests(self):
        key = self.request.authority_pubkey
        tampered = (
            replace(self.request, signature=self.request.signature[:-1] + b"\x00"),
            replace(self.request, nonce=b"\x00" + self.request.nonce[1:]),
            replace(self.request, authority_pubkey=key[:-1] + bytes([key[-1] ^ 1])),
        )
        for request in tampered:
            self.assert_error("invalid-request", request=request)

    def test_epoch_interval_floor_candidate_overflow_and_expiry_fail_closed(self):
        self.assert_error("source-epoch-mismatch", configured_source_epoch=8)
        self.assert_error(
            "reversed-sample-interval",
            sample=AdmittedSample(1_700_000_100, 1_700_000_000, 1_700_000_101),
        )
        self.assert_error("protected-floor-outside-sample", protected_server_floor=1_699_999_999)
        self.assert_error("protected-floor-outside-sample", protected_server_floor=1_700_000_101)
        self.assert_error(
            "candidate-above-ceiling",
            state=replace(self.state, last_unix_seconds=1_700_000_100),
        )
        self.assert_error("sequence-overflow", state=replace(self.state, source_sequence=MAX_UINT64))
        self.assert_error("time-overflow", state=replace(self.state, last_unix_seconds=MAX_UINT64))
        self.assert_error(
            "time-overflow",
            state=AllocationState(7, 41, MAX_UINT64 - 61),
            protected_server_floor=MAX_UINT64 - 59,
            sample=AdmittedSample(MAX_UINT64 - 59, MAX_UINT64, MAX_UINT64),
        )
        for valid_until in (1_700_000_000, 1_699_999_999):
            self.assert_error(
                "invalid-expiry", sample=replace(self.sample, sample_valid_until=valid_until),
            )

    def test_inputs_and_result_are_immutable_and_failures_do_not_mutate(self):
        originals = (self.state, self.sample, self.request)
        self.allocate()
        self.assertEqual(originals, (self.state, self.sample, self.request))
        with self.assertRaises(FrozenInstanceError):
            self.state.source_sequence = 99
        with self.assertRaises(FrozenInstanceError):
            self.sample.sample_floor = 99
        result = self.allocate()
        with self.assertRaises(FrozenInstanceError):
            result.state.last_unix_seconds = 99
        with self.assertRaises(FrozenInstanceError):
            result.request_digest = b"x" * 32
        self.assert_error("candidate-above-ceiling", state=replace(self.state, last_unix_seconds=1_700_000_100))
        self.assertEqual(originals, (self.state, self.sample, self.request))
