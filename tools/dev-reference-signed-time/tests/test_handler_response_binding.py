import unittest
from dataclasses import replace

import path_bootstrap  # noqa: F401

from handler import HandlerError, SignedTimeHandler
from handler_support import FAILURE, dependencies, signed_copy
from protocol import encode_response
from protocol_models import UnsignedResponse
from receipt import Receipt


_BINDING_FIELDS = (
    "device_id",
    "authority_id",
    "boot_epoch",
    "request_id",
    "purpose",
    "nonce",
    "source_epoch",
)


def _different_bytes(value):
    return bytes((value[0] ^ 1,)) + value[1:]


def _mismatched_response(response, field):
    if field in ("device_id", "authority_id", "request_id", "nonce"):
        value = _different_bytes(getattr(response, field))
    elif field == "purpose":
        value = 2 if response.purpose != 2 else 1
    else:
        current = getattr(response, field)
        value = 1 if current == 0 else 0
    return replace(response, **{field: value})


class HandlerResponseBindingTests(unittest.TestCase):
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

    def test_recovery_rejects_each_request_or_source_epoch_mismatch_before_clock(self):
        for field in _BINDING_FIELDS:
            with self.subTest(field=field):
                data, _, _, receipt, calls, reader, store, signer, loaders = dependencies()
                response = _mismatched_response(receipt.response, field)
                self.assertIs(type(response), UnsignedResponse)
                store.recovered = response

                self.assert_handler_error(
                    lambda: self.service(reader, store, signer, loaders).handle(data)
                )

                self.assertEqual([call[0] for call in calls], ["snapshot", "recover"])
                self.assertEqual(
                    (
                        reader.count,
                        store.recover_count,
                        loaders.floor_count,
                        loaders.sample_count,
                        store.commit_count,
                        signer.count,
                    ),
                    (1, 1, 0, 0, 0, 0),
                )

    def test_fresh_commit_rejects_each_request_or_source_epoch_mismatch_before_signing(self):
        for field in _BINDING_FIELDS:
            with self.subTest(field=field):
                data, _, _, receipt, calls, reader, store, signer, loaders = dependencies()
                response = _mismatched_response(receipt.response, field)
                self.assertIs(type(response), UnsignedResponse)
                store.receipt = Receipt(receipt.request_digest, response)

                self.assert_handler_error(
                    lambda: self.service(reader, store, signer, loaders).handle(data)
                )

                self.assertEqual(
                    [call[0] for call in calls],
                    ["snapshot", "recover", "floor", "sample", "commit"],
                )
                self.assertEqual(
                    (
                        reader.count,
                        store.recover_count,
                        loaders.floor_count,
                        loaders.sample_count,
                        store.commit_count,
                        signer.count,
                    ),
                    (1, 1, 1, 1, 1, 0),
                )

    def test_matching_recovery_and_fresh_responses_keep_existing_behavior(self):
        for path in ("recovery", "fresh"):
            with self.subTest(path=path):
                data, _, _, receipt, calls, reader, store, signer, loaders = dependencies()
                if path == "recovery":
                    store.recovered = receipt.response
                    expected_calls = ["snapshot", "recover", "sign"]
                else:
                    expected_calls = [
                        "snapshot",
                        "recover",
                        "floor",
                        "sample",
                        "commit",
                        "sign",
                    ]

                encoded = self.service(reader, store, signer, loaders).handle(data)

                self.assertEqual(encoded, encode_response(signed_copy(receipt.response)))
                self.assertEqual([call[0] for call in calls], expected_calls)
                self.assertIs(signer.responses[0], receipt.response)
                self.assertEqual(signer.count, 1)


if __name__ == "__main__":
    unittest.main()
