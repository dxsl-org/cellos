import unittest

import path_bootstrap  # noqa: F401

from allocation import AdmittedSample
from handler import HandlerError, SignedTimeHandler
from handler_support import FAILURE, dependencies, handler_fixture, signed_copy
from protocol_models import MAX_UINT64, SignedResponse, UnsignedResponse
from receipt import Receipt
from state_reader import StateSnapshot


class BytesChild(bytes):
    pass

class IntChild(int):
    pass



class SnapshotChild(StateSnapshot):
    pass


class SampleChild(AdmittedSample):
    pass


class ReceiptChild(Receipt):
    pass


class UnsignedResponseChild(UnsignedResponse):
    pass


class SignedResponseChild(SignedResponse):
    pass


class HandlerFailureTests(unittest.TestCase):
    def service(self, reader, store, signer, loaders):
        return SignedTimeHandler(
            reader, store, signer, loaders.load_floor, loaders.load_sample
        )

    def assert_handler_error(self, operation):
        with self.assertRaises(HandlerError) as raised:
            operation()
        self.assertEqual(str(raised.exception), FAILURE)
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)

    def test_malformed_request_short_circuits_every_dependency(self):
        valid = handler_fixture()[0]
        malformed = (bytearray(valid), BytesChild(valid), b"\x00" * 1025, b"\x01")
        for data in malformed:
            with self.subTest(kind=type(data).__name__, length=len(data)):
                _, _, _, _, calls, reader, store, signer, loaders = dependencies()
                service = self.service(reader, store, signer, loaders)
                self.assert_handler_error(lambda: service.handle(data))
                self.assertEqual(calls, [])

    def test_registration_failure_stops_before_recovery_and_clock(self):
        data, _, _, _, calls, reader, store, signer, loaders = dependencies(
            reader_error=RuntimeError("revoked registration and secret key")
        )
        self.assert_handler_error(lambda: self.service(reader, store, signer, loaders).handle(data))
        self.assertEqual([call[0] for call in calls], ["snapshot"])
        self.assertEqual((store.recover_count, loaders.floor_count, signer.count), (0, 0, 0))

    def test_recovery_failure_stops_before_floor_sample_write_and_sign(self):
        data, _, _, _, calls, reader, store, signer, loaders = dependencies(
            recover_error=RuntimeError("database provider detail")
        )
        self.assert_handler_error(lambda: self.service(reader, store, signer, loaders).handle(data))
        self.assertEqual([call[0] for call in calls], ["snapshot", "recover"])
        self.assertEqual((loaders.floor_count, loaders.sample_count, store.commit_count, signer.count),
                         (0, 0, 0, 0))

    def test_floor_and_sample_failures_never_write_or_sign(self):
        cases = (
            ({"floor_error": RuntimeError("floor secret")}, ["snapshot", "recover", "floor"]),
            ({"sample_error": RuntimeError("clock secret")},
             ["snapshot", "recover", "floor", "sample"]),
        )
        for kwargs, expected in cases:
            with self.subTest(expected=expected):
                data, _, _, _, calls, reader, store, signer, loaders = dependencies(**kwargs)
                self.assert_handler_error(
                    lambda: self.service(reader, store, signer, loaders).handle(data)
                )
                self.assertEqual([call[0] for call in calls], expected)
                self.assertEqual((store.commit_count, signer.count), (0, 0))

    def test_commit_failure_does_not_sign_or_retry(self):
        data, _, _, _, calls, reader, store, signer, loaders = dependencies(
            commit_error=RuntimeError("ambiguous provider detail")
        )
        self.assert_handler_error(lambda: self.service(reader, store, signer, loaders).handle(data))
        self.assertEqual(
            [call[0] for call in calls],
            ["snapshot", "recover", "floor", "sample", "commit"],
        )
        self.assertEqual((store.recover_count, store.commit_count, signer.count), (1, 1, 0))

    def test_signer_failure_produces_no_bytes_and_is_not_retried(self):
        data, _, _, _, calls, reader, store, signer, loaders = dependencies(
            recovered=handler_fixture()[-1].response,
            sign_error=RuntimeError("KMS credential detail"),
        )
        self.assert_handler_error(lambda: self.service(reader, store, signer, loaders).handle(data))
        self.assertEqual([call[0] for call in calls], ["snapshot", "recover", "sign"])
        self.assertEqual(signer.count, 1)

    def test_invalid_protected_floors_stop_before_sample_commit_and_sign(self):
        for floor in (-1, MAX_UINT64 + 1, True, IntChild(0), 0.0):
            with self.subTest(floor=repr(floor), kind=type(floor).__name__):
                data, _, _, _, calls, reader, store, signer, loaders = dependencies()
                loaders.floor = floor
                self.assert_handler_error(
                    lambda: self.service(reader, store, signer, loaders).handle(data)
                )
                self.assertEqual(
                    [call[0] for call in calls], ["snapshot", "recover", "floor"]
                )
                self.assertEqual(
                    (loaders.sample_count, store.commit_count, signer.count), (0, 0, 0)
                )

    def test_exact_return_type_boundaries_reject_subclasses_and_objects(self):
        data, _, snapshot, receipt, _, _, _, _, _ = dependencies()
        cases = (
            ("snapshot", object()),
            ("snapshot", SnapshotChild(snapshot.registration, snapshot.state)),
            ("recovery", object()),
            ("recovery", UnsignedResponseChild(*receipt.response.__getstate__())),
            ("floor", object()),
            ("sample", object()),
            ("sample", SampleChild(*handler_fixture()[4].__getstate__())),
            ("receipt", object()),
            ("receipt", ReceiptChild(receipt.request_digest, receipt.response)),
            ("signed", object()),
            ("signed", SignedResponseChild(*signed_copy(receipt.response).__getstate__())),
        )
        for boundary, result in cases:
            with self.subTest(boundary=boundary):
                _, _, _, _, _, reader, store, signer, loaders = dependencies()
                if boundary == "snapshot":
                    reader.snapshot = result
                elif boundary == "recovery":
                    store.recovered = result
                elif boundary == "floor":
                    loaders.floor = result
                elif boundary == "sample":
                    loaders.sample = result
                elif boundary == "receipt":
                    store.receipt = result
                else:
                    store.recovered = receipt.response
                    signer.result = result
                self.assert_handler_error(
                    lambda: self.service(reader, store, signer, loaders).handle(data)
                )

    def test_constructor_rejects_missing_noncallable_and_hostile_dependencies(self):
        class NonCallableReader:
            load_snapshot = None

        class HostileStore:
            @property
            def recover_committed(self):
                raise RuntimeError("configuration secret")

            def commit_allocation(self, *args):
                raise AssertionError(args)

        _, _, _, _, _, reader, store, signer, loaders = dependencies()
        cases = (
            (object(), store, signer, loaders.load_floor, loaders.load_sample),
            (NonCallableReader(), store, signer, loaders.load_floor, loaders.load_sample),
            (reader, HostileStore(), signer, loaders.load_floor, loaders.load_sample),
            (reader, store, signer, None, loaders.load_sample),
            (reader, store, signer, loaders.load_floor, None),
        )
        for args in cases:
            with self.subTest(reader=type(args[0]).__name__):
                self.assert_handler_error(lambda: SignedTimeHandler(*args))


if __name__ == "__main__":
    unittest.main()
