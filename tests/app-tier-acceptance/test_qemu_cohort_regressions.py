"""Adversarial regressions keeping QEMU evidence within its own subject."""

from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from app_tier_acceptance import validator
from app_tier_acceptance.checks import canonical_digest
from fixtures import refresh_event
import test_acceptance_ledger as acceptance


class QemuCohortRegressionTests(unittest.TestCase):
    """A QEMU witness cannot be substituted for KVM or physical evidence."""

    def fixture(self):
        """Return the shared real-Git cohort fixture with all source bindings intact."""
        return acceptance.LedgerTests("run").future()

    @staticmethod
    def cohort(data: dict) -> dict:
        """Return one passing QEMU cohort from the generated matrix evidence."""
        return next(
            evidence
            for row in data["rows"]
            for cell in row["cells"]
            for evidence in cell["evidence"]
            if evidence["subject"].startswith("qemu-")
        )

    @staticmethod
    def witness(cohort: dict, kind: str) -> dict:
        """Return a class-specific witness without depending on witness order."""
        return next(witness for witness in cohort["witnesses"] if witness["class"] == kind)

    def reject(self, change) -> None:
        """Rebind intentional mutations so the asserted failure reaches semantic checks."""
        root, data, baseline, baseline_root = self.fixture()
        change(data)
        refresh_event(data, canonical_digest)
        with self.assertRaises(ValueError):
            validator.validate(data, acceptance.NOW, baseline, root, baseline_root)

    def test_qemu_runtime_witness_cannot_be_relabelled_as_kvm_or_physical(self) -> None:
        for environment, hardware, firmware in (
            ("kvm", "ARM64 KVM", "N/A"),
            ("physical", "Raspberry Pi 3 Model B v1.2", "0" * 64),
        ):
            with self.subTest(environment=environment):
                def change(data, environment=environment, hardware=hardware, firmware=firmware):
                    runtime = self.witness(self.cohort(data), "test_runtime")
                    runtime["details"].update(
                        environment=environment,
                        hardware=hardware,
                        firmware_sha256=firmware,
                    )

                self.reject(change)

    def test_qemu_cohort_rejects_tuple_hardware_and_firmware_drift(self) -> None:
        def tuple_drift(data):
            self.cohort(data)["tuple"]["environment"] = "kvm"

        def hardware_drift(data):
            self.witness(self.cohort(data), "test_runtime")["details"]["hardware"] = "ARM64 KVM"

        def firmware_drift(data):
            self.witness(self.cohort(data), "test_runtime")["details"]["firmware_sha256"] = "0" * 64

        for change in (tuple_drift, hardware_drift, firmware_drift):
            with self.subTest(change=change.__name__):
                self.reject(change)

    def test_qemu_witness_requires_raw_evidence_and_a_live_ttl(self) -> None:
        def missing_raw_evidence(data):
            self.witness(self.cohort(data), "test_runtime")["artifacts"] = []

        def expired_ttl(data):
            runtime = self.witness(self.cohort(data), "test_runtime")
            runtime["ttl_seconds"] = 0
            runtime["expires_at"] = runtime["recorded_at"]

        for change in (missing_raw_evidence, expired_ttl):
            with self.subTest(change=change.__name__):
                self.reject(change)

    def test_qemu_witness_requires_an_independent_runner(self) -> None:
        def owner_is_runner(data):
            runtime = self.witness(self.cohort(data), "test_runtime")
            runtime["runner"] = runtime["owner"]

        self.reject(owner_is_runner)

    def test_qemu_resolution_cannot_clear_a_physical_blocker(self) -> None:
        def qemu_resolution_for_physical_blocker(data):
            blocker = next(item for item in data["blockers"] if item["subject"] == "physical-rpi3")
            resolution = blocker["resolution"]
            resolution.update(
                subject="qemu-rv64",
                architecture="riscv64",
                environment="qemu",
                hardware="QEMU TCG",
                firmware_sha256="N/A",
            )

        self.reject(qemu_resolution_for_physical_blocker)
