import copy
import unittest
from dataclasses import replace

import path_bootstrap  # noqa: F401

from allocation import AllocationState
from state_codec import encode_allocation_state, encode_authority_registration
from state_reader import DynamoStateReader, ReaderError
from state_reader_support import EPOCH, TABLE, FakeClient, fixture, read_result


class DictChild(dict):
    pass


class StateReaderItemTests(unittest.TestCase):
    def assert_failed(self, result):
        request = fixture()[0]
        client = FakeClient(result)
        with self.assertRaises(ReaderError) as raised:
            DynamoStateReader(client, TABLE, EPOCH).load_snapshot(request)
        self.assertEqual(str(raised.exception), "state reader operation failed")
        self.assertIsNone(raised.exception.__cause__)
        self.assertIsNone(raised.exception.__context__)
        self.assertEqual(len(client.calls), 1)

    def test_malformed_items_and_attribute_values_fail_closed(self):
        valid = read_result()["Responses"]
        registration_item = valid[0]["Item"]
        state_item = valid[1]["Item"]
        malformed_registration = copy.deepcopy(registration_item)
        del malformed_registration["device_id"]
        extra_registration = copy.deepcopy(registration_item)
        extra_registration["extra"] = {"S": "x"}
        malformed_registration_av = copy.deepcopy(registration_item)
        malformed_registration_av["device_id"] = {"B": b"d" * 32, "S": "extra"}
        malformed_state_av = copy.deepcopy(state_item)
        malformed_state_av["source_epoch"] = {"N": "07"}
        cases = (
            [None, valid[1]],
            [{"Item": DictChild(registration_item)}, valid[1]],
            [{"Item": malformed_registration}, valid[1]],
            [{"Item": extra_registration}, valid[1]],
            [{"Item": malformed_registration_av}, valid[1]],
            [valid[0], {"Item": None}],
            [valid[0], {"Item": DictChild(state_item)}],
            [valid[0], {"Item": malformed_state_av}],
        )
        for responses in cases:
            with self.subTest(first=type(responses[0]).__name__):
                self.assert_failed(read_result(responses=responses))

    def test_revoked_and_every_substituted_registration_tuple_fail_closed(self):
        request, registration, state = fixture()
        alternate_key = request.authority_pubkey[:12] + b"x" * 32
        registrations = (
            replace(registration, revoked=True),
            replace(registration, device_id=b"x" * 32),
            replace(registration, authority_id=b"x" * 32),
            replace(registration, public_key_der=alternate_key),
        )
        for substituted in registrations:
            with self.subTest(substituted=substituted):
                self.assert_failed(read_result(substituted, state))

    def test_wrong_record_keys_fail_closed(self):
        responses = read_result()["Responses"]
        wrong_registration = copy.deepcopy(responses[0]["Item"])
        wrong_registration["pk"] = {"S": "authority#wrong/registration"}
        wrong_state = copy.deepcopy(responses[1]["Item"])
        wrong_state["pk"] = {"S": "source#wrong/state"}
        for changed in (
            [{"Item": wrong_registration}, responses[1]],
            [responses[0], {"Item": wrong_state}],
        ):
            self.assert_failed(read_result(responses=changed))

    def test_configured_source_epoch_must_match_decoded_state(self):
        request, registration, state = fixture()
        wrong_state = AllocationState(EPOCH + 1, state.source_sequence, state.last_unix_seconds)
        self.assert_failed(read_result(registration, wrong_state))

        valid = read_result()["Responses"]
        substituted = copy.deepcopy(valid[1]["Item"])
        substituted["source_epoch"] = {"N": str(EPOCH + 1)}
        self.assert_failed(read_result(responses=[valid[0], {"Item": substituted}]))


if __name__ == "__main__":
    unittest.main()
