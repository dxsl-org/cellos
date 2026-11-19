"""Adversarial tests for schema v5: source evidence binds archived revisions."""

from __future__ import annotations

import copy
import datetime as dt
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from app_tier_acceptance import checks, ledger, source, validator
from app_tier_acceptance.checks import HEX, canonical_digest
from fixtures import append_migration_event, append_migration_v5, refresh_event

SEED_PATH = ROOT / "tests/app-tier-acceptance/fixture-data/app-tier-acceptance-seed.json"
SEED = json.loads(SEED_PATH.read_text())
NOW = dt.datetime(2026, 9, 15, tzinfo=dt.timezone.utc)
AMENDED_SHA = SEED["source_binding"]["sha256"]
RATIFIED_SHA = "027e1a2bbfb19f55dec326a0588a459ff635f24f136ad0f6a971d67e93ca9e42"


RATIFIED_REVISION = "798e8b04e75b6ea787d7b5882da4f64359ed7974"


def ratified_spec_bytes() -> bytes:
    """Read the ratified contract revision out of history."""
    import subprocess

    return subprocess.run(
        ["git", "show", f"{RATIFIED_REVISION}:{source.SOURCE_PATH}"],
        cwd=ROOT,
        capture_output=True,
        check=True,
    ).stdout


def baseline_root(prior: dict) -> Path:
    """Materialize the file tree one historical snapshot was valid in.

    CI validates a trusted baseline in its own checkout; this mirrors that with the
    revision the prior state was written against (the contract was not yet amended).
    """
    import tempfile

    root = Path(tempfile.mkdtemp())
    for path, kind, digest in artifacts(prior):
        payload = ratified_spec_bytes() if path == source.SOURCE_PATH else (ROOT / path).read_bytes()
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)
        if kind == "source":
            preserved = checks.preserve(root, path, digest)
            preserved.parent.mkdir(parents=True, exist_ok=True)
            preserved.write_bytes(payload)
        if path == source.SOURCE_PATH:
            snapshot = root / source.snapshot_path(digest)
            snapshot.parent.mkdir(parents=True, exist_ok=True)
            snapshot.write_bytes(payload)
    return root


def artifacts(node) -> set[tuple[str, str, str]]:
    """Collect every artifact reference a snapshot carries."""
    found: set[tuple[str, str, str]] = set()
    if isinstance(node, dict):
        if isinstance(node.get("path"), str) and isinstance(node.get("sha256"), str):
            found.add((node["path"], node.get("kind", "artifact"), node["sha256"]))
        for value in node.values():
            found |= artifacts(value)
    elif isinstance(node, list):
        for value in node:
            found |= artifacts(value)
    return found


def migrated() -> tuple[dict, dict]:
    """Return a trusted schema 4 state and its schema 5 migration.

    The prior state binds the ratified contract revision; the migrated state binds
    the amended one, which is exactly the amendment the working tree carries.
    """
    seed = copy.deepcopy(SEED)
    v4 = copy.deepcopy(seed)
    v4["schema_version"] = 4
    v4["source_binding"]["sha256"] = RATIFIED_SHA
    v4["source_binding"]["matrix_sha256"] = source.matrix_digest_at(source.snapshot(digest=RATIFIED_SHA))
    append_migration_event(v4, seed, canonical_digest)
    v5 = copy.deepcopy(v4)
    v5["source_binding"]["sha256"] = AMENDED_SHA
    v5["source_binding"]["matrix_sha256"] = source.matrix_digest()
    append_migration_v5(v5, v4, canonical_digest)
    return v4, v5


PREVIOUS_SHA = "9265afc81b1537d7fec26636f1471f532c30c0d58f1f6bda6b1ce7e50cb22299"


def amended() -> tuple[dict, dict]:
    """Return a schema 5 state bound to an archived revision and its source_amendment.

    The amendment re-binds the live contract text; the matrix digest is unchanged.
    """
    _, v5 = migrated()
    prior = copy.deepcopy(v5)
    prior["source_binding"]["sha256"] = PREVIOUS_SHA
    refresh_event(prior, canonical_digest)
    current = copy.deepcopy(prior)
    current["source_binding"] = copy.deepcopy(v5["source_binding"])
    events = current["events"]
    recorded = dt.datetime.fromisoformat(events[-1]["recorded_at"].replace("Z", "+00:00")) + dt.timedelta(seconds=1)
    live = source.source_file().read_bytes()
    events.append({
        "sequence": len(events) + 1,
        "event_id": "source-amendment",
        "previous_hash": events[-1]["hash"],
        "steward": "steward",
        "reviewer": "reviewer",
        "recorded_at": recorded.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "action": {
            "kind": "source_amendment",
            "changes": [{
                "section": "source_binding",
                "before_sha256": canonical_digest(prior["source_binding"]),
                "after_sha256": canonical_digest(current["source_binding"]),
            }],
            "evidence": [{
                "path": source.snapshot_path(AMENDED_SHA),
                "sha256": AMENDED_SHA,
                "size_bytes": len(live),
                "kind": "artifact",
            }],
        },
        "state_digest": "0" * 64,
        "hash": "0" * 64,
    })
    current["baseline_prefix"] = {"event_count": len(events), "tip_hash": "0" * 64}
    refresh_event(current, canonical_digest)
    return prior, current


