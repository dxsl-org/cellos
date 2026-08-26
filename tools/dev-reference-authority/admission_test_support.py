"""Synthetic, non-qualifying fixtures for admission validator tests."""

import hashlib

EVIDENCE_FILES = {
    "vf2-label.jpg": b"visionfive2 label",
    "stm32-label.jpg": b"stm32h573i-dk label",
    "tpm-kit-label.jpg": b"slb9672 kit label",
    "supply-photo.jpg": b"bench supply photo",
    "la-photo.jpg": b"logic analyzer photo",
    "aws-identity.json": (
        b'{"Account":"123456789012","Arn":"arn:aws:sts::123456789012:'
        b'assumed-role/CellosDevReadOnly/session","ConfiguredRegion":"eu-central-1",'
        b'"UserId":"AROATEST:session"}'
    ),
}
EVIDENCE_FILES_LIST = {
    "visionfive2-board": "vf2-label.jpg",
    "stm32h573i-dk": "stm32-label.jpg",
    "slb9672-kit": "tpm-kit-label.jpg",
    "power-reset-isolation": "supply-photo.jpg",
    "logic-analysis": "la-photo.jpg",
}
POWER_CAPS = {
    "bench_supply": True,
    "load_switch_or_reset_supervisor": True,
    "level_shifting_isolation_cabling": True,
    "competing_uart0_tx_disconnect": True,
}
LOGIC_CAPS = {
    "simultaneous_uart0_strap_power_reset_capture": True,
    "voltage_compatible": True,
    "sample_rate_mhz": 200,
    "bandwidth_mhz": 100,
    "capture_toolchain_version": "sigrok 0.7.2",
}


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def attachment(name: str) -> dict:
    return {"name": name, "sha256": _sha(EVIDENCE_FILES[name])}


def asset(kind: str, exact_id: str, **extra) -> dict:
    row = {
        "asset_kind": kind,
        "exact_id": exact_id,
        "manufacturer": "Generic",
        "model_revision": "rev-1",
        "serial_or_asset_id": f"serial-{kind}",
        "custodian": "operator",
        "storage_location": "bench drawer 1",
        "inspected_at": "2026-08-26T12:00:00Z",
        "presence_status": "on-hand",
        "attachment_hashes": [attachment(EVIDENCE_FILES_LIST[kind])],
    }
    row.update(extra)
    return row


def base_inventory() -> dict:
    return {
        "schema": "cellos-dev-admission-v1",
        "classification": "DEV_REFERENCE",
        "assets": [
            asset("visionfive2-board", "StarFive VisionFive 2 v1.3B",
                  manufacturer="StarFive", model_revision="v1.3B", revision="v1.3B"),
            asset("stm32h573i-dk", "STM32H573I-DK", manufacturer="STMicroelectronics",
                  mcu_part_number="STM32H573IIK3Q", marker="development-stm32-authority"),
            asset("slb9672-kit", "Infineon OPTIGA TPM SLB 9672 evaluation kit",
                  manufacturer="Infineon", opn="TPM9672FW1523PCEBTOBO1"),
            asset("power-reset-isolation", "Cellos qualified power/reset isolation equipment",
                  capabilities=POWER_CAPS),
            asset("logic-analysis", "Cellos qualified logic analysis equipment",
                  capabilities=LOGIC_CAPS),
        ],
        "aws_dev_account": {
            "account_alias": "cellos-dev-authority",
            "account_id": "123456789012",
            "region": "eu-central-1",
            "cli_profile": "cellos-dev-ro",
            "classification": "dedicated-dev",
            "identity_evidence": attachment("aws-identity.json"),
        },
        "upstream_time_sources": [{
            "endpoint": "time.example.internal/v1",
            "protocol": "https-signed-time-v1",
            "auth_pin": {"kind": "spki-sha256", "value": "ab" * 32},
            "interval_seconds": 60,
            "max_sample_age_seconds": 300,
            "max_uncertainty_milliseconds": 250,
            "pinned": True,
            "marker": "cellos-dev-time-v1",
        }],
        "actions": {key: "not-authorized" for key in (
            "purchase", "otp", "lifecycle", "debug", "key_creation", "cloud_deployment")},
    }
