import unittest
from collections.abc import Mapping

import path_bootstrap  # noqa: F401

from state_reader import DynamoStateReader, ReaderError
from state_reader_support import CONTRACT, FakeClient, fixture, metadata, read_result


class IntChild(int):
    pass


class StrChild(str):
    pass


class DictChild(dict):
    pass


class HostileMapping(Mapping):
    def __getitem__(self, key):
        raise RuntimeError("credential secret")

    def __iter__(self):
        raise RuntimeError("credential secret")

    def __len__(self):
        raise RuntimeError("credential secret")

    def get(self, key, default=None):
        raise RuntimeError("credential secret")


class StateReaderEnvelopeTests(unittest.TestCase):
    def assert_failed(self, result, calls=1):
        request = fixture()[0]
        client = FakeClient(result)
        with self.assertRaises(ReaderError) as raised:
            DynamoStateReader(client, CONTRACT).load_snapshot(request)
        self.assertEqual(str(raised.exception), "state reader operation failed")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)
        self.assertEqual(len(client.calls), calls)

    def test_missing_extra_and_swapped_responses_fail_closed(self):
        valid = read_result()["Responses"]
        cases = (
            [], valid[:1], valid[:2], valid + [valid[0]], list(reversed(valid)),
            [{}, valid[1], valid[2]],
            [{"Item": valid[0]["Item"], "Extra": {}}, valid[1], valid[2]],
            [valid[0], valid[1], {"item": valid[2]["Item"]}],
        )
        for responses in cases:
            with self.subTest(responses=len(responses)):
                self.assert_failed(read_result(responses=responses))

    def test_malformed_outer_and_metadata_envelopes_fail_closed(self):
        valid_responses = read_result()["Responses"]
        malformed = (
            None, [], object(), {}, HostileMapping(),
            {"Responses": valid_responses},
            {"Responses": valid_responses, "ResponseMetadata": HostileMapping()},
        )
        for result in malformed:
            with self.subTest(result=type(result).__name__):
                self.assert_failed(result)

        bad_metadata = (
            {},
            metadata(503), metadata(True), metadata(IntChild(200)),
            metadata(request_id=""), metadata(request_id=StrChild("id")),
            DictChild(metadata()),
        )
        for value in bad_metadata:
            with self.subTest(metadata=repr(value)):
                self.assert_failed(read_result(response_metadata=value))

    def test_provider_exception_is_sanitized_without_retry(self):
        request = fixture()[0]
        client = FakeClient(error=RuntimeError("provider credential secret"))
        with self.assertRaises(ReaderError) as raised:
            DynamoStateReader(client, CONTRACT).load_snapshot(request)

        self.assertEqual(str(raised.exception), "state reader operation failed")
        self.assertNotIn("secret", str(raised.exception))
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)
        self.assertEqual(len(client.calls), 1)

    def test_hostile_entry_mapping_is_sanitized(self):
        valid = read_result()["Responses"]
        self.assert_failed(
            read_result(responses=[HostileMapping(), valid[1], valid[2]]),
        )


if __name__ == "__main__":
    unittest.main()