class SchemaV5Tests(unittest.TestCase):
    """Source revisions are amendable, so their evidence must be archived by digest."""

    def test_migration_happy_path(self) -> None:
        v4, v5 = migrated()
        root = baseline_root(v4)
        self.assertEqual(validator.validate(v5, NOW, v4, baseline_root=root), "NOT_COMPLETE")
        self.assertEqual(v5["schema_version"], 5)
        self.assertEqual(v5["events"][-1]["action"]["changes"][1]["section"], "source_binding")

    def test_amended_contract_still_validates_its_own_binding(self) -> None:
        """The binding names the live revision and must be archived itself."""
        _, v5 = migrated()
        self.assertEqual(v5["source_binding"]["sha256"], AMENDED_SHA)
        self.assertEqual(validator.validate(v5, NOW), "NOT_COMPLETE")

    def test_source_amendment_rebinds_the_contract_text(self) -> None:
        prior, current = amended()
        ledger.baseline(current, prior)
        self.assertEqual(validator.validate(current, NOW), "NOT_COMPLETE")

    def test_source_amendment_cannot_move_the_capability_matrix(self) -> None:
        prior, current = amended()
        current["source_binding"]["matrix_sha256"] = "ab" * 32
        current["events"][-1]["action"]["changes"][0]["after_sha256"] = canonical_digest(current["source_binding"])
        refresh_event(current, canonical_digest)
        with self.assertRaisesRegex(ValueError, "capability matrix"):
            ledger.baseline(current, prior)

    def test_source_amendment_cannot_carry_other_changes(self) -> None:
        prior, current = amended()
        current["claims"] = []
        refresh_event(current, canonical_digest)
        with self.assertRaisesRegex(ValueError, "unauthorized section"):
            ledger.baseline(current, prior)

    def test_archived_ratified_revision_still_validates(self) -> None:
        """The historical pins keep validating after the contract was amended."""
        seed = copy.deepcopy(SEED)
        pinned = seed["events"][0]["action"]["evidence"][0]
        self.assertEqual(pinned["sha256"], RATIFIED_SHA)
        self.assertNotEqual(pinned["sha256"], source.sha256_bytes(source.source_file().read_bytes()))
        self.assertEqual(validator.validate(seed, NOW), "NOT_COMPLETE")

    def test_missing_archived_revision_is_rejected(self) -> None:
        def mutate(data):
            data["events"][0]["action"]["evidence"][0]["sha256"] = "ab" * 32

        with self.assertRaises(ValueError):
            validator.validate(self.broken(mutate), NOW)

    def test_live_contract_path_cannot_be_used_with_a_stale_digest(self) -> None:
        """A pin whose digest moved on cannot fall back to the working tree."""
        seed = copy.deepcopy(SEED)
        pinned = seed["events"][0]["action"]["evidence"][0]
        pinned["sha256"] = source.sha256_bytes(b"amended contract")
        with self.assertRaises(ValueError):
            validator.validate(seed, NOW)

    def test_claim_requires_archived_revision(self) -> None:
        def mutate(data):
            data["claims"][0]["source_sha256"] = "cd" * 32

        with self.assertRaises(ValueError):
            validator.validate(self.broken(mutate), NOW)

    def test_claim_matrix_must_match_the_live_contract(self) -> None:
        def mutate(data):
            data["claims"][0]["matrix_sha256"] = "ef" * 32

        with self.assertRaises(ValueError):
            validator.validate(self.broken(mutate), NOW)

    def test_binding_requires_preserved_revision(self) -> None:
        def mutate(data):
            data["source_binding"]["sha256"] = source.sha256_bytes(source.source_file().read_bytes())
            data["source_binding"]["sha256"] = source.sha256_bytes(b"unarchived contract revision")

        with self.assertRaises(ValueError):
            validator.validate(self.broken(mutate), NOW)

    def test_migration_requires_both_steps(self) -> None:
        """Schema 5 without the 4 -> 5 migration is refused."""
        _, v5 = migrated()
        truncated = copy.deepcopy(v5)
        truncated["events"] = [event for event in truncated["events"] if event["event_id"] != "schema-migration-v4-to-v5"]
        truncated["baseline_prefix"] = {"event_count": len(truncated["events"]), "tip_hash": truncated["events"][-1]["hash"]}
        refresh_event(truncated, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(truncated, NOW)

    def test_migration_rejects_unauthorized_section(self) -> None:
        v4, v5 = migrated()
        v5["c9"] = "FULLY_QUALIFIED"
        refresh_event(v5, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(v5, NOW, v4, baseline_root=baseline_root(v4))
        self.assertEqual(HEX.fullmatch(v5["events"][-1]["hash"]) is not None, True)

    def test_migration_changes_must_match_the_state_delta(self) -> None:
        v4, v5 = migrated()
        v5["events"][-1]["action"]["changes"] = v5["events"][-1]["action"]["changes"][:1]
        refresh_event(v5, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(v5, NOW, v4, baseline_root=baseline_root(v4))

    def test_migration_cannot_rewrite_history(self) -> None:
        v4, v5 = migrated()
        v5["events"][0]["recorded_at"] = "2030-01-01T00:00:00Z"
        refresh_event(v5, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(v5, NOW, v4, baseline_root=baseline_root(v4))

    def test_migration_rejects_phase_lifecycle_drift(self) -> None:
        v4, v5 = migrated()
        v5["phase_lifecycle"][2]["status"] = "IMPLEMENTED"
        v5["phase_lifecycle"][2]["event_id"] = "phase-3-implemented"
        refresh_event(v5, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(v5, NOW, v4, baseline_root=baseline_root(v4))

    def broken(self, change) -> dict:
        data = copy.deepcopy(SEED)
        change(data)
        refresh_event(data, canonical_digest)
        return data


if __name__ == "__main__":
    unittest.main()
