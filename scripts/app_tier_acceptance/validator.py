"""Fail-closed validation for the authoritative app-tier acceptance ledger."""

from __future__ import annotations

import datetime as dt
import subprocess
from pathlib import Path

from . import cohort, events, source
from .checks import GIT, HEX, begin_context, canonical_digest, end_context, exact, integer, safe_file, text, timestamp

AXES = ["tier", "runtime_profile", "sdk_module", "cpu", "environment", "admission", "ipc", "grant", "mmio", "dma", "lifecycle", "security_negative"]
STATUS, LIFE = {"PASS", "BLOCKED", "PLANNED"}, ["PLANNED", "IMPLEMENTED", "VERIFIED", "LEDGER_RECORDED"]
ROOT_KEYS = {"schema_version", "authoritative", "projection", "axes", "subjects", "blockers", "rows", "security_negatives", "claims", "phase_lifecycle", "c9", "events", "baseline_prefix", "source_binding"}
TUPLE_ENUMS = {
    "tier": {"T1", "T2"},
    "runtime_profile": {"rust-no-std", "rust-std", "ffi-posix", "lua"},
    "admission": {"admitted", "denied", "unqualified"},
    "ipc": {"zero-copy", "copied", "denied", "unqualified"},
    "grant": {"sas-identity", "explicit", "denied", "unqualified"},
    "mmio": {"granted", "denied", "unqualified"},
    "dma": {"granted", "denied", "unqualified"},
    "lifecycle": {"verified", "unqualified"},
    "security_negative": {"deny", "fault", "not-run", "hostile-test-missing"},
}


def artifact_list(root: Path, value: object, label: str) -> None:
    """Validate nonempty raw evidence files, not references to prose."""
    if not isinstance(value, list) or not value:
        raise ValueError(f"{label} needs raw artifacts")
    for item in value:
        exact(item, {"path", "sha256", "size_bytes", "kind"}, label)
        safe_file(root, item["path"], item["sha256"], item["size_bytes"], item["kind"])


def bind_source(root: Path, value: object, require_ratified: bool) -> None:
    """Bind the exact source bytes and matrix; qualification requires a commit copy."""
    item = exact(value, {"path", "sha256", "matrix_sha256", "ratified_revision"}, "source binding")
    raw = source.source_file(root).read_bytes()
    if item["path"] != source.SOURCE_PATH or item["sha256"] != source.sha256_bytes(raw) or item["matrix_sha256"] != source.matrix_digest(root):
        raise ValueError("source import or digest drift")
    revision = item["ratified_revision"]
    if not isinstance(revision, str) or (revision and not GIT.fullmatch(revision)):
        raise ValueError("ratified revision invalid")
    if require_ratified:
        if not revision:
            raise ValueError("qualification needs ratified amended source")
        found = subprocess.run(["git", "show", f"{revision}:{source.SOURCE_PATH}"], cwd=root, capture_output=True)
        if found.returncode or found.stdout != raw:
            raise ValueError("ratified revision does not contain exact source")


def state_digest(root: dict) -> str:
    """Hash the complete present ledger state independently from its history envelope."""
    return canonical_digest({key: value for key, value in root.items() if key not in {"events", "baseline_prefix"}})


def subjects(value: object) -> dict[str, dict]:
    """Require distinct qemu, KVM, and physical execution subjects."""
    if not isinstance(value, list):
        raise ValueError("subjects must be a list")
    found = {}
    for item in value:
        exact(item, {"id", "environment", "architecture", "board_revision", "firmware_digest", "host_vmm"}, "subject")
        ident = text(item["id"], "subject id")
        if ident in found or item["environment"] not in {"qemu", "kvm", "physical"}:
            raise ValueError("duplicate or invalid subject")
        if item["environment"] == "physical" and (not text(item["board_revision"], "board") or not HEX.fullmatch(text(item["firmware_digest"], "firmware"))):
            raise ValueError("physical subject identity incomplete")
        if item["environment"] == "kvm" and not text(item["host_vmm"], "KVM metadata"):
            raise ValueError("KVM subject lacks host metadata")
        found[ident] = item
    if {item["environment"] for item in found.values()} != {"qemu", "kvm", "physical"}:
        raise ValueError("all three execution environments required")
    return found


