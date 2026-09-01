import unittest
from dataclasses import replace

import path_bootstrap  # noqa: F401

from handler import HandlerError, SignedTimeHandler
from handler_support import (
    FAILURE,
    Loaders,
    Reader,
    Signer,
    Store,
    dependencies,
    handler_fixture,
    signed_copy,
)
from protocol import encode_response
from receipt import Receipt


class HandlerPathTests(unittest.TestCase):
    def make_handler(self, reader, store, signer, loaders):
        return SignedTimeHandler(
            reader, store, signer, loaders.load_floor, loaders.load_sample
        )

    def test_retry_recovers_before_clock_and_resigns_exact_expired_labels(self):
        data, request, snapshot, receipt, calls, reader, store, signer, loaders = dependencies(
            recovered=replace(receipt_from_fixture(), unix_seconds=1, expires_at=2),
            floor_error=RuntimeError("clock unavailable secret"),
        )
        recovered = store.recovered

        encoded = self.make_handler(reader, store, signer, loaders).handle(data)

        self.assertEqual(encoded, encode_response(signed_copy(recovered)))
        self.assertEqual([call[0] for call in calls], ["snapshot", "recover", "sign"])
        self.assertEqual(calls[0][1], request)
        self.assertIs(calls[1][1], calls[0][1])
        self.assertIs(calls[1][2], snapshot.registration)
        self.assertIs(signer.responses[0], recovered)
        self.assertEqual((recovered.unix_seconds, recovered.expires_at), (1, 2))
        self.assertEqual((loaders.floor_count, loaders.sample_count, store.commit_count), (0, 0, 0))
        self.assertEqual((reader.count, store.recover_count, signer.count), (1, 1, 1))

    def test_fresh_path_orders_each_boundary_once_and_returns_canonical_bytes(self):
        data, request, snapshot, receipt, calls, reader, store, signer, loaders = dependencies()

        encoded = self.make_handler(reader, store, signer, loaders).handle(data)

        self.assertEqual(
            [call[0] for call in calls],
            ["snapshot", "recover", "floor", "sample", "commit", "sign"],
        )
        commit = calls[4]
        self.assertEqual(commit[1:], (
            snapshot.registration,
            snapshot.state,
            loaders.floor,
            loaders.sample,
            request,
        ))
        self.assertEqual(loaders.sample_floors, [loaders.floor])
        self.assertIs(signer.responses[0], receipt.response)
        self.assertEqual(encoded, encode_response(signed_copy(receipt.response)))
        self.assertIs(type(encoded), bytes)
        self.assertEqual(
            (reader.count, store.recover_count, loaders.floor_count,
             loaders.sample_count, store.commit_count, signer.count),
            (1, 1, 1, 1, 1, 1),
        )

    def test_fresh_path_signs_the_receipt_returned_by_commit(self):
        data, request, snapshot, floor, sample, receipt = handler_fixture()
        winner_response = replace(
            receipt.response,
            source_sequence=receipt.response.source_sequence + 1,
            unix_seconds=receipt.response.unix_seconds + 1,
            expires_at=receipt.response.expires_at + 1,
        )
        winner = Receipt(receipt.request_digest, winner_response)
        calls = []
        reader = Reader(calls, snapshot)
        store = Store(calls, receipt=winner)
        signer = Signer(calls)
        loaders = Loaders(calls, floor, sample)

        encoded = self.make_handler(reader, store, signer, loaders).handle(data)

        self.assertIs(signer.responses[0], winner_response)
        self.assertEqual(encoded, encode_response(signed_copy(winner_response)))
        self.assertEqual(store.commit_count, 1)

    def test_constructor_pins_only_the_injected_callables(self):
        data, _, _, receipt, calls, reader, store, signer, loaders = dependencies(
            recovered=receipt_from_fixture()
        )
        service = self.make_handler(reader, store, signer, loaders)
        reader.load_snapshot = lambda request: (_ for _ in ()).throw(AssertionError(request))
        store.recover_committed = lambda request, registration: (_ for _ in ()).throw(
            AssertionError((request, registration))
        )
        signer.sign_response = lambda response: (_ for _ in ()).throw(AssertionError(response))
        loaders.load_floor = lambda: (_ for _ in ()).throw(AssertionError("floor"))
        loaders.load_sample = lambda floor: (_ for _ in ()).throw(AssertionError(floor))

        self.assertEqual(service.handle(data), encode_response(signed_copy(receipt.response)))
        self.assertEqual([call[0] for call in calls], ["snapshot", "recover", "sign"])

    def test_public_surface_and_error_subclass_are_narrow(self):
        public = {name for name in SignedTimeHandler.__dict__ if not name.startswith("_")}
        self.assertEqual(public, {"handle"})
        self.assertTrue(issubclass(HandlerError, RuntimeError))


def receipt_from_fixture():
    return handler_fixture()[-1].response


if __name__ == "__main__":
    unittest.main()
