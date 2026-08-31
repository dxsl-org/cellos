from dataclasses import replace
import unittest

import path_bootstrap
from lineage import LineageError, admit_lineage_contract, decode_transition, encode_transition
from lineage_test_support import (
    ALLOCATOR_TABLE_ID, LINEAGE_PUBLIC_DER, LINEAGE_TABLE, LINEAGE_TABLE_ID,
    contract, signed_transition,
)


class LineageTransitionTests(unittest.TestCase):
    def assert_rejected(self, transition, previous=None, **pins):
        values = {
            "lineage_table_name": LINEAGE_TABLE,
            "lineage_table_id": LINEAGE_TABLE_ID,
        }
        values.update(pins)
        with self.assertRaisesRegex(LineageError, "^invalid allocator lineage$"):
            admit_lineage_contract(
                values["lineage_table_name"], values["lineage_table_id"],
                encode_transition(transition), LINEAGE_PUBLIC_DER, previous,
            )

    def test_genesis_round_trips_and_authenticates(self):
        transition = signed_transition()
        encoded = encode_transition(transition)
        self.assertEqual(decode_transition(encoded), transition)
        admitted = contract(transition)
        self.assertEqual(admitted.encoded_transition, encoded)
        self.assertEqual(admitted.transition.allocator_table_id, ALLOCATOR_TABLE_ID)

    def test_direct_child_requires_exact_epoch_parent_and_new_response_key(self):
        genesis = contract()
        restored_table = "cellos-dev-signed-time-allocator-restored"
        restored_table_id = "88888888-2222-4333-8444-555555555555"
        child = signed_transition(
            epoch=2,
            parent_digest=genesis.transition_digest,
            table_name=restored_table,
            table_id=restored_table_id,
            response_key_id=genesis.transition.response_key_id + ":rotated",
            response_key_digest=bytes.fromhex("33" * 32),
            reason="restore",
        )
        self.assertEqual(contract(child).transition, child)
        admitted = contract(child, genesis)
        self.assertEqual(admitted.transition.source_epoch, 2)
        for changes in (
            {"epoch": 3},
            {"parent_digest": bytes.fromhex("44" * 32)},
            {"response_key_id": genesis.transition.response_key_id},
        ):
            with self.subTest(changes=changes):
                candidate = signed_transition(
                    epoch=changes.get("epoch", 2),
                    parent_digest=changes.get(
                        "parent_digest", genesis.transition_digest,
                    ),
                    table_name=restored_table,
                    table_id=restored_table_id,
                    response_key_id=changes.get(
                        "response_key_id",
                        genesis.transition.response_key_id + ":new",
                    ),
                    response_key_digest=bytes.fromhex("55" * 32),
                    reason="restore",
                )
                self.assert_rejected(candidate, genesis)
        same_table_restore = signed_transition(
            epoch=2,
            parent_digest=genesis.transition_digest,
            response_key_id=genesis.transition.response_key_id + ":new",
            reason="restore",
        )
        self.assert_rejected(same_table_restore, genesis)
        moved_rotation = signed_transition(
            epoch=2,
            parent_digest=genesis.transition_digest,
            table_name=restored_table,
            table_id=restored_table_id,
            response_key_id=genesis.transition.response_key_id + ":new",
            reason="key_rotation",
        )
        self.assert_rejected(moved_rotation, genesis)
        with self.assertRaises(LineageError):
            signed_transition(
                epoch=2,
                parent_digest=genesis.transition_digest,
                response_key_id=genesis.transition.response_key_id + ":new",
                reason="initialize",
            )

    def test_signature_covers_every_allocator_and_response_selection(self):
        original = signed_transition()
        for candidate in (
            replace(original, allocator_table_id="99999999-2222-4333-8444-555555555555"),
            replace(original, response_key_id=original.response_key_id + ":other"),
            replace(original, response_public_key_der_sha256=bytes.fromhex("66" * 32)),
        ):
            with self.subTest(candidate=candidate):
                self.assert_rejected(candidate)

    def test_lineage_root_cannot_change_between_transitions(self):
        genesis = contract()
        child = signed_transition(
            epoch=2,
            parent_digest=genesis.transition_digest,
            response_key_id=genesis.transition.response_key_id + ":new",
            reason="key_rotation",
        )
        self.assert_rejected(
            child, genesis,
            lineage_table_id="bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        )

    def test_malformed_and_noncanonical_encoding_fail_closed(self):
        encoded = encode_transition(signed_transition())
        for candidate in (b"", encoded + b"\x00", encoded[:-1]):
            with self.subTest(candidate=candidate):
                with self.assertRaises(LineageError):
                    admit_lineage_contract(
                        LINEAGE_TABLE, LINEAGE_TABLE_ID, candidate,
                        LINEAGE_PUBLIC_DER,
                    )


if __name__ == "__main__":
    unittest.main()
