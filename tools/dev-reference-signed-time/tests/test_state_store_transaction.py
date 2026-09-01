import unittest
from dataclasses import FrozenInstanceError
import path_bootstrap

from allocation import AdmittedSample, AllocationState
from state_store import DynamoStateStore
from state_store_support import (
    CONTRACT, EPOCH, KEY_ID, TABLE, FakeClient, expected_write, fixture, write_success,
)


class StateStoreTransactionTests(unittest.TestCase):
    def test_commit_uses_one_exact_low_level_transaction_and_returns_receipt(self):
        registration, state, floor, sample, request, receipt = fixture()
        client = FakeClient(write_result=write_success("commit-request"))
        store = DynamoStateStore(client, CONTRACT)

        result = store.commit_allocation(registration, state, floor, sample, request)

        self.assertEqual(result, receipt)
        self.assertIsNot(result, receipt)
        self.assertEqual(client.calls, [("write", expected_write(registration, state, receipt))])
        transaction = client.calls[0][1]
        self.assertNotIn("ClientRequestToken", transaction)
        self.assertEqual(
            [next(iter(item)) for item in transaction["TransactItems"]],
            ["ConditionCheck", "ConditionCheck", "Put", "Put"],
        )

    def test_candidate_at_sample_high_bound_commits_without_inferred_state(self):
        state = AllocationState(EPOCH, 99, 1_700_000_029)
        sample = AdmittedSample(1_700_000_020, 1_700_000_030, 1_700_000_031)
        registration, state, floor, sample, request, receipt = fixture(
            state=state, sample=sample, floor=1_700_000_025,
        )
        client = FakeClient(write_result=write_success("high-bound-request"))

        result = DynamoStateStore(client, CONTRACT).commit_allocation(
            registration, state, floor, sample, request,
        )

        self.assertEqual(result.response.unix_seconds, sample.sample_ceiling)
        self.assertEqual(result.response.source_sequence, 100)
        self.assertEqual(result.response.expires_at, sample.sample_valid_until)
        self.assertEqual(client.calls[0], ("write", expected_write(registration, state, receipt)))

    def test_inputs_and_returned_receipt_are_immutable_and_unchanged(self):
        registration, state, floor, sample, request, receipt = fixture()
        before = (registration, state, sample, request)
        result = DynamoStateStore(FakeClient(), CONTRACT).commit_allocation(
            registration, state, floor, sample, request,
        )

        self.assertEqual((registration, state, sample, request), before)
        for value, field in (
            (registration, "revoked"), (state, "source_sequence"),
            (sample, "sample_floor"), (request, "request_id"),
            (result, "request_digest"), (result.response, "unix_seconds"),
        ):
            with self.subTest(value=type(value).__name__):
                with self.assertRaises((FrozenInstanceError, AttributeError)):
                    setattr(value, field, None)
        self.assertEqual(result, receipt)

    def test_only_injected_transaction_methods_are_reachable(self):
        registration, state, floor, sample, request, _ = fixture()

        class MinimalClient:
            def __init__(self):
                self.calls = 0

            def transact_write_items(self, **kwargs):
                self.calls += 1
                return write_success("minimal-request")

            def transact_get_items(self, **kwargs):
                raise AssertionError("successful commit must not read")

        client = MinimalClient()
        DynamoStateStore(client, CONTRACT).commit_allocation(
            registration, state, floor, sample, request,
        )
        self.assertEqual(client.calls, 1)
        for forbidden in ("scan", "update_item", "delete_item", "sign"):
            self.assertFalse(hasattr(client, forbidden))


if __name__ == "__main__":
    unittest.main()
