import unittest
from dataclasses import replace
import path_bootstrap

from state_codec import AuthorityRegistration
from state_store_recovery_failure_cases import RecoveryFailureCases
from state_store import DynamoStateStore, StoreError
from state_store_support import (
    EPOCH, KEY_ID, TABLE, FakeClient, expected_read, expected_write, fixture,
    receipt_read, request_signer, response_metadata,
)


class StateStoreRecoveryTests(RecoveryFailureCases, unittest.TestCase):
    def test_ambiguous_write_recovers_only_the_exact_durable_receipt(self):
        class RecoverySpy(DynamoStateStore):
            def _read_committed(self, candidate, candidate_registration):
                self.recovered_request = candidate
                self.recovered_registration = candidate_registration
                return super()._read_committed(candidate, candidate_registration)

        registration, state, floor, sample, request, receipt = fixture()
        client = FakeClient(
            write_error=TimeoutError("provider timeout secret"),
            read_result=receipt_read(receipt),
        )

        store = RecoverySpy(client, TABLE, EPOCH, KEY_ID)
        result = store.commit_allocation(registration, state, floor, sample, request)

        self.assertEqual(result, receipt)
        self.assertIs(store.recovered_request, request)
        self.assertIs(store.recovered_registration, registration)
        self.assertEqual(client.calls, [
            ("write", expected_write(registration, state, receipt)),
            ("read", expected_read(request)),
        ])

    def test_preflight_exact_retry_reads_once_without_allocation_inputs(self):
        registration, _, _, _, request, receipt = fixture()
        client = FakeClient(read_result=receipt_read(receipt))

        recovered = DynamoStateStore(
            client, TABLE, EPOCH, KEY_ID,
        ).recover_committed(request, registration)

        self.assertEqual(recovered, receipt.response)
        self.assertEqual(client.calls, [("read", expected_read(request))])
        self.assertEqual(recovered.expires_at, receipt.response.expires_at)

    def test_preflight_canonical_absence_returns_none(self):
        registration, _, _, _, request, _ = fixture()
        absent = {
            "Responses": [None],
            "ResponseMetadata": response_metadata("absent"),
        }
        client = FakeClient(read_result=absent)

        recovered = DynamoStateStore(
            client, TABLE, EPOCH, KEY_ID,
        ).recover_committed(request, registration)

        self.assertIsNone(recovered)
        self.assertEqual(client.calls, [("read", expected_read(request))])

    def test_absent_pre_read_still_recovers_concurrent_identical_winner(self):
        registration, state, floor, sample, request, winner = fixture()
        absent = {
            "Responses": [None],
            "ResponseMetadata": response_metadata("absent"),
        }
        client = FakeClient(
            write_error=RuntimeError("conditional conflict"),
            read_results=(absent, receipt_read(winner)),
        )
        store = DynamoStateStore(client, TABLE, EPOCH, KEY_ID)

        self.assertIsNone(store.recover_committed(request, registration))
        recovered = store.commit_allocation(
            registration, state, floor, sample, request,
        )

        self.assertEqual(recovered, winner)
        self.assertEqual(recovered.response, winner.response)
        self.assertEqual(recovered.response.expires_at, winner.response.expires_at)
        self.assertEqual(client.calls, [
            ("read", expected_read(request)),
            ("write", expected_write(registration, state, winner)),
            ("read", expected_read(request)),
        ])

    def test_ambiguous_write_converts_canonical_absence_to_store_error(self):
        make_request = request_signer()
        changed = make_request(request_id=b"x" * 16)
        registration, state, floor, sample, changed, changed_receipt = fixture(
            request=changed,
        )
        absent = {
            "Responses": [None],
            "ResponseMetadata": response_metadata("absent"),
        }
        client = FakeClient(write_error=RuntimeError("stale"), read_result=absent)

        with self.assertRaises(StoreError):
            DynamoStateStore(client, TABLE, EPOCH, KEY_ID).commit_allocation(
                registration, state, floor, sample, changed,
            )

        self.assertEqual(client.calls, [
            ("write", expected_write(registration, state, changed_receipt)),
            ("read", expected_read(changed)),
        ])

    def test_preflight_rejects_altered_request_bytes(self):
        make_request = request_signer()
        request = make_request()
        registration, _, _, _, _, receipt = fixture(request=request)
        changed = make_request(nonce=b"z" * 32)
        client = FakeClient(read_result=receipt_read(receipt))

        with self.assertRaises(StoreError):
            DynamoStateStore(client, TABLE, EPOCH, KEY_ID).recover_committed(
                changed, registration,
            )

        self.assertEqual(client.calls, [("read", expected_read(changed))])

    def test_preflight_invalid_signature_fails_before_read(self):
        registration, _, _, _, request, _ = fixture()
        signature = request.signature[:-1] + bytes([request.signature[-1] ^ 1])
        invalid = replace(request, signature=signature)
        absent = {
            "Responses": [None],
            "ResponseMetadata": response_metadata("absent"),
        }
        client = FakeClient(read_result=absent)

        with self.assertRaises(StoreError):
            DynamoStateStore(
                client, TABLE, EPOCH, KEY_ID,
            ).recover_committed(invalid, registration)

        self.assertEqual(client.calls, [])

    def test_preflight_requires_exact_active_registration_before_read(self):
        class RegistrationSubclass(AuthorityRegistration):
            pass

        registration, _, _, _, request, _ = fixture()
        other_request = request_signer()()
        subclass = RegistrationSubclass(
            registration.device_id, registration.authority_id,
            registration.public_key_der, False,
        )
        cases = (
            (request, replace(registration, revoked=True)),
            (request, replace(registration, device_id=b"x" * 32)),
            (request, replace(registration, authority_id=b"y" * 32)),
            (request, replace(registration, public_key_der=other_request.authority_pubkey)),
            (request, subclass),
            (request, replace(registration, public_key_der=b"malformed")),
            (other_request, registration),
        )
        for candidate_request, candidate_registration in cases:
            with self.subTest(registration=type(candidate_registration).__name__):
                client = FakeClient(read_result={"must": "not be read"})
                with self.assertRaises(StoreError):
                    DynamoStateStore(
                        client, TABLE, EPOCH, KEY_ID,
                    ).recover_committed(candidate_request, candidate_registration)
                self.assertEqual(client.calls, [])

    def test_malformed_write_envelope_recovers_only_exact_receipt(self):
        registration, state, floor, sample, request, receipt = fixture()
        client = FakeClient(
            write_result={"ResponseMetadata": {"HTTPStatusCode": 503, "RequestId": "failed"}},
            read_result=receipt_read(receipt),
        )
        result = DynamoStateStore(client, TABLE, EPOCH, KEY_ID).commit_allocation(
            registration, state, floor, sample, request,
        )
        self.assertEqual(result, receipt)
        self.assertEqual([name for name, _ in client.calls], ["write", "read"])




if __name__ == "__main__":
    unittest.main()
