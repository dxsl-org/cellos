"""Adversarial validation tests using real temporary Git repositories and files."""

from __future__ import annotations

import copy
import datetime as dt
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from app_tier_acceptance import public_api as sdk_source  # noqa: E402
from app_tier_acceptance import source, validator  # noqa: E402
from app_tier_acceptance.checks import canonical_digest  # noqa: E402
from fixtures import append_event, claim, cohort, refresh_event  # noqa: E402

NOW = dt.datetime(2026, 8, 21, 12, tzinfo=dt.timezone.utc)
SEED = json.loads((ROOT / "docs/app-tier-acceptance-ledger.json").read_text())


def digest(path: Path) -> str:
    """Hash a real artifact file."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def artifact(root: Path, path: str, kind: str = "artifact") -> dict:
    """Describe bytes that the validator will read back from disk."""
    item = root / path
    return {"path": path, "sha256": digest(item), "size_bytes": item.stat().st_size, "kind": kind}


class LedgerTests(unittest.TestCase):
    """Every confirmed promotion and evidence mutation must fail closed."""

    def reject(self, change) -> None:
        data = copy.deepcopy(SEED)
        change(data)
        with self.assertRaises(ValueError):
            validator.validate(data, NOW)

    def test_seed_is_truthful_and_exact(self) -> None:
        self.assertEqual(validator.validate(SEED, NOW), "NOT_COMPLETE")
        cells = [cell for row in SEED["rows"] for cell in row["cells"]]
        self.assertEqual((len(cells), sum(cell["required_for_c9"] for cell in cells)), (60, 44))

    def test_schema_source_history_and_negative_mutations_fail(self) -> None:
        self.reject(lambda item: item.update(extra=True))
        self.reject(lambda item: item["rows"][0]["cells"][0].update(source_text="**USABLE**"))
        self.reject(lambda item: item["events"][0].update(action="rewritten"))
        self.reject(lambda item: item["security_negatives"][0].update(observed="allow"))
        self.reject(lambda item: item["blockers"][0].update(scope="rewritten"))
        self.reject(lambda item: item["claims"][0]["tuple"].update(cpu="riscv64"))

    def test_nonempty_raw_security_and_blocker_evidence_are_mandatory(self) -> None:
        self.reject(lambda item: item["security_negatives"][0].update(evidence=[]))
        self.reject(lambda item: item["blockers"][0].update(evidence=[]))

    def test_claim_completion_and_status_are_coupled(self) -> None:
        self.reject(lambda item: item["claims"][0].update(status="PASS"))
        self.reject(lambda item: item["claims"][0].update(completion="COMPLETE"))

    def test_cell_evidence_cannot_be_added_before_source_is_usable(self) -> None:
        self.reject(lambda item: item["rows"][0]["cells"][0].update(evidence=[{}]))

    def test_history_state_digest_cannot_be_replayed_after_state_change(self) -> None:
        self.reject(lambda item: item["phase_lifecycle"][1].update(status="IMPLEMENTED"))

    def test_promotion_requires_external_baseline_and_ratified_source(self) -> None:
        root, data, baseline, baseline_root = self.future()
        with self.assertRaises(ValueError):
            validator.validate(data, NOW, root=root)
        data["source_binding"]["ratified_revision"] = ""
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, NOW, baseline, root, baseline_root)

    def test_dirty_bundle_witness_mutations_fail(self) -> None:
        root, data, baseline, baseline_root = self.future()
        bad = copy.deepcopy(data)
        bad["rows"][0]["cells"][0]["evidence"][0]["dirty_bundle"]["patch"]["sha256"] = "0" * 64
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline, root, baseline_root)
        bad = copy.deepcopy(data)
        bad["rows"][0]["cells"][0]["evidence"][0]["witnesses"].pop()
        refresh_event(bad, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(bad, NOW, baseline, root, baseline_root)

    def test_stepwise_future_chain_reaches_fully_qualified(self) -> None:
        root, current, baseline, baseline_root = self.future()
        self.assertEqual(validator.validate(current, NOW, baseline, root, baseline_root), "NOT_COMPLETE")
        prior = copy.deepcopy(current)
        for phase in range(2, 9):
            states = ("VERIFIED", "LEDGER_RECORDED") if phase == 2 else ("IMPLEMENTED", "VERIFIED", "LEDGER_RECORDED")
            for status in states:
                current["phase_lifecycle"][phase - 1]["status"] = status
                append_event(current, prior, phase, status, canonical_digest)
                self.assertEqual(validator.validate(current, NOW, prior, root, root), "NOT_COMPLETE")
                prior = copy.deepcopy(current)

    def future(self, full_matrix: bool = False) -> tuple[Path, dict, dict, Path]:
        """Create a committed amended source plus a content-verified dirty evidence bundle."""
        root, baseline_root = Path(tempfile.mkdtemp()), Path(tempfile.mkdtemp())
        baseline_spec = baseline_root / source.SOURCE_PATH
        baseline_spec.parent.mkdir(parents=True)
        baseline_spec.write_bytes((ROOT / source.SOURCE_PATH).read_bytes())
        (baseline_root / "evidence").mkdir()
        (baseline_root / "evidence/baseline.log").write_text("baseline evidence\n")
        spec = root / source.SOURCE_PATH
        spec.parent.mkdir(parents=True)
        lines = (ROOT / source.SOURCE_PATH).read_text().splitlines()
        promoted = False
        for index, line in enumerate(lines):
            if line.startswith("| C2-"):
                fields = [value.strip() for value in line.strip().strip("|").split("|")]
                for cell in range(1, 7):
                    axis = source.CELL_AXES[cell - 1]
                    eligible = axis in {"rust-no-std", "T1"}
                    if eligible and (full_matrix or not promoted):
                        fields[cell] = "**USABLE**"
                        promoted = True
                lines[index] = "| " + " | ".join(fields) + " |"
        spec.write_text("\n".join(lines) + "\n")
        for public_path in sdk_source.paths(ROOT):
            target = root / public_path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((ROOT / public_path).read_bytes())
        (root / "evidence").mkdir()
        (root / "evidence/trace.log").write_text("clean\n")
        (root / "evidence/baseline.log").write_text("baseline evidence\n")
        self.git(root, "init")
        self.git(root, "add", ".")
        self.git(root, "-c", "user.email=a@b", "-c", "user.name=test", "commit", "-m", "seed")
        revision = self.git(root, "rev-parse", "HEAD").strip()
        tree = self.git(root, "rev-parse", "HEAD^{tree}").strip()
        (root / "evidence/trace.log").write_text("dirty trace\n")
        (root / "evidence/untracked.bin").write_bytes(b"untracked\x00bytes")
        (root / "evidence/patch.diff").write_bytes(subprocess.run(["git", "diff", "--binary", revision], cwd=root, capture_output=True).stdout)
        data, baseline = copy.deepcopy(SEED), copy.deepcopy(SEED)
        data["subjects"].extend([
            {"id": "qemu-arm64", "environment": "qemu", "architecture": "aarch64", "board_revision": "", "firmware_digest": "", "host_vmm": "QEMU TCG"},
            {"id": "qemu-x86", "environment": "qemu", "architecture": "x86_64", "board_revision": "", "firmware_digest": "", "host_vmm": "QEMU TCG"},
        ])
        binding = data["source_binding"]
        binding.update(sha256=digest(spec), matrix_sha256=source.matrix_digest(root), ratified_revision=revision)
        raw_artifact = artifact(root, "evidence/trace.log", "log")
        baseline_artifact = artifact(root, "evidence/baseline.log")
        for item in data["blockers"]:
            item["evidence"] = [baseline_artifact]
            item.update(status="PASS", resolution={"event_id": "phase-2-implemented", "artifacts": [raw_artifact]})
        for item in baseline["blockers"]:
            item["evidence"] = [baseline_artifact]
        baseline["security_negatives"][0]["evidence"] = [baseline_artifact]
        baseline["events"][0]["action"]["evidence"] = [baseline_artifact]
        data["events"][0]["action"]["evidence"] = [baseline_artifact]
        refresh_event(baseline, canonical_digest)
        data["events"], data["baseline_prefix"] = copy.deepcopy(baseline["events"]), copy.deepcopy(baseline["baseline_prefix"])
        negative_witness = {
            "owner": "security-owner", "runner": "independent-runner",
            "command": "run-hostile-denial", "test_name": "native-domain-hostile-tests",
            "target": "aarch64-unknown-none-softfloat", "architecture": "aarch64",
            "environment": "kvm", "hardware": "ARM64 KVM", "firmware_sha256": "N/A",
            "expected": "deny", "observed": "deny", "recorded_at": "2026-08-20T00:00:00Z",
            "expires_at": "2026-08-22T00:00:00Z", "ttl_seconds": 172800,
            "artifacts": [raw_artifact], "event_id": "phase-2-implemented",
        }
        for negative in data["security_negatives"]:
            negative.update(status="PASS", observed="deny", evidence=[raw_artifact], witness=copy.deepcopy(negative_witness))
        data["claims"] = []
        claims_by_tuple = {}
        for row, imported in zip(data["rows"], source.matrix(root)):
            for cell, raw in zip(row["cells"], imported[1]):
                available = source.availability(raw)
                cell.update(source_text=raw, source_availability=available, required_for_c9=available not in {"N/A", "UNSUPPORTED"})
                if available == "USABLE" and source.applicability(row["id"], cell["id"])["build_denominators"]:
                    evidence = []
                    for denominator in source.applicability(row["id"], cell["id"])["build_denominators"]:
                        _, target, _, _, _, selection, _, _, _, _ = denominator.split("|")
                        cpu = "riscv64" if target.startswith("riscv64") else "aarch64" if target.startswith("aarch64") else "x86_64"
                        claim_value = claim(row["id"], cell["id"], binding, cpu)
                        key = (claim_value["subject"], tuple(claim_value["tuple"].values()))
                        claim_value = claims_by_tuple.setdefault(key, claim_value)
                        if claim_value not in data["claims"]:
                            data["claims"].append(claim_value)
                        evidence.append(cohort(root, claim_value, revision, tree, raw_artifact, canonical_digest, target, selection))
                    cell.update(status="PASS", evidence=evidence)
                else:
                    cell.update(status="PLANNED" if available == "PLANNED" else "BLOCKED", evidence=[])
            required = [cell for cell in row["cells"] if cell["required_for_c9"]]
            row["aggregate"] = "PASS" if required and all(cell["status"] == "PASS" for cell in required) else "BLOCKED"
        data["phase_lifecycle"][1]["status"] = "IMPLEMENTED"
        append_event(data, baseline, 2, "IMPLEMENTED", canonical_digest)
        refresh_event(data, canonical_digest)
        return root, data, baseline, baseline_root

    @staticmethod
    def git(root: Path, *args: str) -> str:
        """Run a local disposable-repository command and return its output."""
        return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, text=True).stdout
