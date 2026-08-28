"""Regression tests for production-review qualification bypasses."""

from __future__ import annotations

import copy
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from app_tier_acceptance import baseline_ref, public_api, source, validator
from app_tier_acceptance.checks import canonical_digest
from fixtures import append_event, refresh_event
import test_acceptance_ledger as acceptance


class ReviewRegressionTests(unittest.TestCase):
    """Qualification must remain closed under each reviewed adversarial input."""

    def fixture(self, full_matrix: bool = True):
        """Return the shared real-Git future fixture."""
        return acceptance.LedgerTests("run").future(full_matrix)

    def test_public_api_resolves_explicit_module_paths(self) -> None:
        root = Path(__file__).resolve().parents[2]
        self.assertIn("libs/viui/src/managed_surface_tests.rs", public_api.paths(root))

    def test_clean_cohort_requires_resolvable_git_identity(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        evidence = data["rows"][0]["cells"][0]["evidence"][0]
        evidence.update(dirty=False, dirty_bundle=None, revision="f" * 40, base_tree="e" * 40)
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_witness_semantics_are_required(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        data["rows"][0]["cells"][0]["evidence"][0]["witnesses"][0]["command"] = ""
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_witness_denominator_and_runner_must_agree(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        witness = data["rows"][0]["cells"][0]["evidence"][0]["witnesses"][1]
        witness["details"]["target"] = "x86_64-unknown-none"
        witness["runner"] = witness["owner"]
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_contract_document_cannot_replace_public_sdk_sources(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        (root / "libs/ostd/src/lib.rs").unlink()
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)
        root, data, baseline, baseline_root = self.fixture()
        (root / "libs/viui/src/lib.rs").unlink()
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)
        root, data, baseline, baseline_root = self.fixture()
        (root / "libs/viui-macros/src/lib.rs").unlink()
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_blocker_resolution_requires_related_event(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        data["blockers"][0]["resolution"]["event_id"] = "missing"
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_security_negative_requires_typed_witness(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        data["security_negatives"][0]["witness"]["command"] = ""
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_complete_tuple_enums_are_required(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        data["claims"][0]["tuple"]["ipc"] = "anything"
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_event_must_bind_every_state_delta(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        data["subjects"][0]["host_vmm"] = "rewritten"
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_one_usable_cell_cannot_qualify_a_blocked_matrix(self) -> None:
        root, current, baseline, baseline_root = self.fixture(full_matrix=False)
        self.assertEqual(validator.validate(current, acceptance.NOW, baseline, root, baseline_root), "NOT_COMPLETE")
        prior = copy.deepcopy(current)
        for phase in range(2, 9):
            states = ("VERIFIED", "LEDGER_RECORDED") if phase == 2 else ("IMPLEMENTED", "VERIFIED", "LEDGER_RECORDED")
            for status in states:
                current["phase_lifecycle"][phase - 1]["status"] = status
                append_event(current, prior, phase, status, canonical_digest)
                self.assertEqual(validator.validate(current, acceptance.NOW, prior, root, root), "NOT_COMPLETE")
                prior = copy.deepcopy(current)

    def test_seed_rejects_missing_spec22_case_and_applicability_drift(self) -> None:
        data = copy.deepcopy(acceptance.SEED)
        data["security_negatives"].pop()
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW)

    def test_spec22_records_must_be_unique_even_when_blocked_or_passing(self) -> None:
        for status in ("BLOCKED", "PASS"):
            data = copy.deepcopy(acceptance.SEED)
            duplicate = copy.deepcopy(data["security_negatives"][0])
            duplicate["status"] = status
            data["security_negatives"].append(duplicate)
            refresh_event(data, canonical_digest)
            with self.assertRaises(ValueError):
                validator.validate(data, acceptance.NOW)

    def test_dispatch_baseline_uses_latest_ledger_transition_parent(self) -> None:
        root = Path(tempfile.mkdtemp())
        self.git(root, "init")
        self.commit(root, "docs/app-tier-acceptance-ledger.json", "seed", "seed")
        seed = self.git(root, "rev-parse", "HEAD").strip()
        self.commit(root, "docs/app-tier-acceptance-ledger.json", "transition", "transition")
        transition = self.git(root, "rev-parse", "HEAD").strip()
        self.commit(root, "README", "unrelated one", "unrelated-one")
        self.commit(root, "notes", "unrelated two", "unrelated-two")
        previous = Path.cwd()
        try:
            os.chdir(root)
            self.assertEqual(baseline_ref.dispatch_baseline("main", "main"), seed)
            self.assertEqual(baseline_ref.trusted_snapshot("HEAD"), transition)
        finally:
            os.chdir(previous)

    def test_supplied_identical_seed_validates_and_rewrites_reject(self) -> None:
        seed = copy.deepcopy(acceptance.SEED)
        self.assertEqual(validator.validate(seed, acceptance.NOW, copy.deepcopy(seed)), "NOT_COMPLETE")
        rewritten = copy.deepcopy(seed)
        rewritten["blockers"][0]["scope"] = "rewritten"
        refresh_event(rewritten, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(rewritten, acceptance.NOW, seed)

    def test_matrix_cell_identity_is_ordered_and_complete(self) -> None:
        for change in (
            lambda cells: cells.__setitem__(0, dict(cells[0], id="C2-FDN/renamed")),
            lambda cells: cells.reverse(),
            lambda cells: cells.pop(),
        ):
            data = copy.deepcopy(acceptance.SEED)
            change(data["rows"][0]["cells"])
            refresh_event(data, canonical_digest)
            with self.assertRaises(ValueError):
                validator.validate(data, acceptance.NOW)

    def test_canonical_denominator_rejects_every_field_drift(self) -> None:
        root, data, baseline, baseline_root = self.fixture()
        for field in ("compiler", "target", "language", "cfg", "rustflags", "feature_selection", "cargo_features", "cargo_profile", "runtime_profile"):
            changed = copy.deepcopy(data)
            cohort = changed["rows"][0]["cells"][0]["evidence"][0]
            cohort["denominator"][field] = "drift"
            compile_witness = next(item for item in cohort["witnesses"] if item["class"] == "compile")
            compile_witness["details"][field] = "drift"
            refresh_event(changed, canonical_digest)
            with self.assertRaises(ValueError):
                validator.validate(changed, acceptance.NOW, baseline, root, baseline_root)
        changed = copy.deepcopy(data)
        cohort = changed["rows"][0]["cells"][0]["evidence"][0]
        claim_value = next(item for item in changed["claims"] if item["id"] == cohort["claim_id"])
        claim_value["tuple"]["tier"] = "T2"
        cohort["tuple"]["tier"] = "T2"
        next(item for item in cohort["witnesses"] if item["class"] == "tier")["details"]["tier"] = "T2"
        refresh_event(changed, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(changed, acceptance.NOW, baseline, root, baseline_root)
        self.assertEqual(len(set(source.applicability("C2-FDN", "C2-FDN/rust-no-std")["build_denominators"])), 96)

    @staticmethod
    def git(root: Path, *args: str) -> str:
        return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, text=True).stdout

    def commit(self, root: Path, path: str, body: str, message: str) -> None:
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(body)
        self.git(root, "add", path)
        self.git(root, "-c", "user.email=test@example.invalid", "-c", "user.name=test", "commit", "-m", message)
        data = copy.deepcopy(acceptance.SEED)
        data["rows"][0]["cells"][0]["applicability"] = {"architectures": ["x86_64"], "environments": ["kvm"]}
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW)

    def test_events_must_increase_and_pass_claims_must_project(self) -> None:
        root, current, baseline, baseline_root = self.fixture()
        current["events"][-1]["recorded_at"] = baseline["events"][-1]["recorded_at"]
        refresh_event(current, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(current, acceptance.NOW, baseline, root, baseline_root)
        root, current, baseline, baseline_root = self.fixture()
        current["claims"][0]["id"] = "unreferenced-pass"
        refresh_event(current, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(current, acceptance.NOW, baseline, root, baseline_root)