def claims(value: object, subject_map: dict[str, dict], root: Path) -> dict[str, dict]:
    """Validate typed claim tuples and completion before evidence can reference them."""
    found = {}
    if not isinstance(value, list):
        raise ValueError("claims must be a list")
    for item in value:
        exact(item, {"id", "status", "subject", "tuple", "completion", "source_sha256", "matrix_sha256"}, "claim")
        if text(item["id"], "claim id") in found or item["status"] not in STATUS or item["completion"] not in {"INCOMPLETE", "COMPLETE"}:
            raise ValueError("claim enum or identity invalid")
        tuple_ = exact(item["tuple"], set(AXES), "claim tuple")
        if item["subject"] not in subject_map or tuple_["environment"] != subject_map[item["subject"]]["environment"] or tuple_["cpu"] != subject_map[item["subject"]]["architecture"]:
            raise ValueError("claim subject relationship invalid")
        if any(tuple_[key] not in allowed for key, allowed in TUPLE_ENUMS.items()) or not text(tuple_["sdk_module"], "SDK module"):
            raise ValueError("claim tuple enum invalid")
        if tuple_["tier"] == "T2" and tuple_["ipc"] == "zero-copy":
            raise ValueError("Tier 2 cannot claim SAS zero-copy IPC")
        if tuple_["tier"] == "T2" and tuple_["grant"] == "sas-identity":
            raise ValueError("Tier 2 cannot claim SAS identity grants")
        if tuple_["admission"] == "admitted" and tuple_["lifecycle"] != "verified":
            raise ValueError("admitted claim requires verified lifecycle")
        if (item["status"] == "PASS") != (item["completion"] == "COMPLETE"):
            raise ValueError("PASS claim completion mismatch")
        if item["source_sha256"] != source.sha256_bytes(source.source_file(root).read_bytes()) or item["matrix_sha256"] != source.matrix_digest(root):
            raise ValueError("claim source digest drift")
        canonical = (item["subject"], tuple(tuple_[axis] for axis in AXES))
        if any((claim["subject"], tuple(claim["tuple"][axis] for axis in AXES)) == canonical for claim in found.values()):
            raise ValueError("contradictory duplicate claim tuple")
        found[item["id"]] = item
    return found


def _validate_snapshot(data: object, as_of: dt.datetime, root: Path) -> str:
    """Validate one complete ledger snapshot without consulting a prior snapshot."""
    from . import ledger

    value = exact(data, ROOT_KEYS, "ledger")
    if integer(value["schema_version"], "schema version") != 3 or value["authoritative"] is not True or value["axes"] != AXES:
        raise ValueError("ledger root schema invalid")
    subject_map = subjects(value["subjects"])
    progressed = any(item.get("status") != "PLANNED" for item in value["phase_lifecycle"] if item.get("phase") != 1)
    bind_source(root, value["source_binding"], progressed or value["c9"] == "FULLY_QUALIFIED")
    claim_map = claims(value["claims"], subject_map, root)
    promoted = ledger.cells(value, claim_map, subject_map, as_of, root)
    referenced = {evidence["claim_id"] for row in value["rows"] for cell in row["cells"] for evidence in cell["evidence"]}
    if any(claim["status"] == "PASS" and claim_id not in referenced for claim_id, claim in claim_map.items()):
        raise ValueError("PASS claim is not referenced by a matrix cell")
    states, event_ids = events.history(value, root, state_digest, as_of)
    blocks = ledger.blockers(value, subject_map, root, as_of)
    hostile = ledger.negatives(value, subject_map, root, as_of)
    phases = ledger.lifecycle(value, states, event_ids)
    complete = promoted and blocks and hostile and phases
    if value["c9"] != ("FULLY_QUALIFIED" if complete else "NOT_COMPLETE"):
        raise ValueError("C9 deterministic derivation mismatch")
    return value["c9"]


def validate(data: object, as_of: dt.datetime | None = None, baseline: object | None = None, root: Path = source.ROOT, baseline_root: Path | None = None) -> str:
    """Validate current and, when required, exactly one independently valid baseline."""
    token = begin_context()
    try:
        now = as_of or dt.datetime.now(dt.timezone.utc)
        result = _validate_snapshot(data, now, root)
        value = exact(data, ROOT_KEYS, "ledger")
        needs_anchor = result == "FULLY_QUALIFIED" or any(item["status"] != "PLANNED" for item in value["phase_lifecycle"] if item["phase"] != 1)
        if baseline is not None:
            if not isinstance(baseline, dict):
                raise ValueError("external trusted baseline required")
            prior_time = timestamp(baseline["events"][-1]["recorded_at"], "baseline final event")
            _validate_snapshot(baseline, prior_time, baseline_root or root)
            if value != baseline:
                current_time = timestamp(value["events"][-1]["recorded_at"], "candidate final event")
                if current_time <= prior_time:
                    raise ValueError("candidate event must be later than trusted baseline tip")
                from . import ledger
                ledger.baseline(value, baseline)
        elif needs_anchor:
            raise ValueError("external trusted baseline required")
        return result
    finally:
        end_context(token)
