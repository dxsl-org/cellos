from __future__ import annotations

import unittest

from relay_admission import (
    AdmissionError,
    AdmissionLease,
    AdmissionTable,
    AuthenticatedSessionIdentity,
)


class RelayAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.node_a = b"a" * 32
        self.node_b = b"b" * 32
        self.identity_a = AuthenticatedSessionIdentity(self.node_a)
        self.identity_b = AuthenticatedSessionIdentity(self.node_b)
        self.table: AdmissionTable[object] = AdmissionTable(1)

    def test_unauthenticated_and_mismatched_claims_do_not_mutate(self) -> None:
        session = object()
        self.assertEqual(
            self.table.admit(None, self.node_a, session),
            AdmissionError.UNAUTHENTICATED,
        )
        self.assertEqual(
            self.table.admit(self.identity_a, self.node_b, session),
            AdmissionError.IDENTITY_MISMATCH,
        )
        self.assertEqual(
            self.table.admit(
                AuthenticatedSessionIdentity(b"short"), b"short", session
            ),
            AdmissionError.IDENTITY_MISMATCH,
        )
        self.assertEqual(len(self.table), 0)

    def test_duplicate_live_and_capacity_rejections_preserve_original(self) -> None:
        original_session = object()
        lease = self.table.admit(self.identity_a, self.node_a, original_session)
        self.assertIsInstance(lease, AdmissionLease)
        assert isinstance(lease, AdmissionLease)
        generation = lease.generation

        self.assertEqual(
            self.table.admit(self.identity_a, self.node_a, object()),
            AdmissionError.DUPLICATE_LIVE,
        )
        self.assertEqual(
            self.table.admit(self.identity_b, self.node_b, object()),
            AdmissionError.CAPACITY,
        )
        self.assertEqual(len(self.table), 1)
        self.assertIs(self.table.lookup(self.node_a), lease)
        self.assertIs(self.table.current(self.node_a, generation).session, original_session)
        self.assertIsNone(self.table.lookup(self.node_b))

    def test_exact_release_allows_new_generation_and_stale_cleanup_is_rejected(self) -> None:
        first = self.table.admit(self.identity_a, self.node_a, object())
        assert isinstance(first, AdmissionLease)
        self.assertEqual(
            self.table.release(self.node_a, first.generation + 1),
            AdmissionError.STALE_DISCONNECT,
        )
        self.assertIs(self.table.lookup(self.node_a), first)
        self.assertIsNone(self.table.release(self.node_a, first.generation))
        self.assertEqual(len(self.table), 0)

        second = self.table.admit(self.identity_a, self.node_a, object())
        assert isinstance(second, AdmissionLease)
        self.assertGreater(second.generation, first.generation)
        self.assertEqual(
            self.table.release(self.node_a, first.generation),
            AdmissionError.STALE_DISCONNECT,
        )
        self.assertIs(self.table.lookup(self.node_a), second)

    def test_capacity_must_be_positive(self) -> None:
        with self.assertRaisesRegex(ValueError, "capacity must be positive"):
            AdmissionTable(0)


if __name__ == "__main__":
    unittest.main()
