"""Focused adversarial tests for baseline snapshots and lifecycle event actions."""

from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from app_tier_acceptance import validator
from app_tier_acceptance.checks import canonical_digest
from fixtures import append_event, refresh_event
import test_acceptance_ledger as acceptance


class HistoryLifecycleTests(unittest.TestCase):
    """A signed hash must not turn malformed history into trusted state."""

    def fixture(self):
        """Return a valid progressed candidate and independently valid prior snapshot."""
        return acceptance.LedgerTests("run").future()

    def reject(self, change, baseline_change=None) -> None:
        """Preserve hashes so each failure reaches the targeted invariant."""
        root, current, baseline, baseline_root = self.fixture()
        change(current)
        refresh_event(current, canonical_digest)
        if baseline_change:
            baseline_change(baseline)
            refresh_event(baseline, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(current, acceptance.NOW, baseline, root, baseline_root)

    def test_mutated_prior_c9_rejected(self) -> None:
        self.reject(lambda _: None, lambda item: item.update(c9="FULLY_QUALIFIED"))

    def test_mutated_prior_claim_rejected(self) -> None:
        self.reject(lambda _: None, lambda item: item["claims"][0].update(source_sha256="0" * 64))

    def test_mutated_prior_artifact_rejected(self) -> None:
        self.reject(lambda _: None, lambda item: item["blockers"][0]["evidence"][0].update(sha256="0" * 64))

    def test_duplicate_event_id_rejected(self) -> None:
        self.reject(lambda item: item["events"][-1].update(event_id=item["events"][0]["event_id"]))

    def test_free_string_action_rejected(self) -> None:
        self.reject(lambda item: item["events"][-1].update(action="advance"))

    def test_action_phase_mismatch_rejected(self) -> None:
        self.reject(lambda item: item["events"][-1]["action"].update(phase=3))

    def test_skipped_lifecycle_status_rejected(self) -> None:
        def change(item):
            item["phase_lifecycle"][1]["status"] = "VERIFIED"
            item["events"][-1]["action"].update(to_status="VERIFIED")
        self.reject(change)

    def test_unrelated_appended_event_rejected(self) -> None:
        def change(item):
            prior = copy.deepcopy(item)
            item["phase_lifecycle"][2]["status"] = "IMPLEMENTED"
            append_event(item, prior, 3, "IMPLEMENTED", canonical_digest)
        self.reject(change)
