"""Fail-closed tests for the deterministic provisioning-plan generator."""

from __future__ import annotations

import hashlib
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
from plan import MUTATION_ORDER, ProvisioningPlanError, generate, main  # noqa: E402

FIXTURE = HERE / "fixtures" / "software-harness-input.json"


class ProvisioningPlanTest(unittest.TestCase):
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
        self.configuration.write_bytes(FIXTURE.read_bytes())

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def load_configuration(self) -> dict:
        return json.loads(FIXTURE.read_text(encoding="utf-8"))

    def write_configuration(self, value: dict) -> None:
        self.configuration.write_text(json.dumps(value), encoding="utf-8")

    def assert_rejected(self, value: dict, message: str) -> None:
        self.write_configuration(value)
        with self.assertRaisesRegex(ProvisioningPlanError, message):
            generate(
                self.inventory,
                self.evidence,
                self.configuration,
                software_harness=True,
            )

    def test_output_is_deterministic_and_binds_approval_hash(self) -> None:
        first = generate(
            self.inventory, self.evidence, self.configuration, software_harness=True
        )
        second = generate(
            self.inventory, self.evidence, self.configuration, software_harness=True
        )
        self.assertEqual(first, second)
        self.assertEqual(
            first["plan_payload"]["authorization"], "ABSENT_DO_NOT_EXECUTE"
        )
        self.assertEqual(first["plan_payload"]["classification"], "SOFTWARE_HARNESS")
        self.assertEqual(first["plan_payload"]["source"]["admission_status"], "BLOCKED")
        self.assertEqual(
            [step["name"] for step in first["plan_payload"]["steps"]],
            list(MUTATION_ORDER),
        )
        approval_hash = first["approval"]["bound_plan_payload_sha256"]
        canonical = json.dumps(
            first["plan_payload"],
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=True,
        ).encode()
        self.assertEqual(approval_hash, hashlib.sha256(canonical).hexdigest())
        for field, replacement in (
            ("classification", "DEV_REFERENCE"),
            ("authorization", "PRESENT"),
        ):
            tampered = json.loads(json.dumps(first["plan_payload"]))
            tampered[field] = replacement
            tampered_bytes = json.dumps(
                tampered,
                sort_keys=True,
                separators=(",", ":"),
                ensure_ascii=True,
            ).encode()
            self.assertNotEqual(
                approval_hash,
                hashlib.sha256(tampered_bytes).hexdigest(),
            )

    def test_cli_writes_same_plan(self) -> None:
        output = self.root / "plan.json"
        result = main([
            "--inventory", str(self.inventory),
            "--evidence-dir", str(self.evidence),
            "--configuration", str(self.configuration),
            "--output", str(output),
            "--software-harness",
        ])
        self.assertEqual(result, 0)
        self.assertEqual(
            json.loads(output.read_text()),
            generate(
                self.inventory,
                self.evidence,
                self.configuration,
                software_harness=True,
            ),
        )

    def test_phase_one_block_prevents_generation(self) -> None:
        inventory = base_inventory()
        inventory["actions"]["otp"] = "authorized"
        self.inventory.write_text(json.dumps(inventory), encoding="utf-8")
        for software_harness in (False, True):
            with self.subTest(software_harness=software_harness):
                with self.assertRaisesRegex(
                    ProvisioningPlanError, "not READY_FOR_PHASE_02"
                ):
                    generate(
                        self.inventory,
                        self.evidence,
                        self.configuration,
                        software_harness=software_harness,
                    )

    def test_unknown_or_missing_configuration_fields_fail(self) -> None:
        configuration = self.load_configuration()
        configuration["extra"] = "forbidden"
        self.assert_rejected(configuration, "fields differ")
        configuration = self.load_configuration()
        del configuration["digests"]["stirot_policy_sha256"]
        self.assert_rejected(configuration, "fields differ")
        configuration = self.load_configuration()
        del configuration["tpm"]["authorization_policy_sha256"]
        self.assert_rejected(configuration, "fields differ")

    def test_duplicate_json_keys_fail(self) -> None:
        self.configuration.write_text(
            '{"schema":"a","schema":"b"}', encoding="utf-8"
        )
        with self.assertRaisesRegex(ProvisioningPlanError, "duplicate JSON key"):
            generate(
                self.inventory,
                self.evidence,
                self.configuration,
                software_harness=True,
            )

    def test_invalid_digests_and_descriptive_rows_fail(self) -> None:
        configuration = self.load_configuration()
        configuration["digests"]["stirot_image_sha256"] = "AA" * 32
        self.assert_rejected(configuration, "lowercase sha256")
        configuration = self.load_configuration()
        configuration["mutations"][0]["recovery_consequence"] = "TBD after hardware"
        self.assert_rejected(configuration, "contains a placeholder")
        configuration = self.load_configuration()
        row = configuration["mutations"][0]
        row["requested_value"] = row.pop("requested_value_hex")
        row["expected_readback"] = row.pop("expected_readback_hex")
        self.assert_rejected(configuration, "fields differ")

    def test_counter_attributes_fail_closed(self) -> None:
        configuration = self.load_configuration()
        configuration["tpm"]["nv_attributes"].append("TPMA_NV_ORDERLY")
        configuration["tpm"]["nv_attributes"].sort()
        self.assert_rejected(configuration, "frozen exact set")
        configuration = self.load_configuration()
        configuration["tpm"]["nv_attributes"].remove("TPMA_NV_COUNTER")
        self.assert_rejected(configuration, "frozen exact set")

    def test_duplicate_handles_and_mutation_reordering_fail(self) -> None:
        configuration = self.load_configuration()
        configuration["tpm"]["pending_relay_handle"] = configuration["tpm"]["active_relay_handle"]
        self.assert_rejected(configuration, "must be distinct")
        configuration = self.load_configuration()
        configuration["mutations"][0], configuration["mutations"][1] = (
            configuration["mutations"][1], configuration["mutations"][0]
        )
        self.assert_rejected(configuration, "frozen order")

    def test_every_step_remains_explicitly_irreversible(self) -> None:
        configuration = self.load_configuration()
        configuration["mutations"][4]["irreversible"] = False
        self.assert_rejected(configuration, "irreversible must be true")


if __name__ == "__main__":
    unittest.main()
