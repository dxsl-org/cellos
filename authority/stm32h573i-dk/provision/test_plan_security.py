"""Security-boundary tests for typed provisioning plans."""

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
from plan import ProvisioningPlanError, generate  # noqa: E402

FIXTURE = HERE / "fixtures" / "software-harness-input.json"
TPM_DIGESTS = (
    "stable_identity_template_sha256",
    "active_relay_template_sha256",
    "pending_relay_template_sha256",
    "nv_public_template_sha256",
    "authorization_policy_sha256",
)


class ProvisioningPlanSecurityTest(unittest.TestCase):
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

    def test_tpm_handle_classes_are_closed(self) -> None:
        configuration = self.fixture()
        configuration["tpm"]["stable_identity_handle"] = "0x80010001"
        configuration["mutations"][0]["target_identifier"] = "0x80010001"
        self.reject(configuration, "outside the persistent range")
        configuration = self.fixture()
        configuration["tpm"]["nv_counter_index"] = "0x02000001"
        configuration["mutations"][3]["target_identifier"] = "0x02000001"
        self.reject(configuration, "outside the NV range")

    def test_every_tpm_template_and_policy_digest_is_required(self) -> None:
        for field in TPM_DIGESTS:
            with self.subTest(field=field):
                configuration = self.fixture()
                del configuration["tpm"][field]
                self.reject(configuration, "fields differ")

    def test_fixture_sentinels_cannot_be_promoted(self) -> None:
        configuration = self.fixture()
        configuration["software_harness_fixture"] = False
        self.reject(
            configuration,
            "preclosure_verification_sha256 is a synthetic sentinel",
        )

    def test_preclosure_checkpoint_is_hash_bound_and_disables_execution(self) -> None:
        configuration = self.fixture()
        self.configuration.write_text(json.dumps(configuration), encoding="utf-8")
        plan = generate(
            self.inventory,
            self.evidence,
            self.configuration,
            software_harness=True,
        )
        gate = plan["plan_payload"]["execution_gate"]
        self.assertEqual(
            gate["preclosure_verification_sha256"],
            configuration["preclosure_verification_sha256"],
        )
        self.assertEqual(gate["operator_approval"], "REQUIRED_AFTER_PLAN_HASH")
        self.assertIs(gate["irreversible_actions_enabled"], False)

    def test_target_identifiers_cannot_drift_from_tpm_map(self) -> None:
        configuration = self.fixture()
        configuration["mutations"][2]["target_identifier"] = "0x81010004"
        self.reject(configuration, "target_identifier differs")

    def test_exact_mutation_descriptor_rejects_drift(self) -> None:
        configuration = self.fixture()
        configuration["mutations"][0]["address"] = "0x81010004"
        self.reject(configuration, "address must equal the TPM target")
        configuration = self.fixture()
        configuration["mutations"][5]["requested_value_hex"] = "00"
        self.reject(configuration, "exact lowercase hex with 8 digits")
        configuration = self.fixture()
        configuration["mutations"][5]["address_space"] = "stm32-mmio"
        self.reject(configuration, "address_space differs")
        configuration = self.fixture()
        configuration["mutations"][5]["width_bits"] = 7
        self.reject(configuration, "byte-aligned 8..32768")

    def test_exact_mutation_bytes_are_self_hashing_and_plan_bound(self) -> None:
        configuration = self.fixture()
        self.configuration.write_text(json.dumps(configuration), encoding="utf-8")
        plan = generate(
            self.inventory,
            self.evidence,
            self.configuration,
            software_harness=True,
        )
        step = plan["plan_payload"]["steps"][5]
        self.assertEqual(step["address_space"], "software-harness")
        self.assertEqual(step["address"], "0x52002000")
        self.assertEqual(step["width_bits"], 32)
        self.assertEqual(len(step["write_mask_hex"]), 8)
        self.assertEqual(
            step["requested_value_sha256"],
            hashlib.sha256(bytes.fromhex(step["requested_value_hex"])).hexdigest(),
        )
        artifact_hash = step["artifact_sha256"]
        unhashed = dict(step)
        del unhashed["artifact_sha256"]
        canonical = json.dumps(
            unhashed, sort_keys=True, separators=(",", ":"), ensure_ascii=True
        ).encode()
        self.assertEqual(artifact_hash, hashlib.sha256(canonical).hexdigest())

    def test_canonical_contract_authorities_are_hash_bound(self) -> None:
        configuration = self.fixture()
        self.configuration.write_text(json.dumps(configuration), encoding="utf-8")
        plan = generate(
            self.inventory,
            self.evidence,
            self.configuration,
            software_harness=True,
        )
        payload = plan["plan_payload"]
        bindings = payload["contract_bindings"]
        self.assertEqual(bindings["authority_protocol"]["protocol_version"], 2)
        self.assertEqual(len(bindings["authority_protocol"]["operation_set"]), 14)
        self.assertEqual(bindings["journal_core"]["record_schema"], "PERSIST-003/FullRecord-v2")
        self.assertEqual(bindings["journal_core"]["record_version"], 2)
        for owner in ("authority_protocol", "journal_core"):
            for name, binding in bindings[owner].items():
                if isinstance(binding, dict) and set(binding) == {"path", "sha256"}:
                    actual = hashlib.sha256(
                        (REPO_ROOT / binding["path"]).read_bytes()
                    ).hexdigest()
                    self.assertEqual(binding["sha256"], actual, name)
            tree = bindings[owner]["source_tree"]
            root = REPO_ROOT / tree["path"]
            files = [root / "Cargo.toml", *sorted((root / "src").rglob("*.rs"))]
            digest = hashlib.sha256()
            for source in files:
                relative = source.relative_to(REPO_ROOT).as_posix().encode()
                content = source.read_bytes()
                digest.update(len(relative).to_bytes(4, "big"))
                digest.update(relative)
                digest.update(len(content).to_bytes(8, "big"))
                digest.update(content)
            self.assertEqual(tree["file_count"], len(files))
            self.assertEqual(tree["sha256"], digest.hexdigest())
        tampered = json.loads(json.dumps(payload))
        tampered["contract_bindings"]["authority_protocol"]["protocol_version"] = 3
        canonical = json.dumps(
            tampered, sort_keys=True, separators=(",", ":"), ensure_ascii=True
        ).encode()
        self.assertNotEqual(
            plan["approval"]["bound_plan_payload_sha256"],
            hashlib.sha256(canonical).hexdigest(),
        )



if __name__ == "__main__":
    unittest.main()
