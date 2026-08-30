import copy
import unittest
from dataclasses import replace
import path_bootstrap

from state_codec import encode_receipt
from state_store import DynamoStateStore, StoreError
from state_store_support import (
    EPOCH, KEY_ID, TABLE, FakeClient, expected_read, expected_write, fixture,
    receipt_read, request_signer, response_metadata,
)


class StateStoreRecoveryTests(unittest.TestCase):
    def test_ambiguous_write_recovers_only_the_exact_durable_receipt(self):
        class RecoverySpy(DynamoStateStore):
            def _read_committed(self, candidate):
                self.recovered_request = candidate
                return super()._read_committed(candidate)

        registration, state, floor, sample, request, receipt = fixture()
        client = FakeClient(
            write_error=TimeoutError("provider timeout secret"),
            read_result=receipt_read(receipt),
        )

        store = RecoverySpy(client, TABLE, EPOCH, KEY_ID)
        result = store.commit_allocation(registration, state, floor, sample, request)

        self.assertEqual(result, receipt)
        self.assertIs(store.recovered_request, request)
        self.assertEqual(client.calls, [
            ("write", expected_write(registration, state, receipt)),
            ("read", expected_read(request)),
        ])

    def test_preflight_exact_retry_reads_once_without_allocation_inputs(self):
        _, _, _, _, request, receipt = fixture()
        client = FakeClient(read_result=receipt_read(receipt))

        recovered = DynamoStateStore(
            client, TABLE, EPOCH, KEY_ID,
        ).recover_committed(request)

        self.assertEqual(recovered, receipt.response)
        self.assertEqual(client.calls, [("read", expected_read(request))])
        self.assertEqual(recovered.expires_at, receipt.response.expires_at)

    def test_preflight_canonical_absence_returns_none(self):
        _, _, _, _, request, _ = fixture()
        absent = {
            "Responses": [None],
            "ResponseMetadata": response_metadata("absent"),
        }
        client = FakeClient(read_result=absent)

        recovered = DynamoStateStore(
            client, TABLE, EPOCH, KEY_ID,
        ).recover_committed(request)

        self.assertIsNone(recovered)
        self.assertEqual(client.calls, [("read", expected_read(request))])

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
        _, _, _, _, _, receipt = fixture(request=request)
        changed = make_request(nonce=b"z" * 32)
        client = FakeClient(read_result=receipt_read(receipt))

        with self.assertRaises(StoreError):
            DynamoStateStore(client, TABLE, EPOCH, KEY_ID).recover_committed(changed)

        self.assertEqual(client.calls, [("read", expected_read(changed))])

    def test_preflight_invalid_signature_fails_before_read(self):
        _, _, _, _, request, _ = fixture()
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
            ).recover_committed(invalid)

        self.assertEqual(client.calls, [])

    def test_preflight_rejects_wrong_manifest_key_or_source_epoch(self):
        _, _, _, _, request, receipt = fixture()
        for epoch, key_id in ((EPOCH + 1, KEY_ID), (EPOCH, "wrong-key")):
            with self.subTest(epoch=epoch, key_id=key_id):
                client = FakeClient(read_result=receipt_read(receipt))
                with self.assertRaises(StoreError):
                    DynamoStateStore(
                        client, TABLE, epoch, key_id,
                    ).recover_committed(request)
                self.assertEqual(client.calls, [("read", expected_read(request))])

    def test_preflight_malformed_envelopes_and_receipts_fail_closed(self):
        _, _, _, _, request, receipt = fixture()
        wrong_receipt = replace(receipt, request_digest=b"w" * 32)
        malformed = copy.deepcopy(encode_receipt(receipt))
        malformed["pk"] = {"S": "request#wrong/key"}
        failed_status = receipt_read(receipt)
        failed_status["ResponseMetadata"]["HTTPStatusCode"] = 503
        metadata = {"ResponseMetadata": response_metadata("read")}
        results = (
            [], {}, {"Responses": (), **metadata}, {"Responses": [], **metadata},
            {"Responses": [{}, {}], **metadata}, {"Responses": [[]], **metadata},
            {"Responses": [{}], **metadata},
            {"Responses": [None], "extra": {}, **metadata},
            {"Responses": [{"unexpected": {}}], **metadata},
            {"Responses": [{"Item": encode_receipt(receipt)}]},
            failed_status,
            {"Responses": [{"Item": malformed}], **metadata},
            receipt_read(wrong_receipt),
            {"Responses": [{
                "Item": encode_receipt(receipt), "extra": {},
            }], **metadata},
        )
        for read_result in results:
            with self.subTest(read_result=read_result):
                client = FakeClient(read_result=read_result)
                with self.assertRaises(StoreError) as raised:
                    DynamoStateStore(
                        client, TABLE, EPOCH, KEY_ID,
                    ).recover_committed(request)
                self.assertEqual(str(raised.exception), "state store operation failed")
                self.assertIsNone(raised.exception.__cause__)
                self.assertIsNone(raised.exception.__context__)
                self.assertEqual(client.calls, [("read", expected_read(request))])

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


    def test_preflight_read_exception_details_are_suppressed(self):
        _, _, _, _, request, _ = fixture()
        client = FakeClient(read_error=RuntimeError("read credential secret"))

        with self.assertRaises(StoreError) as raised:
            DynamoStateStore(
                client, TABLE, EPOCH, KEY_ID,
            ).recover_committed(request)

        self.assertEqual(str(raised.exception), "state store operation failed")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)
        self.assertNotIn("secret", repr(raised.exception))
        self.assertEqual(client.calls, [("read", expected_read(request))])


if __name__ == "__main__":
    unittest.main()
