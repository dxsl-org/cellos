import unittest

import path_bootstrap
from lineage import LINEAGE_HEAD_KEY
from lineage_state import (
    LineageStateError, encode_lineage_head, lineage_head_condition,
    lineage_head_get, lineage_transition_update, require_lineage_head,
)
from lineage_test_support import LINEAGE_TABLE, contract, signed_transition


class LineageStateTests(unittest.TestCase):
    def test_exact_head_round_trips(self):
        selected = contract()
        item = encode_lineage_head(selected)
        self.assertIsNone(require_lineage_head(item, selected))
        self.assertEqual(
            item,
            {
                "pk": {"S": LINEAGE_HEAD_KEY},
                "schema_version": {"N": "1"},
                "record_type": {"S": "lineage_head"},
                "transition": {"B": selected.encoded_transition},
            },
        )

    def test_substituted_restored_or_extended_heads_fail_closed(self):
        selected = contract()
        expected = encode_lineage_head(selected)
        candidates = (
            {},
            {**expected, "unexpected": {"S": "value"}},
            {**expected, "transition": {"B": b"restored-old-head"}},
            {**expected, "schema_version": {"N": "01"}},
        )
        for candidate in candidates:
            with self.subTest(candidate=candidate):
                with self.assertRaisesRegex(
                    LineageStateError, "^invalid allocator lineage head$",
                ):
                    require_lineage_head(candidate, selected)

    def test_transaction_read_and_condition_target_external_head(self):
        selected = contract()
        item = encode_lineage_head(selected)
        self.assertEqual(
            lineage_head_get(selected),
            {"Get": {
                "TableName": LINEAGE_TABLE,
                "Key": {"pk": {"S": LINEAGE_HEAD_KEY}},
            }},
        )
        condition = lineage_head_condition(selected)["ConditionCheck"]
        self.assertEqual(condition["TableName"], LINEAGE_TABLE)
        self.assertEqual(condition["Key"], {"pk": {"S": LINEAGE_HEAD_KEY}})
        self.assertEqual(
            condition["ExpressionAttributeValues"][":transition"],
            item["transition"],
        )
        self.assertNotIn("ReturnValuesOnConditionCheckFailure", condition)

    def test_transition_update_is_an_exact_parent_compare_and_swap(self):
        parent = contract()
        transition = signed_transition(
            epoch=2,
            parent_digest=parent.transition_digest,
            response_key_id=parent.transition.response_key_id + ":new",
            reason="key_rotation",
        )
        child = contract(transition, parent)
        update = lineage_transition_update(parent, child)["Update"]
        self.assertEqual(update["TableName"], LINEAGE_TABLE)
        self.assertEqual(update["UpdateExpression"], "SET #transition = :child")
        self.assertEqual(
            update["ConditionExpression"],
            "#pk = :pk AND #sv = :sv AND #rt = :rt AND #transition = :old",
        )
        values = update["ExpressionAttributeValues"]
        self.assertEqual(values[":old"], {"B": parent.encoded_transition})
        self.assertEqual(values[":child"], {"B": child.encoded_transition})
        unrotated = signed_transition(
            epoch=2,
            parent_digest=parent.transition_digest,
            response_key_id=parent.transition.response_key_id,
            reason="key_rotation",
        )
        with self.assertRaises(LineageStateError):
            lineage_transition_update(parent, contract(unrotated))
        with self.assertRaises(LineageStateError):
            lineage_transition_update(child, parent)


if __name__ == "__main__":
    unittest.main()
