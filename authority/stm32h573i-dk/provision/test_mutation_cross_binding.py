"""Cross-binding tests for exact TPM mutation artifacts."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[2]
ADMISSION_TOOLS = REPO_ROOT / "tools" / "dev-reference-authority"
for path in (HERE, ADMISSION_TOOLS):
    if str(path) not in sys.path:
        sys.path.insert(0, str(path))

from admission_test_support import EVIDENCE_FILES, base_inventory  # noqa: E402
from plan import ProvisioningPlanError, generate, main  # noqa: E402

FIXTURE = HERE / "fixtures" / "software-harness-input.json"


class MutationCrossBindingTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.evidence = self.root / "evidence"
        self.evidence.mkdir()
        for name, data in EVIDENCE_FILES.items():
            (self.evidence / name).write_bytes(data)
        self.inventory = self.root / "inventory.json"
        self.inventory.write_text(json.dumps(base_inventory()), encoding="utf-8")
        self.configuration = self.root / "configuration.json"

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def fixture(self) -> dict:
        return json.loads(FIXTURE.read_text(encoding="utf-8"))

    def reject(self, configuration: dict, message: str) -> None:
        self.configuration.write_text(json.dumps(configuration), encoding="utf-8")
        with self.assertRaisesRegex(ProvisioningPlanError, message):
            generate(
                self.inventory,
                self.evidence,
                self.configuration,
                software_harness=True,
            )

    def test_tpm_request_bytes_must_match_template_digest(self) -> None:
        configuration = self.fixture()
        configuration["mutations"][1]["requested_value_hex"] = "21" * 32
        self.reject(configuration, "conflicts with TPM template")

    def test_malformed_tpm_hex_returns_closed_cli_error(self) -> None:
        configuration = self.fixture()
        configuration["mutations"][0]["requested_value_hex"] = "zz" * 32
        self.configuration.write_text(json.dumps(configuration), encoding="utf-8")
        output = self.root / "plan.json"
        result = main([
            "--inventory", str(self.inventory),
            "--evidence-dir", str(self.evidence),
            "--configuration", str(self.configuration),
            "--output", str(output),
            "--software-harness",
        ])
        self.assertEqual(result, 2)
        self.assertFalse(output.exists())

    def test_every_step_policy_must_match_tpm_policy(self) -> None:
        for index in range(9):
            with self.subTest(index=index):
                configuration = self.fixture()
                configuration["mutations"][index]["authorization_policy_sha256"] = (
                    "0123456789abcdef" * 4
                )
                self.reject(configuration, "differs from TPM policy")

    def test_stm32_target_mask_and_readback_are_cross_bound(self) -> None:
        configuration = self.fixture()
        configuration["mutations"][5]["target_identifier"] = (
            "option-byte-bank@0x52002004"
        )
        self.reject(configuration, "target_identifier differs")
        configuration = self.fixture()
        configuration["mutations"][5]["write_mask_hex"] = "00000000"
        self.reject(configuration, "must select at least one bit")
        configuration = self.fixture()
        configuration["mutations"][5]["write_mask_hex"] = "0000ffff"
        self.reject(configuration, "bits outside write_mask_hex")
        configuration = self.fixture()
        configuration["mutations"][5]["write_mask_hex"] = "ffff0000"
        configuration["mutations"][5]["requested_value_hex"] = "60600000"
        configuration["mutations"][5]["expected_readback_hex"] = "60600001"
        self.configuration.write_text(json.dumps(configuration), encoding="utf-8")
        plan = generate(
            self.inventory,
            self.evidence,
            self.configuration,
            software_harness=True,
        )
        self.assertEqual(
            plan["plan_payload"]["steps"][5]["expected_readback_hex"],
            "60600001",
        )
        configuration = self.fixture()
        configuration["mutations"][5]["expected_readback_hex"] = "60606061"
        self.reject(configuration, "conflicts with masked write")

    def test_valid_steps_expose_matching_template_and_policy_bindings(self) -> None:
        configuration = self.fixture()
        self.configuration.write_text(json.dumps(configuration), encoding="utf-8")
        plan = generate(
            self.inventory,
            self.evidence,
            self.configuration,
            software_harness=True,
        )
        payload = plan["plan_payload"]
        fields = (
            "stable_identity_template_sha256",
            "active_relay_template_sha256",
            "pending_relay_template_sha256",
            "nv_public_template_sha256",
        )
        for index, field in enumerate(fields):
            self.assertEqual(
                payload["steps"][index]["requested_value_sha256"],
                payload["tpm_map"][field],
            )
        for step in payload["steps"]:
            self.assertEqual(
                step["authorization_policy_sha256"],
                payload["tpm_map"]["authorization_policy_sha256"],
            )


if __name__ == "__main__":
    unittest.main()
