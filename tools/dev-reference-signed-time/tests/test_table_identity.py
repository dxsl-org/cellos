import unittest

import path_bootstrap
from lineage_test_support import contract
from table_identity import DynamoTableIdentityVerifier, TableIdentityError


class Client:
    def __init__(self, responses):
        self.responses = list(responses)
        self.calls = []

    def describe_table(self, **request):
        self.calls.append(request)
        response = self.responses.pop(0)
        if isinstance(response, Exception):
            raise response
        return response


def response(name, table_id, **changes):
    table = {
        "TableName": name,
        "TableId": table_id,
        "TableStatus": "ACTIVE",
        "DeletionProtectionEnabled": True,
    }
    table.update(changes)
    return {
        "Table": table,
        "ResponseMetadata": {"HTTPStatusCode": 200, "RequestId": "request-id"},
    }


class TableIdentityTests(unittest.TestCase):
    def test_both_exact_live_table_ids_are_verified_once(self):
        selected = contract()
        transition = selected.transition
        client = Client([
            response(transition.allocator_table_name, transition.allocator_table_id),
            response(selected.lineage_table_name, selected.lineage_table_id),
        ])
        self.assertIs(DynamoTableIdentityVerifier(client, selected).verify(), selected)
        self.assertEqual(
            client.calls,
            [
                {"TableName": transition.allocator_table_name},
                {"TableName": selected.lineage_table_name},
            ],
        )

    def test_recreated_same_name_with_new_table_id_fails_closed(self):
        selected = contract()
        transition = selected.transition
        client = Client([
            response(
                transition.allocator_table_name,
                "99999999-2222-4333-8444-555555555555",
            ),
        ])
        with self.assertRaisesRegex(
            TableIdentityError, "^table identity verification failed$",
        ):
            DynamoTableIdentityVerifier(client, selected).verify()
        self.assertEqual(len(client.calls), 1)

    def test_unavailable_or_inactive_tables_are_not_retried(self):
        selected = contract()
        transition = selected.transition
        failures = (
            RuntimeError("unavailable"),
            response(
                transition.allocator_table_name,
                transition.allocator_table_id,
                TableStatus="CREATING",
            ),
            response(
                transition.allocator_table_name,
                transition.allocator_table_id,
                DeletionProtectionEnabled=False,
            ),
            {"Table": {}, "ResponseMetadata": {"HTTPStatusCode": 200}},
        )
        for failure in failures:
            with self.subTest(failure=failure):
                client = Client([failure])
                with self.assertRaises(TableIdentityError):
                    DynamoTableIdentityVerifier(client, selected).verify()
                self.assertEqual(len(client.calls), 1)

    def test_invalid_dependency_has_stable_configuration_failure(self):
        with self.assertRaisesRegex(
            TableIdentityError,
            "^invalid table identity verifier configuration$",
        ):
            DynamoTableIdentityVerifier(object(), contract())


if __name__ == "__main__":
    unittest.main()
