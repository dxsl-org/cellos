import unittest

import path_bootstrap
from allocation import AllocationState
from lineage_migration import (
    allocator_epoch_migration_update, require_completed_epoch_migration,
)
from lineage_state import LineageStateError
from lineage_test_support import contract, signed_transition
from receipt import SOURCE_STATE_KEY
from state_codec import encode_allocation_state


class LineageMigrationTests(unittest.TestCase):
    def child_contract(self, parent):
        transition = signed_transition(
            epoch=2,
            parent_digest=parent.transition_digest,
            table_name="cellos-dev-signed-time-allocator-restored",
            table_id="88888888-2222-4333-8444-555555555555",
            response_key_id=parent.transition.response_key_id + ":new",
            reason="restore",
        )
        return contract(transition, parent)

    def test_restored_state_epoch_advances_by_exact_cas_without_floor_reset(self):
        parent = contract()
        child = self.child_contract(parent)
        restored = AllocationState(1, 42, 1_700_000_000)
        update = allocator_epoch_migration_update(parent, child, restored)["Update"]
        self.assertEqual(
            update["TableName"], child.transition.allocator_table_name,
        )
        self.assertEqual(update["Key"], {"pk": {"S": SOURCE_STATE_KEY}})
        self.assertEqual(update["UpdateExpression"], "SET #epoch = :child")
        self.assertEqual(
            update["ConditionExpression"],
            "#pk = :pk AND #sv = :sv AND #rt = :rt AND #epoch = :epoch AND "
            "#sequence = :sequence AND #time = :time",
        )
        values = update["ExpressionAttributeValues"]
        self.assertEqual(values[":epoch"], {"N": "1"})
        self.assertEqual(values[":child"], {"N": "2"})
        self.assertEqual(values[":sequence"], {"N": "42"})
        migrated = AllocationState(2, 42, 1_700_000_000)
        self.assertEqual(
            require_completed_epoch_migration(
                parent, child, restored, encode_allocation_state(migrated),
            ),
            migrated,
        )
        for rejected in (
            encode_allocation_state(restored),
            encode_allocation_state(AllocationState(2, 43, 1_700_000_000)),
            {},
        ):
            with self.assertRaises(LineageStateError):
                require_completed_epoch_migration(
                    parent, child, restored, rejected,
                )
        self.assertEqual(values[":time"], {"N": "1700000000"})

    def test_wrong_restored_epoch_and_invalid_edge_fail_before_update(self):
        parent = contract()
        child = self.child_contract(parent)
        for previous, selected, state in (
            (parent, child, AllocationState(2, 42, 1_700_000_000)),
            (child, parent, AllocationState(1, 42, 1_700_000_000)),
        ):
            with self.assertRaisesRegex(
                LineageStateError, "^invalid allocator lineage head$",
            ):
                allocator_epoch_migration_update(previous, selected, state)


if __name__ == "__main__":
    unittest.main()
