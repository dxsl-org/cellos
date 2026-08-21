"""Typed hostile-test witnesses for security-negative acceptance claims."""

from __future__ import annotations

import datetime as dt
from pathlib import Path

from .checks import exact, integer, text, timestamp
from .events import artifacts

KEYS = {
    "owner", "runner", "command", "test_name", "target", "architecture",
    "environment", "hardware", "firmware_sha256", "expected", "observed",
    "recorded_at", "expires_at", "ttl_seconds", "artifacts", "event_id",
}
TARGET_PREFIX = {"riscv64": "riscv64", "aarch64": "aarch64", "x86_64": "x86_64"}
CASE_IDS = {
    "S22-01-PEER-PAGE", "S22-02-PRIVILEGED-PROBE", "S22-03-SYSCALL-POINTER",
    "S22-04-SAS-SCHEDULE", "S22-05-DOMAIN-SWITCH", "S22-06-TAG-REUSE",
    "S22-07-GRANT-REVOKE", "S22-08-KILL-DMA", "S22-09-FORCED-EXIT",
    "S22-10-INVALID-IMAGE", "S22-11-DEVICE-REQUEST", "S22-12-ROLLBACK",
}


def validate(root: Path, value: object, claim: dict, subject: dict, as_of: dt.datetime) -> str:
    """Validate one hostile test against its claim, subject, clock, and artifacts."""
    witness = exact(value, KEYS, "security-negative witness")
    for key in ("owner", "runner", "command", "test_name", "target", "event_id"):
        text(witness[key], f"security-negative {key}")
    if witness["owner"] == witness["runner"]:
        raise ValueError("security-negative owner and runner must be independent")
    if witness["architecture"] != subject["architecture"] or witness["environment"] != subject["environment"]:
        raise ValueError("security-negative execution subject mismatch")
    if not witness["target"].startswith(TARGET_PREFIX[subject["architecture"]]):
        raise ValueError("security-negative target mismatch")
    hardware = subject["board_revision"] if subject["environment"] == "physical" else subject["host_vmm"]
    firmware = subject["firmware_digest"] if subject["environment"] == "physical" else "N/A"
    if witness["hardware"] != hardware or witness["firmware_sha256"] != firmware:
        raise ValueError("security-negative hardware identity mismatch")
    if witness["expected"] != claim["expected"] or witness["observed"] != claim["observed"]:
        raise ValueError("security-negative outcome mismatch")
    recorded = timestamp(witness["recorded_at"], "security-negative recorded")
    expires = timestamp(witness["expires_at"], "security-negative expiry")
    ttl = integer(witness["ttl_seconds"], "security-negative ttl")
    if ttl <= 0 or expires != recorded + dt.timedelta(seconds=ttl) or recorded > as_of or expires <= as_of:
        raise ValueError("security-negative TTL invalid")
    artifacts(root, witness["artifacts"], "security-negative artifacts")
    if not any(artifact["kind"] == "log" for artifact in witness["artifacts"]):
        raise ValueError("security-negative witness needs a raw log")
    return witness["event_id"]
