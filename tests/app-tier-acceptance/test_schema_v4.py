"""Adversarial and functional validation tests for schema v4 append-only governance."""

from __future__ import annotations

import copy
import datetime as dt
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from app_tier_acceptance import source, validator
from app_tier_acceptance.checks import canonical_digest
from fixtures import (
    append_correction_event,
    append_event,
    append_migration_event,
    append_resolution_event,
    refresh_event,
)

ROOT = source.ROOT
SEED_PATH = ROOT / "tests/app-tier-acceptance/fixture-data/app-tier-acceptance-seed.json"
SEED = json.loads(SEED_PATH.read_text())
NOW = dt.datetime(2026, 9, 3, tzinfo=dt.timezone.utc)

RAW_LOG = {
    "path": "docs/evidence/aarch64-semihosting-20260902-01-raw.txt",
    "sha256": "4e95514712074e077fa88c871c699aa7d8fcc039b26aa3f830f266e4b2275925",
    "size_bytes": 29571,
    "kind": "log",
}
RUNNER_LOG = {
    "path": "docs/evidence/aarch64-semihosting-20260902-01-runner.txt",
    "sha256": "6527744a11e110ec550ed15a83e970280f58b57fa3c187d1e4be44fa75e4016b",
    "size_bytes": 17032,
    "kind": "artifact",
}
VALID_APPROVAL = {
    "issue": 47,
    "decision": "YES",
    "approver": "distinct-governance-reviewer",
    "decision_recorded_at": "2026-09-02T12:00:00Z",
    "proposal_commit": "0e3b48f5a1b2c3d4e5f60718293a4b5c6d7e8f90",
    "evidence_urls": ["https://github.com/cellos/cellos/pull/47"],
}


