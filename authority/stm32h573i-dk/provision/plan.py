#!/usr/bin/env python3
"""Generate a deterministic, non-executing STM32/TPM provisioning plan."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
ADMISSION_TOOLS = REPO_ROOT / "tools" / "dev-reference-authority"
if str(ADMISSION_TOOLS) not in sys.path:
    sys.path.insert(0, str(ADMISSION_TOOLS))

from admission import READY, evaluate_inventory  # noqa: E402
from admission_schema import AdmissionError, load_json  # noqa: E402
from contract_bindings import build_contract_bindings  # noqa: E402
from plan_validation import (  # noqa: E402
    MUTATION_ORDER,
    ProvisioningPlanError,
    validate_configuration,
)

PLAN_SCHEMA = "cellos-stm32-provision-plan-v1"



def _canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode()


def _digest(value: object) -> str:
    return hashlib.sha256(_canonical(value)).hexdigest()

def _materialize_steps(mutations: list[dict]) -> list[dict]:
    steps = []
    for index, mutation in enumerate(mutations):
        step = {
            "artifact_schema": "cellos-provision-mutation-artifact-v1",
            "step": index + 1,
            **mutation,
        }
        step["requested_value_sha256"] = hashlib.sha256(
            bytes.fromhex(step["requested_value_hex"])
        ).hexdigest()
        step["expected_readback_sha256"] = hashlib.sha256(
            bytes.fromhex(step["expected_readback_hex"])
        ).hexdigest()
        step["artifact_sha256"] = _digest(step)
        steps.append(step)
    return steps

def _software_harness_allowed(admission_report: dict) -> bool:
    failures = {
        check["id"]
        for check in admission_report["checks"]
        if check["result"] != "pass"
    }
    return failures == {"aws-read-only-identity"}




def generate(
    inventory_path: Path,
    evidence_dir: Path,
    configuration_path: Path,
    *,
    software_harness: bool = False,
) -> dict:
    try:
        inventory = load_json(inventory_path)
        status, admission_report = evaluate_inventory(inventory, evidence_dir)
        configuration = validate_configuration(load_json(configuration_path))
    except AdmissionError as exc:
        raise ProvisioningPlanError(str(exc)) from exc
    if status != READY and (
        not software_harness or not _software_harness_allowed(admission_report)
    ):
        raise ProvisioningPlanError("Phase 1 admission is not READY_FOR_PHASE_02")
    fixture = configuration["software_harness_fixture"]
    if fixture != (status != READY):
        raise ProvisioningPlanError(
            "software_harness_fixture must match the admission evidence class"
        )

    assets = {asset["asset_kind"]: asset for asset in inventory["assets"]}
    classification = "DEV_REFERENCE" if status == READY else "SOFTWARE_HARNESS"
    payload = {
        "schema": PLAN_SCHEMA,
        "classification": classification,
        "authorization": "ABSENT_DO_NOT_EXECUTE",
        "plan_revision": configuration["plan_revision"],
        "source": {
            "admission_status": status,
            "admission_report_sha256": _digest(admission_report),
            "inventory_sha256": _digest(inventory),
            "configuration_sha256": _digest(configuration),
        },
        "contract_bindings": build_contract_bindings(),
        "execution_gate": {
            "preclosure_verification_sha256": configuration[
                "preclosure_verification_sha256"
            ],
            "operator_approval": "REQUIRED_AFTER_PLAN_HASH",
            "irreversible_actions_enabled": False,
        },
        "device_bindings": {
            "stm32_serial_or_asset_id": assets["stm32h573i-dk"]["serial_or_asset_id"],
            "stm32_mcu_part_number": assets["stm32h573i-dk"]["mcu_part_number"],
            "tpm_serial_or_asset_id": assets["slb9672-kit"]["serial_or_asset_id"],
            "tpm_opn": assets["slb9672-kit"]["opn"],
            **configuration["digests"],
        },
        "tpm_map": configuration["tpm"],
        "steps": _materialize_steps(configuration["mutations"]),
    }
    return {
        "plan_payload": payload,
        "approval": {
            "required": True,
            "bound_plan_payload_sha256": _digest(payload),
            "changed_plan_requires_new_approval": True,
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", required=True)
    parser.add_argument("--evidence-dir", required=True)
    parser.add_argument("--configuration", required=True)
    parser.add_argument("--output")
    parser.add_argument(
        "--software-harness",
        action="store_true",
        help="allow deterministic non-qualifying output while Phase 1 is blocked",
    )
    args = parser.parse_args(argv)
    try:
        plan = generate(
            Path(args.inventory),
            Path(args.evidence_dir),
            Path(args.configuration),
            software_harness=args.software_harness,
        )
    except (AdmissionError, ProvisioningPlanError) as exc:
        print(f"provision-plan: {exc}", file=sys.stderr)
        return 2
    rendered = json.dumps(plan, sort_keys=True, indent=2) + "\n"
    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
