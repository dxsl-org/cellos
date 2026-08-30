import unittest
from dataclasses import FrozenInstanceError, replace

import path_bootstrap  # noqa: F401

from protocol_models import MAX_UINT64, SignedRequest
from state_reader import DynamoStateReader, ReaderError, StateSnapshot
from state_reader_support import (
    EPOCH, TABLE, FakeClient, expected_transaction, fixture, read_result,
)
from vector_support import unsigned_request


class SignedRequestChild(SignedRequest):
    pass


class IntChild(int):
    pass


class StrChild(str):
    pass


class StateReaderContractTests(unittest.TestCase):
    def assert_reader_error(self, operation, message):
        with self.assertRaises(ReaderError) as raised:
            operation()
        self.assertEqual(str(raised.exception), message)
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_one_exact_ordered_transaction_returns_frozen_snapshot(self):
        request, registration, state = fixture()
        client = FakeClient()
        snapshot = DynamoStateReader(client, TABLE, EPOCH).load_snapshot(request)

        self.assertEqual(client.calls, [expected_transaction(request)])
        self.assertEqual(snapshot, StateSnapshot(registration, state))
        self.assertIsNot(snapshot.registration, registration)
        self.assertIsNot(snapshot.state, state)
        with self.assertRaises(FrozenInstanceError):
            snapshot.state = state

    def test_response_items_and_transaction_mutation_cannot_change_snapshot(self):
        request, registration, state = fixture()
        result = read_result(registration, state)
        client = FakeClient(result)
        snapshot = DynamoStateReader(client, TABLE, EPOCH).load_snapshot(request)

        result["Responses"][0]["Item"].clear()
        result["Responses"][1]["Item"].clear()
        client.calls[0]["TransactItems"].clear()
        self.assertEqual(snapshot, StateSnapshot(registration, state))
        self.assertEqual(request, fixture()[0])

    def test_request_is_exact_and_revalidated_before_io(self):
        request = fixture()[0]
        child = SignedRequestChild(
            request.device_id, request.authority_id, request.boot_epoch,
            request.request_id, request.purpose, request.nonce,
            request.authority_pubkey, request.signature,
        )
        invalid = (
            object(), unsigned_request(request), child,
            replace(request, boot_epoch=True),
            replace(request, signature=request.signature[:-1] + bytes([request.signature[-1] ^ 1])),
        )
        for candidate in invalid:
            with self.subTest(candidate=type(candidate).__name__):
                client = FakeClient()
                self.assert_reader_error(
                    lambda: DynamoStateReader(client, TABLE, EPOCH).load_snapshot(candidate),
                    "state reader operation failed",
                )
                self.assertEqual(client.calls, [])

    def test_configuration_is_exact_and_value_free(self):
        class MissingOperation:
            pass

        class NonCallable:
            transact_get_items = None

        class HostileOperation:
            @property
            def transact_get_items(self):
                raise RuntimeError("configuration credential secret")

        valid_client = FakeClient()
        cases = (
            (MissingOperation(), TABLE, EPOCH),
            (NonCallable(), TABLE, EPOCH),
            (HostileOperation(), TABLE, EPOCH),
            (valid_client, "", EPOCH),
            (valid_client, StrChild(TABLE), EPOCH),
            (valid_client, TABLE, -1),
            (valid_client, TABLE, MAX_UINT64 + 1),
            (valid_client, TABLE, True),
            (valid_client, TABLE, IntChild(EPOCH)),
        )
        for client, table, epoch in cases:
            with self.subTest(table=repr(table), epoch=repr(epoch)):
                self.assert_reader_error(
                    lambda: DynamoStateReader(client, table, epoch),
                    "invalid state reader configuration",
                )

    def test_reader_pins_callable_and_has_only_transactional_read_surface(self):
        request = fixture()[0]
        client = FakeClient()
        reader = DynamoStateReader(client, TABLE, EPOCH)

        def forbidden(**kwargs):
            raise AssertionError(kwargs)

        client.transact_get_items = forbidden
        snapshot = reader.load_snapshot(request)
        self.assertEqual(snapshot.state.source_epoch, EPOCH)
        self.assertEqual(len(client.calls), 1)
        for name in (
            "transact_write_items", "put_item", "update_item", "delete_item",
            "scan", "query", "get_item", "sign", "kms", "clock", "receipt",
        ):
            self.assertFalse(hasattr(reader, name), name)

    def test_client_exposes_only_the_one_injected_read_operation(self):
        class SurfaceClient:
            def __init__(self):
                self.lookups = []
                self.result = read_result()
                self.calls = []

            def __getattr__(self, name):
                self.lookups.append(name)
                if name == "transact_get_items":
                    return self.read
                raise AssertionError(name)

            def read(self, **kwargs):
                self.calls.append(kwargs)
                return self.result

        request = fixture()[0]
        client = SurfaceClient()
        DynamoStateReader(client, TABLE, EPOCH).load_snapshot(request)
        self.assertEqual(client.lookups, ["transact_get_items"])
        self.assertEqual(client.calls, [expected_transaction(request)])


if __name__ == "__main__":
    unittest.main()
