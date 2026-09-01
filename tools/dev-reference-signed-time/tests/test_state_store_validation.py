from collections.abc import Mapping
import unittest
from dataclasses import replace
import path_bootstrap

from allocation import AdmittedSample, AllocationState
from state_codec import AuthorityRegistration
from state_store import DynamoStateStore, StoreError
from state_store_support import CONTRACT, EPOCH, KEY_ID, FakeClient, fixture


class StateStoreValidationTests(unittest.TestCase):
    def test_constructor_rejects_malformed_contract_and_client_operations(self):
        client = FakeClient()
        cases = (
            (object(), CONTRACT),
            (client, object()),
            (client, None),
            (client, True),
        )
        for arguments in cases:
            with self.subTest(contract_type=type(arguments[1]).__name__):
                with self.assertRaises(StoreError) as raised:
                    DynamoStateStore(*arguments)
                self.assertEqual(str(raised.exception), "invalid state store configuration")
                self.assertIsNone(raised.exception.__cause__)
                self.assertIsNone(raised.exception.__context__)

        for field in ("transact_write_items", "transact_get_items"):
            broken = FakeClient()
            setattr(broken, field, None)
            with self.subTest(field=field):
                with self.assertRaises(StoreError):
                    DynamoStateStore(broken, CONTRACT)

    def test_revoked_and_wrong_registration_tuples_make_no_client_call(self):
        registration, state, floor, sample, request, _ = fixture()
        other_key = registration.public_key_der[:-1] + bytes([registration.public_key_der[-1] ^ 1])
        cases = (
            replace(registration, revoked=True),
            replace(registration, device_id=b"d" * 32),
            replace(registration, authority_id=b"a" * 32),
            replace(registration, public_key_der=other_key),
        )
        for changed in cases:
            with self.subTest(changed=changed):
                client = FakeClient()
                with self.assertRaises(StoreError):
                    DynamoStateStore(client, CONTRACT).commit_allocation(
                        changed, state, floor, sample, request,
                    )
                self.assertEqual(client.calls, [])

    def test_exact_registration_type_and_valid_record_are_required(self):
        registration, state, floor, sample, request, _ = fixture()

        class RegistrationChild(AuthorityRegistration):
            pass

        malformed = (
            object(),
            RegistrationChild(
                registration.device_id, registration.authority_id,
                registration.public_key_der, False,
            ),
            replace(registration, public_key_der=b"not-a-public-key"),
        )
        for changed in malformed:
            client = FakeClient()
            with self.subTest(changed=type(changed).__name__):
                with self.assertRaises(StoreError):
                    DynamoStateStore(client, CONTRACT).commit_allocation(
                        changed, state, floor, sample, request,
                    )
                self.assertEqual(client.calls, [])

    def test_malformed_allocation_inputs_fail_before_persistence(self):
        registration, state, floor, sample, request, _ = fixture()
        cases = (
            (object(), floor, sample, request),
            (AllocationState(EPOCH + 1, state.source_sequence, state.last_unix_seconds), floor, sample, request),
            (state, True, sample, request),
            (state, floor, object(), request),
            (state, floor, AdmittedSample(sample.sample_ceiling, sample.sample_floor, sample.sample_valid_until), request),
            (state, floor, sample, object()),
        )
        for changed_state, changed_floor, changed_sample, changed_request in cases:
            client = FakeClient()
            with self.subTest(types=tuple(map(type, (changed_state, changed_floor, changed_sample, changed_request)))):
                with self.assertRaises(StoreError) as raised:
                    DynamoStateStore(client, CONTRACT).commit_allocation(
                        registration, changed_state, changed_floor, changed_sample, changed_request,
                    )
                self.assertEqual(str(raised.exception), "state store operation failed")
                self.assertIsNone(raised.exception.__context__)
                self.assertEqual(client.calls, [])

    def test_malformed_write_results_are_ambiguous_and_not_retried(self):
        class RaisingMapping(Mapping):
            def __getitem__(self, key):
                raise RuntimeError("envelope secret")

            def __iter__(self):
                return iter(())

            def __len__(self):
                return 0

        registration, state, floor, sample, request, _ = fixture()
        results = (
            [], {}, {"unexpected": object()}, {"ResponseMetadata": {}},
            {"ResponseMetadata": {"HTTPStatusCode": True, "RequestId": "request"}},
            {"ResponseMetadata": {"HTTPStatusCode": 200, "RequestId": ""}},
            RaisingMapping(),
        )
        for result in results:
            with self.subTest(result=result):
                client = FakeClient(write_result=result)
                with self.assertRaises(StoreError) as raised:
                    DynamoStateStore(client, CONTRACT).commit_allocation(
                        registration, state, floor, sample, request,
                    )
                self.assertIsNone(raised.exception.__cause__)
                self.assertIsNone(raised.exception.__context__)
                self.assertEqual([name for name, _ in client.calls], ["write", "read"])


if __name__ == "__main__":
    unittest.main()