class SchemaV4Tests(unittest.TestCase):
    """Test append-only schema v4 governance, transitions, and adversarial mutations."""

    def make_migrated(self) -> tuple[dict, dict]:
        """Produce trusted schema 3 seed and valid schema 4 migrated candidate."""
        seed = copy.deepcopy(SEED)
        migrated = copy.deepcopy(seed)
        migrated["schema_version"] = 4
        append_migration_event(migrated, seed, canonical_digest)
        return seed, migrated

    def make_corrected(self) -> tuple[dict, dict]:
        """Produce migrated baseline and valid record_correction candidate."""
        _, migrated = self.make_migrated()
        corrected = copy.deepcopy(migrated)
        corrected["subjects"].append({
            "id": "qemu-arm64",
            "environment": "qemu",
            "architecture": "aarch64",
            "board_revision": "",
            "firmware_digest": "",
            "host_vmm": "QEMU TCG",
        })
        for item in corrected["blockers"]:
            if item["id"] == "B-AARCH64-SEMHOSTING":
                item["subject"] = "qemu-arm64"
                item["scope"] = "AArch64 test-hooks semihosting execution and clean exit verified."
        append_correction_event(corrected, migrated, canonical_digest)
        return migrated, corrected

    def make_resolved(self) -> tuple[dict, dict]:
        """Produce corrected baseline and valid blocker_resolution candidate."""
        _, corrected = self.make_corrected()
        resolved = copy.deepcopy(corrected)
        event_id = "resolution-b-aarch64-semhosting"
        for item in resolved["blockers"]:
            if item["id"] == "B-AARCH64-SEMHOSTING":
                item["status"] = "PASS"
                item["resolution"] = {
                    "event_id": event_id,
                    "subject": "qemu-arm64",
                    "architecture": "aarch64",
                    "environment": "qemu",
                    "hardware": "QEMU TCG",
                    "firmware_sha256": "N/A",
                    "owner": "accountable-maintainer",
                    "runner": "independent-runner",
                    "command": "bash scripts/qemu-aarch64-test-hooks.sh",
                    "recorded_at": "2026-09-02T12:00:00Z",
                    "expires_at": "2026-09-04T12:00:00Z",
                    "ttl_seconds": 172800,
                    "artifacts": [RAW_LOG],
                }
        append_resolution_event(
            resolved,
            corrected,
            "B-AARCH64-SEMHOSTING",
            copy.deepcopy(VALID_APPROVAL),
            canonical_digest,
            recorded="2026-09-02T13:00:00Z",
            evidence=[RAW_LOG, RUNNER_LOG],
        )
        return corrected, resolved

    # --- 1. Backward Replay ---

    def test_backward_replay_schema_3(self) -> None:
        """Schema 3 snapshots replay unchanged and reject v4 actions."""
        self.assertEqual(validator.validate(SEED, NOW), "NOT_COMPLETE")

        # Schema 3 snapshot rejecting schema_migration
        bad = copy.deepcopy(SEED)
        append_migration_event(bad, SEED, canonical_digest)
        bad["schema_version"] = 3
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW)

        # Schema 3 snapshot rejecting record_correction
        bad_corr = copy.deepcopy(SEED)
        append_correction_event(bad_corr, SEED, canonical_digest)
        bad_corr["schema_version"] = 3
        refresh_event(bad_corr, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad_corr, NOW)

        # Schema 3 snapshot rejecting blocker_resolution
        bad_res = copy.deepcopy(SEED)
        append_resolution_event(bad_res, SEED, "B-AARCH64-SEMHOSTING", copy.deepcopy(VALID_APPROVAL), canonical_digest, evidence=[RAW_LOG])
        bad_res["schema_version"] = 3
        refresh_event(bad_res, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad_res, NOW)

    # --- 2. Schema Migration (3 -> 4) ---

    def test_schema_migration_happy_path(self) -> None:
        """Schema migration from 3 to 4 validates against trusted baseline."""
        seed, migrated = self.make_migrated()
        self.assertEqual(validator.validate(migrated, NOW, baseline=seed), "NOT_COMPLETE")

    def test_schema_migration_rejects_invalid_versions(self) -> None:
        """Migration must strictly transition from version 3 to 4."""
        seed, migrated = self.make_migrated()
        bad = copy.deepcopy(migrated)
        bad["events"][-1]["action"]["from_version"] = 2
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=seed)

        bad = copy.deepcopy(migrated)
        bad["events"][-1]["action"]["to_version"] = 5
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=seed)

    def test_schema_migration_rejects_lifecycle_drift(self) -> None:
        """Schema migration must not alter phase lifecycle."""
        seed, migrated = self.make_migrated()
        bad = copy.deepcopy(migrated)
        bad["phase_lifecycle"][1]["status"] = "IMPLEMENTED"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=seed)

    def test_schema_migration_rejects_unauthorized_section_changes(self) -> None:
        """Schema migration must not bundle changes to subjects, blockers, or claims."""
        seed, migrated = self.make_migrated()
        bad = copy.deepcopy(migrated)
        bad["subjects"].append({"id": "qemu-arm64", "environment": "qemu", "architecture": "aarch64", "board_revision": "", "firmware_digest": "", "host_vmm": "QEMU"})
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=seed)

    def test_schema_migration_rejects_wrong_change_digests(self) -> None:
        """Migration change digests must match canonical JSON of versions 3 and 4."""
        seed, migrated = self.make_migrated()
        bad = copy.deepcopy(migrated)
        bad["events"][-1]["action"]["changes"][0]["before_sha256"] = "0" * 64
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=seed)

    def test_schema_4_requires_migration_event(self) -> None:
        """Schema 4 snapshots require exactly one schema_migration event in history."""
        seed = copy.deepcopy(SEED)
        bad = copy.deepcopy(seed)
        bad["schema_version"] = 4
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW)

        # Two migration events
        _, migrated = self.make_migrated()
        dup = copy.deepcopy(migrated)
        append_migration_event(dup, migrated, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(dup, NOW)

    # --- 3. Record Correction ---

    def test_record_correction_happy_path(self) -> None:
        """Record correction updates subject and blocker scope while leaving status BLOCKED."""
        migrated, corrected = self.make_corrected()
        self.assertEqual(validator.validate(corrected, NOW, baseline=migrated), "NOT_COMPLETE")

    def test_record_correction_rejects_resolving_blocker(self) -> None:
        """Record correction must keep status BLOCKED and resolution null."""
        migrated, corrected = self.make_corrected()
        bad = copy.deepcopy(corrected)
        bad["blockers"][3]["status"] = "PASS"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

        bad = copy.deepcopy(corrected)
        bad["blockers"][3]["resolution"] = {"event_id": "dummy"}
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

    def test_record_correction_rejects_altering_existing_subjects(self) -> None:
        """Record correction cannot mutate or drop existing subjects."""
        migrated, corrected = self.make_corrected()
        bad = copy.deepcopy(corrected)
        bad["subjects"][0]["host_vmm"] = "TAMPERED"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

        bad = copy.deepcopy(corrected)
        bad["subjects"].pop(0)
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

    def test_record_correction_rejects_altering_blocker_id_or_evidence(self) -> None:
        """Record correction preserves blocker ID and historical evidence."""
        migrated, corrected = self.make_corrected()
        bad = copy.deepcopy(corrected)
        bad["blockers"][3]["id"] = "B-NEW-ID"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

        bad = copy.deepcopy(corrected)
        bad["blockers"][3]["evidence"] = []
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

    def test_record_correction_rejects_unauthorized_sections(self) -> None:
        """Record correction cannot touch claims, rows, lifecycle, or source_binding."""
        migrated, corrected = self.make_corrected()
        bad = copy.deepcopy(corrected)
        bad["claims"][0]["status"] = "PASS"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

        bad = copy.deepcopy(corrected)
        bad["phase_lifecycle"][1]["status"] = "IMPLEMENTED"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

    # --- 4. Blocker Resolution ---

    def test_blocker_resolution_happy_path(self) -> None:
        """Blocker resolution updates blocker to PASS with bound raw log and GitHub approval."""
        corrected, resolved = self.make_resolved()
        self.assertEqual(validator.validate(resolved, NOW, baseline=corrected), "NOT_COMPLETE")

    def test_blocker_resolution_rejects_uncorrected_blocker(self) -> None:
        """Cannot resolve a blocker against a mismatched or uncorrected subject."""
        # Attempting to resolve B-AARCH64-SEMHOSTING while its subject is still qemu-rv64
        _, migrated = self.make_migrated()
        bad = copy.deepcopy(migrated)
        for item in bad["blockers"]:
            if item["id"] == "B-AARCH64-SEMHOSTING":
                item["status"] = "PASS"
                item["resolution"] = {
                    "event_id": "resolution-b-aarch64-semhosting",
                    "subject": "qemu-rv64",
                    "architecture": "riscv64",
                    "environment": "qemu",
                    "hardware": "QEMU TCG",
                    "firmware_sha256": "N/A",
                    "owner": "accountable-maintainer",
                    "runner": "independent-runner",
                    "command": "bash scripts/qemu-aarch64-test-hooks.sh",
                    "recorded_at": "2026-09-02T12:00:00Z",
                    "expires_at": "2026-09-04T12:00:00Z",
                    "ttl_seconds": 172800,
                    "artifacts": [RAW_LOG],
                }
        append_resolution_event(bad, migrated, "B-AARCH64-SEMHOSTING", copy.deepcopy(VALID_APPROVAL), canonical_digest, recorded="2026-09-02T13:00:00Z", evidence=[RAW_LOG, RUNNER_LOG])
        # Resolution uses arm evidence for a riscv subject, or riscv architecture for an arm run -> must fail
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

    def test_blocker_resolution_rejects_wrong_architecture(self) -> None:
        """Resolution execution architecture must match the subject architecture."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        bad["blockers"][3]["resolution"]["architecture"] = "riscv64"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_blocker_resolution_rejects_wrong_subject(self) -> None:
        """Resolution subject must match the blocker subject."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        bad["blockers"][3]["resolution"]["subject"] = "qemu-rv64"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_blocker_resolution_rejects_blocker_id_mismatch(self) -> None:
        """Event action blocker_id must match the resolved blocker."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["blocker_id"] = "B-TIER2"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_blocker_resolution_rejects_missing_raw_log(self) -> None:
        """Blocker resolution evidence must contain at least one raw log."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["evidence"] = [RUNNER_LOG]  # only artifact, no log
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_blocker_resolution_rejects_reusing_historical_evidence(self) -> None:
        """Blocker resolution cannot reuse historical blocker evidence."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        hist_ev = resolved["blockers"][3]["evidence"][0]
        bad["events"][-1]["action"]["evidence"] = [RAW_LOG, hist_ev]
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_blocker_resolution_rejects_already_passing_blocker(self) -> None:
        """Cannot resolve a blocker that is already PASS in the baseline."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        append_resolution_event(bad, resolved, "B-AARCH64-SEMHOSTING", copy.deepcopy(VALID_APPROVAL), canonical_digest, recorded="2026-09-02T14:00:00Z", evidence=[RAW_LOG])
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=resolved)

    # --- 5. Bundling Rejection ---

    def test_bundling_rejection_correction_and_resolution(self) -> None:
        """Record correction cannot bundle blocker resolution."""
        migrated, corrected = self.make_corrected()
        bad = copy.deepcopy(corrected)
        bad["blockers"][3]["status"] = "PASS"
        bad["blockers"][3]["resolution"] = {
            "event_id": "correction-qemu-arm64",
            "subject": "qemu-arm64",
            "architecture": "aarch64",
            "environment": "qemu",
            "hardware": "QEMU TCG",
            "firmware_sha256": "N/A",
            "owner": "accountable-maintainer",
            "runner": "independent-runner",
            "command": "verify",
            "recorded_at": "2026-09-02T12:00:00Z",
            "expires_at": "2026-09-04T12:00:00Z",
            "ttl_seconds": 172800,
            "artifacts": [RAW_LOG],
        }
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

    def test_bundling_rejection_in_schema_4_lifecycle(self) -> None:
        """Schema 4 lifecycle transition cannot bundle correction or resolution."""
        _, migrated = self.make_migrated()
        bundled = copy.deepcopy(migrated)
        bundled["phase_lifecycle"][1]["status"] = "IMPLEMENTED"
        bundled["phase_lifecycle"][1]["event_id"] = "phase-2-implemented"
        bundled["subjects"].append({
            "id": "qemu-arm64",
            "environment": "qemu",
            "architecture": "aarch64",
            "board_revision": "",
            "firmware_digest": "",
            "host_vmm": "QEMU TCG",
        })
        events = bundled["events"]
        action = {
            "kind": "lifecycle_transition",
            "phase": 2,
            "from_status": "PLANNED",
            "to_status": "IMPLEMENTED",
            "changes": [
                {"section": "phase_lifecycle", "before_sha256": canonical_digest(migrated["phase_lifecycle"]), "after_sha256": canonical_digest(bundled["phase_lifecycle"])},
                {"section": "subjects", "before_sha256": canonical_digest(migrated["subjects"]), "after_sha256": canonical_digest(bundled["subjects"])},
            ],
            "evidence": [dict(migrated["events"][0]["action"]["evidence"][0])],
            "implementation": {
                "revision": "0" * 40,
                "base_tree": "0" * 40,
                "command": "cargo build --release",
                "target": "aarch64-unknown-none-softfloat",
                "result": "PASS",
                "artifact": dict(migrated["events"][0]["action"]["evidence"][0]),
            },
        }
        prior_time = dt.datetime.fromisoformat(events[-1]["recorded_at"].replace("Z", "+00:00"))
        recorded = (prior_time + dt.timedelta(seconds=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
        events.append({
            "sequence": len(events) + 1,
            "event_id": "phase-2-implemented",
            "previous_hash": events[-1]["hash"],
            "steward": "steward",
            "reviewer": "reviewer",
            "recorded_at": recorded,
            "action": action,
            "state_digest": "0" * 64,
            "hash": "0" * 64,
        })
        bundled["baseline_prefix"] = {"event_count": len(events), "tip_hash": "0" * 64}
        refresh_event(bundled, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bundled, NOW, baseline=migrated)

    # --- 6. GitHub Approval Binding ---

    def test_github_approval_decision_not_yes(self) -> None:
        """GitHub approval with decision != YES is rejected."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["github_approval"]["decision"] = "NO"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_github_approval_approver_must_match_reviewer_and_differ_from_steward(self) -> None:
        """Approver must match the event reviewer and be distinct from the accountable steward."""
        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["github_approval"]["approver"] = bad["events"][-1]["steward"]
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

        bad2 = copy.deepcopy(resolved)
        bad2["events"][-1]["action"]["github_approval"]["approver"] = "third-party"
        refresh_event(bad2, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad2, NOW, baseline=corrected)
    def test_github_approval_missing_fields(self) -> None:
        """GitHub approval missing required keys or invalid values is rejected."""
        corrected, resolved = self.make_resolved()

        # Missing proposal_commit
        bad = copy.deepcopy(resolved)
        del bad["events"][-1]["action"]["github_approval"]["proposal_commit"]
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

        # Invalid proposal_commit (not 40-hex)
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["github_approval"]["proposal_commit"] = "not-a-hash"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

        # Empty evidence URLs
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["github_approval"]["evidence_urls"] = []
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

        # Negative issue
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["github_approval"]["issue"] = -1
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_github_approval_timestamp_order(self) -> None:
        """Decision cannot be recorded after the event timestamp or in future."""
        corrected, resolved = self.make_resolved()

        # Decision recorded after event
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["github_approval"]["decision_recorded_at"] = "2026-09-02T14:00:00Z"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

        # Decision recorded in future relative to as_of
        bad = copy.deepcopy(resolved)
        bad["events"][-1]["action"]["github_approval"]["decision_recorded_at"] = "2026-09-10T00:00:00Z"
        bad["events"][-1]["recorded_at"] = "2026-09-10T01:00:00Z"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    # --- 7. Lifecycle Drift and Event Order ---

    def test_lifecycle_drift_rejection(self) -> None:
        """Governance actions must strictly keep lifecycle unchanged."""
        migrated, corrected = self.make_corrected()
        bad = copy.deepcopy(corrected)
        bad["phase_lifecycle"][1]["status"] = "IMPLEMENTED"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=migrated)

        corrected, resolved = self.make_resolved()
        bad = copy.deepcopy(resolved)
        bad["phase_lifecycle"][1]["status"] = "IMPLEMENTED"
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline=corrected)

    def test_v4_actions_cannot_precede_migration(self) -> None:
        """record_correction or blocker_resolution cannot appear before schema_migration."""
        seed = copy.deepcopy(SEED)
        bad = copy.deepcopy(seed)
        bad["schema_version"] = 4
        # Add correction event first, then migration event
        append_correction_event(bad, seed, canonical_digest)
        prior_state = copy.deepcopy(bad)
        append_migration_event(bad, prior_state, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW)


if __name__ == "__main__":
    unittest.main()
