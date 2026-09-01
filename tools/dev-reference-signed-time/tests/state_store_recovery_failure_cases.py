import copy
from dataclasses import replace

from state_codec import encode_receipt
from state_store import DynamoStateStore, StoreError
from state_store_support import (
    EPOCH, KEY_ID, TABLE, FakeClient, expected_read, fixture, receipt_read,
    response_metadata,
)


class RecoveryFailureCases:
    def test_preflight_rejects_wrong_manifest_key_or_source_epoch(self):
        registration, _, _, _, request, receipt = fixture()
        for epoch, key_id in ((EPOCH + 1, KEY_ID), (EPOCH, "wrong-key")):
            with self.subTest(epoch=epoch, key_id=key_id):
                client = FakeClient(read_result=receipt_read(receipt))
                with self.assertRaises(StoreError):
                    DynamoStateStore(
                        client, TABLE, epoch, key_id,
                    ).recover_committed(request, registration)
                self.assertEqual(client.calls, [("read", expected_read(request))])

    def test_preflight_malformed_envelopes_and_receipts_fail_closed(self):
        registration, _, _, _, request, receipt = fixture()
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
                    ).recover_committed(request, registration)
                self.assertEqual(str(raised.exception), "state store operation failed")
                self.assertIsNone(raised.exception.__cause__)
                self.assertIsNone(raised.exception.__context__)
                self.assertEqual(client.calls, [("read", expected_read(request))])

    def test_preflight_read_exception_details_are_suppressed(self):
        registration, _, _, _, request, _ = fixture()
        client = FakeClient(read_error=RuntimeError("read credential secret"))
        with self.assertRaises(StoreError) as raised:
            DynamoStateStore(
                client, TABLE, EPOCH, KEY_ID,
            ).recover_committed(request, registration)
        self.assertEqual(str(raised.exception), "state store operation failed")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)
        self.assertNotIn("secret", repr(raised.exception))
        self.assertEqual(client.calls, [("read", expected_read(request))])
