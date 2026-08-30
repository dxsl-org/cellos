import copy
import unittest
from dataclasses import replace
import path_bootstrap

from state_codec import encode_receipt
from state_store import DynamoStateStore, StoreError
from state_store_support import (
    EPOCH, KEY_ID, TABLE, FakeClient, expected_read, expected_write, fixture,
    receipt_read, request_signer,
)


class StateStoreRecoveryTests(unittest.TestCase):
    def test_ambiguous_write_recovers_only_the_exact_durable_receipt(self):
        registration, state, floor, sample, request, receipt = fixture()
        client = FakeClient(
            write_error=TimeoutError("provider timeout secret"),
            read_result=receipt_read(receipt),
        )

        result = DynamoStateStore(client, TABLE, EPOCH, KEY_ID).commit_allocation(
            registration, state, floor, sample, request,
        )

        self.assertEqual(result, receipt)
        self.assertEqual(client.calls, [
            ("write", expected_write(registration, state, receipt)),
            ("read", expected_read(request)),
        ])

    def test_exact_duplicate_request_recovers_without_second_write(self):
        registration, state, floor, sample, request, receipt = fixture()
        client = FakeClient(
            write_error=RuntimeError("conditional transaction canceled"),
            read_result=receipt_read(receipt),
        )
        store = DynamoStateStore(client, TABLE, EPOCH, KEY_ID)

        first_recovery = store.commit_allocation(registration, state, floor, sample, request)

        self.assertEqual(first_recovery, receipt)
        self.assertEqual([name for name, _ in client.calls], ["write", "read"])
        self.assertEqual(first_recovery.response.source_sequence, receipt.response.source_sequence)
        self.assertEqual(first_recovery.response.unix_seconds, receipt.response.unix_seconds)
        self.assertEqual(first_recovery.response.expires_at, receipt.response.expires_at)

    def test_stale_state_with_new_request_fails_after_one_exact_read(self):
        make_request = request_signer()
        changed = make_request(request_id=b"x" * 16)
        registration, state, floor, sample, changed, changed_receipt = fixture(
            request=changed,
        )
        client = FakeClient(write_error=RuntimeError("stale"), read_result={"Responses": [{}]})

        with self.assertRaises(StoreError):
            DynamoStateStore(client, TABLE, EPOCH, KEY_ID).commit_allocation(
                registration, state, floor, sample, changed,
            )

        self.assertEqual(client.calls, [
            ("write", expected_write(registration, state, changed_receipt)),
            ("read", expected_read(changed)),
        ])

    def test_reused_request_id_with_different_request_bytes_is_rejected(self):
        make_request = request_signer()
        request = make_request()
        registration, state, floor, sample, request, receipt = fixture(request=request)
        changed = make_request(nonce=b"z" * 32)
        changed_registration, _, _, _, _, _ = fixture(request=changed)
        client = FakeClient(write_error=RuntimeError("duplicate"), read_result=receipt_read(receipt))

        with self.assertRaises(StoreError):
            DynamoStateStore(client, TABLE, EPOCH, KEY_ID).commit_allocation(
                changed_registration, state, floor, sample, changed,
            )

        self.assertEqual([name for name, _ in client.calls], ["write", "read"])

    def test_absent_malformed_and_substituted_receipts_fail_closed(self):
        registration, state, floor, sample, request, receipt = fixture()
        wrong_receipt = replace(receipt, request_digest=b"w" * 32)
        malformed = copy.deepcopy(encode_receipt(receipt))
        malformed["pk"] = {"S": "request#wrong/key"}
        failed_status = receipt_read(receipt)
        failed_status["ResponseMetadata"]["HTTPStatusCode"] = 503
        results = (
            [], {}, {"Responses": ()}, {"Responses": []}, {"Responses": [{}]},
            {"Responses": [{"Item": encode_receipt(receipt)}]},
            failed_status,
            {"Responses": [{"Item": malformed}], "ResponseMetadata": {
                "HTTPStatusCode": 200, "RequestId": "malformed",
            }},
            receipt_read(wrong_receipt),
            {"Responses": [{"Item": encode_receipt(receipt), "extra": {}}]},
        )
        for read_result in results:
            with self.subTest(read_result=read_result):
                client = FakeClient(write_error=RuntimeError("ambiguous"), read_result=read_result)
                with self.assertRaises(StoreError):
                    DynamoStateStore(client, TABLE, EPOCH, KEY_ID).commit_allocation(
                        registration, state, floor, sample, request,
                    )
                self.assertEqual([name for name, _ in client.calls], ["write", "read"])
                self.assertEqual(client.calls[1][1], expected_read(request))

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


    def test_write_and_read_exception_details_are_suppressed(self):
        registration, state, floor, sample, request, _ = fixture()
        client = FakeClient(
            write_error=RuntimeError("write credential secret"),
            read_error=RuntimeError("read credential secret"),
        )

        with self.assertRaises(StoreError) as raised:
            DynamoStateStore(client, TABLE, EPOCH, KEY_ID).commit_allocation(
                registration, state, floor, sample, request,
            )

        self.assertEqual(str(raised.exception), "state store operation failed")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)
        self.assertNotIn("secret", repr(raised.exception))
        self.assertEqual([name for name, _ in client.calls], ["write", "read"])


if __name__ == "__main__":
    unittest.main()
