"""Security boundary tests for DEV_REFERENCE evidence admission."""

import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from admission import AdmissionError, BLOCKED, evaluate
from admission_test_support import EVIDENCE_FILES, base_inventory


class AdmissionSecurityTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.evidence_dir = Path(self.temporary.name)
        for name, data in EVIDENCE_FILES.items():
            (self.evidence_dir / name).write_bytes(data)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def evaluate(self, inventory: dict):
        path = self.evidence_dir / "inventory.json"
        path.write_text(json.dumps(inventory), encoding="utf-8")
        return evaluate(path, self.evidence_dir)

    def assert_blocked(self, inventory: dict, check_id: str, detail: str | None = None) -> None:
        status, report = self.evaluate(inventory)
        self.assertEqual(status, BLOCKED)
        failure = next(c for c in report["checks"] if c["id"] == check_id)
        self.assertEqual(failure["result"], "fail")
        if detail is not None:
            self.assertIn(detail, failure["detail"])


    def replace_identity_evidence(self, inventory: dict, captured: dict) -> None:
        encoded = json.dumps(captured, separators=(",", ":")).encode()
        (self.evidence_dir / "aws-identity.json").write_bytes(encoded)
        inventory["aws_dev_account"]["identity_evidence"]["sha256"] = hashlib.sha256(
            encoded
        ).hexdigest()
    def test_multiple_upstream_sources_are_rejected(self) -> None:
        inventory = base_inventory()
        inventory["upstream_time_sources"].append(
            copy.deepcopy(inventory["upstream_time_sources"][0])
        )
        self.assert_blocked(inventory, "closed-schema", "at most 1")

    def test_noncanonical_attachment_path_is_rejected(self) -> None:
        inventory = base_inventory()
        inventory["assets"][0]["attachment_hashes"][0]["name"] = "sub/../vf2-label.jpg"
        self.assert_blocked(inventory, "evidence-attachments-present", "canonical")

    def test_absolute_attachment_path_is_rejected(self) -> None:
        inventory = base_inventory()
        inventory["assets"][0]["attachment_hashes"][0]["name"] = str(
            self.evidence_dir / "vf2-label.jpg"
        )
        self.assert_blocked(inventory, "evidence-attachments-present", "canonical")
    def test_nonfinite_json_number_is_unusable_input(self) -> None:
        inventory = base_inventory()
        inventory["upstream_time_sources"][0]["max_uncertainty_milliseconds"] = float("nan")
        path = self.evidence_dir / "nonfinite.json"
        path.write_text(json.dumps(inventory), encoding="utf-8")
        with self.assertRaises(AdmissionError):
            evaluate(path, self.evidence_dir)

    def test_root_or_unproven_writable_aws_identity_is_rejected(self) -> None:
        inventory = base_inventory()
        self.replace_identity_evidence(
            inventory,
            {
                "Account": "123456789012",
                "Arn": "arn:aws:iam::123456789012:root",
                "ConfiguredRegion": "eu-central-1",
                "ReadOnlyVerified": False,
                "WriteActionsDenied": [],
            },
        )
        self.assert_blocked(inventory, "aws-read-only-identity", "must not be account root")



    def test_invalid_utf8_is_unusable_input(self) -> None:
        path = self.evidence_dir / "invalid-utf8.json"
        path.write_bytes(b"{\xff}")
        with self.assertRaises(AdmissionError):
            evaluate(path, self.evidence_dir)


if __name__ == "__main__":
    unittest.main()
