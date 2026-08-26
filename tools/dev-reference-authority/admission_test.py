"""Tests for the Phase 1 DEV_REFERENCE admission validator."""
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from admission import BLOCKED, evaluate
from admission_test_support import EVIDENCE_FILES, asset, base_inventory

class AdmissionTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._temp = tempfile.TemporaryDirectory()
        cls.evidence_dir = Path(cls._temp.name)
        for name, data in EVIDENCE_FILES.items():
            (cls.evidence_dir / name).write_bytes(data)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._temp.cleanup()


    def run_case(self, mutate) -> list:
        """Evaluate a mutation and prove it adds a failure beyond the known AWS blocker."""
        _, baseline = self.evaluate_inventory(base_inventory())
        inventory = base_inventory()
        mutate(inventory)
        status, report = self.evaluate_inventory(inventory)
        self.assertEqual(status, BLOCKED)
        baseline_failures = [c for c in baseline["checks"] if c["result"] == "fail"]
        failures = [c for c in report["checks"] if c["result"] == "fail"]
        self.assertNotEqual(failures, baseline_failures)
        return failures

    def evaluate_inventory(self, inventory: dict):
        path = self.evidence_dir / "_case-inventory.json"
        path.write_text(json.dumps(inventory), encoding="utf-8")
        try:
            return evaluate(path, self.evidence_dir)
        finally:
            path.unlink()

    def test_complete_authorized_evidence_remains_blocked_on_read_only_proof(self) -> None:
        status, report = self.evaluate_inventory(base_inventory())
        self.assertEqual(status, BLOCKED)
        failure = next(c for c in report["checks"] if c["id"] == "aws-read-only-identity")
        self.assertIn("cannot be proven", failure["detail"])

    def test_cli_end_to_end_and_determinism(self) -> None:
        fixture = self.evidence_dir / "fixture.json"
        fixture.write_text(json.dumps(base_inventory()), encoding="utf-8")
        tool = Path(__file__).with_name("admission.py")
        outputs = []
        for expected_rc in (1, 1):
            proc = subprocess.run(
                [sys.executable, str(tool), "validate",
                 "--inventory", str(fixture), "--evidence-dir", str(self.evidence_dir)],
                capture_output=True, text=True)
            self.assertEqual(proc.returncode, expected_rc, proc.stdout + proc.stderr)
            parsed = json.loads(proc.stdout)
            self.assertEqual(parsed["status"], BLOCKED)
            self.assertEqual(parsed["classification"], "DEV_REFERENCE")
            outputs.append(proc.stdout)
        self.assertEqual(outputs[0], outputs[1])
        blocked = copy.deepcopy(base_inventory())
        blocked["actions"]["purchase"] = "authorized"
        fixture.write_text(json.dumps(blocked), encoding="utf-8")
        proc = subprocess.run(
            [sys.executable, str(tool), "validate",
             "--inventory", str(fixture), "--evidence-dir", str(self.evidence_dir)],
            capture_output=True, text=True)
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(json.loads(proc.stdout)["status"], BLOCKED)
        fixture.unlink()

    def test_rejection_classes_return_blocked(self) -> None:
        def vf2_alias(inv):
            inv["assets"][0]["exact_id"] = "StarFive VisionFive 2 (VF2)"

        def alt_revision(inv):
            inv["assets"][0]["exact_id"] = "StarFive VisionFive 2 v1.2A"
            inv["assets"][0]["revision"] = "v1.2A"
            inv["assets"][0]["model_revision"] = "v1.2A"

        def alt_opn(inv):
            inv["assets"][2]["opn"] = "TPM9672FW1523PCEBTOBO2"

        def blank_serial(inv):
            inv["assets"][1]["serial_or_asset_id"] = ""

        def duplicate_asset(inv):
            inv["assets"].append(copy.deepcopy(inv["assets"][1]))

        def missing_asset(inv):
            del inv["assets"][0]

        def duplicate_hash(inv):
            inv["assets"][1]["attachment_hashes"].append(dict(inv["assets"][0]["attachment_hashes"][0]))

        def wrong_evidence_hash(inv):
            inv["assets"][0]["attachment_hashes"][0]["sha256"] = "cd" * 32

        def ordered_status(inv):
            inv["assets"][0]["presence_status"] = "ordered"

        def expected_status(inv):
            inv["assets"][0]["presence_status"] = "expected"

        def shared_aws(inv):
            inv["aws_dev_account"]["classification"] = "shared-with-production"

        def bad_account_id(inv):
            inv["aws_dev_account"]["account_id"] = "12345"

        def missing_aws_identity(inv):
            del inv["aws_dev_account"]["identity_evidence"]

        def substituted_aws_identity(inv):
            inv["aws_dev_account"]["account_id"] = "210987654321"

        def substituted_aws_region(inv):
            inv["aws_dev_account"]["region"] = "us-east-1"

        def no_time_source_row(inv):
            del inv["upstream_time_sources"]

        def unpinned_time_source(inv):
            inv["upstream_time_sources"][0]["pinned"] = False

        def missing_auth_pin(inv):
            del inv["upstream_time_sources"][0]["auth_pin"]

        def weak_pin_value(inv):
            inv["upstream_time_sources"][0]["auth_pin"]["value"] = "deadbeef"

        def unpinned_interval(inv):
            del inv["upstream_time_sources"][0]["max_uncertainty_milliseconds"]

        def authorized_action(inv):
            inv["actions"]["otp"] = "authorized-by-operator"

        def unknown_top_field(inv):
            inv["notes"] = "anything"

        cases = [
            ("vf2 alias", vf2_alias),
            ("alternate revision", alt_revision),
            ("alternate OPN", alt_opn),
            ("blank serial", blank_serial),
            ("duplicate asset", duplicate_asset),
            ("missing asset", missing_asset),
            ("duplicate evidence hash", duplicate_hash),
            ("evidence hash mismatch", wrong_evidence_hash),
            ("ordered presence", ordered_status),
            ("expected presence", expected_status),
            ("shared AWS classification", shared_aws),
            ("malformed account id", bad_account_id),
            ("missing AWS identity output", missing_aws_identity),
            ("substituted AWS identity output", substituted_aws_identity),
            ("substituted AWS region output", substituted_aws_region),
            ("missing time-source row", no_time_source_row),
            ("unpinned time source", unpinned_time_source),
            ("missing auth pin", missing_auth_pin),
            ("weak pin value", weak_pin_value),
            ("unpinned uncertainty bound", unpinned_interval),
            ("authorized action field", authorized_action),
            ("unknown top-level field", unknown_top_field),
        ]
        for label, mutate in cases:
            with self.subTest(case=label):
                failures = self.run_case(mutate)
                self.assertTrue(failures)

    def test_substituted_board_is_not_admitted(self) -> None:
        swapped = base_inventory()
        swapped["assets"][1] = asset("stm32h573i-dk", "NUCLEO-H573I-DK",
                                     manufacturer="STMicroelectronics")
        status, _ = self.evaluate_inventory(swapped)
        self.assertEqual(status, BLOCKED)

    def test_unusable_inputs_exit_nonzero(self) -> None:
        tool = Path(__file__).with_name("admission.py")
        broken = self.evidence_dir / "broken.json"
        broken.write_text("{not json", encoding="utf-8")
        try:
            proc = subprocess.run(
                [sys.executable, str(tool), "validate",
                 "--inventory", str(broken), "--evidence-dir", str(self.evidence_dir)],
                capture_output=True, text=True)
            self.assertEqual(proc.returncode, 2)
        finally:
            broken.unlink()


if __name__ == "__main__":
    unittest.main()
