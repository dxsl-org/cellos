"""Shared fixture construction for acceptance-ledger adversarial tests."""

from __future__ import annotations

import hashlib
import json
import subprocess
import datetime as dt
from pathlib import Path

from app_tier_acceptance import public_api as sdk_source, source

AXES = ["tier", "runtime_profile", "sdk_module", "cpu", "environment", "admission", "ipc", "grant", "mmio", "dma", "lifecycle", "security_negative"]


def artifact(root: Path, path: str) -> dict:
    """Describe an existing raw temporary evidence file."""
    item = root / path
    return {"path": path, "sha256": hashlib.sha256(item.read_bytes()).hexdigest(), "size_bytes": item.stat().st_size, "kind": "artifact"}


def claim(module, cell_id, binding, cpu="riscv64"):
    """Build a typed, completed claim for one current matrix cell."""
    part = cell_id.rsplit("/", 1)[1]
    subjects = {"riscv64": "qemu-rv64", "aarch64": "qemu-arm64", "x86_64": "qemu-x86"}
    tuple_ = {"tier": part if part.startswith("T") else "T1", "runtime_profile": "rust-no-std" if part.startswith("T") else part, "sdk_module": module, "cpu": cpu, "environment": "qemu", "admission": "admitted", "ipc": "copied", "grant": "explicit", "mmio": "denied", "dma": "denied", "lifecycle": "verified", "security_negative": "deny"}
    return {"id": f"claim-{module}-{part}-{cpu}", "status": "PASS", "subject": subjects[cpu], "tuple": tuple_, "completion": "COMPLETE", "source_sha256": binding["sha256"], "matrix_sha256": binding["matrix_sha256"]}


def cohort(
    root,
    claim_value,
    revision,
    tree,
    evidence_artifact,
    canonical_digest,
    target=None,
    selection=None,
    public_api_template=None,
):
    """Build six witnesses linked to one real dirty bundle and claim tuple."""
    patch = artifact(root, "evidence/patch.diff")
    blob = artifact(root, "evidence/untracked.bin")
    bundle = {"base_revision": revision, "base_tree": tree, "patch": patch, "untracked": [patch, blob]}
    bundle["digest"] = canonical_digest(bundle)
    tuple_ = claim_value["tuple"]
    public_api = (
        [dict(entry) for entry in public_api_template]
        if public_api_template is not None
        else [dict(artifact(root, path), kind="source") for path in sorted(sdk_source.paths(root))]
    )
    denominator = {
        "compiler": "nightly-2026-05-01", "target": "riscv64gc-unknown-none-elf",
        "language": "rust", "feature_selection": "api=default;ostd=default;viui=default", "cfg": "target_arch=\"riscv64\"", "cargo_features": "api=default;ostd=default;viui=default", "cargo_profile": "release", "rustflags": "-C relocation-model=pic",
        "runtime_profile": tuple_["runtime_profile"], "source_path": "docs/specs/23-native-sdk-contract.md",
        "source_sha256": claim_value["source_sha256"],
        "public_api_sha256": canonical_digest(public_api), "abi_version": "2",
    }
    if target:
        compiler, target, language, cfg, rustflags, selection, features, profile, runtime, _ = source.denominator_tuple(target, selection)
        denominator.update(compiler=compiler, target=target, language=language, cfg=cfg, rustflags=rustflags, feature_selection=selection, cargo_features=features, cargo_profile=profile, runtime_profile=runtime)
    details = {
        "source": {key: denominator[key] for key in ("source_path", "source_sha256", "public_api_sha256", "abi_version")},
        "compile": {key: denominator[key] for key in ("compiler", "target", "language", "feature_selection", "cfg", "cargo_features", "cargo_profile", "rustflags", "runtime_profile")},
        "test_runtime": {"test_name": "sdk-conformance", "expected_outcome": "pass", "architecture": tuple_["cpu"], "environment": tuple_["environment"], "hardware": "QEMU TCG", "firmware_sha256": "N/A"},
        "delivery": {"build": "PASS", "manifest": "PASS", "package": "PASS", "signing": "PASS", "verification": "PASS", "development_only_scope": "N/A"},
        "architecture": {"architecture": tuple_["cpu"], "target": denominator["target"]},
        "tier": {key: tuple_[key] for key in ("tier", "admission", "ipc", "grant", "mmio", "dma", "security_negative")},
    }

    def witness(name):
        evidence = artifact(root, "docs/specs/23-native-sdk-contract.md") if name == "source" else evidence_artifact
        if name == "source":
            evidence["kind"] = "source"
        artifacts = [evidence, *public_api] if name == "source" else [evidence]
        return {"class": name, "recorded_at": "2026-08-20T00:00:00Z", "expires_at": "2026-08-22T00:00:00Z", "ttl_seconds": 172800, "owner": "sdk-owner", "runner": "independent-runner", "command": f"verify-{name}", "result": "PASS", "details": details[name], "artifacts": artifacts}
    return {"claim_id": claim_value["id"], "subject": claim_value["subject"], "revision": revision, "base_tree": tree, "dirty": True, "dirty_bundle": bundle, "source_sha256": claim_value["source_sha256"], "matrix_sha256": claim_value["matrix_sha256"], "tuple": claim_value["tuple"], "denominator": denominator, "public_api": public_api, "witnesses": [witness(name) for name in ("source", "compile", "test_runtime", "delivery", "architecture", "tier")]}


def refresh_event(data, canonical_digest):
    """Rebind the final append-only event after each intentional test mutation."""
    event = data["events"][-1]
    event["state_digest"] = canonical_digest({key: value for key, value in data.items() if key not in {"events", "baseline_prefix"}})
    event["hash"] = canonical_digest({key: value for key, value in event.items() if key != "hash"})
    data["baseline_prefix"]["tip_hash"] = event["hash"]


def append_event(data, prior, phase, to_status, canonical_digest):
    """Append the sole adjacent structured lifecycle transition for a candidate."""
    events = data["events"]
    before = prior["phase_lifecycle"][phase - 1]["status"]
    ident = f"phase-{phase}-{to_status.lower()}"
    data["phase_lifecycle"][phase - 1]["event_id"] = ident
    mutable = ("source_binding", "subjects", "blockers", "rows", "security_negatives", "claims", "c9", "phase_lifecycle")
    changes = [{"section": key, "before_sha256": canonical_digest(prior[key]), "after_sha256": canonical_digest(data[key])} for key in mutable if data[key] != prior[key]]
    evidence = data["security_negatives"][0]["evidence"]
    if to_status != "VERIFIED":
        evidence = [dict(evidence[0], kind="artifact")]
    action = {"kind": "lifecycle_transition", "phase": phase, "from_status": before, "to_status": to_status, "changes": changes, "evidence": evidence}
    if to_status == "VERIFIED":
        action["independent_runner"] = "independent-runner"
    if to_status == "IMPLEMENTED":
        cohort_value = next(cell["evidence"][0] for row in data["rows"] for cell in row["cells"] if cell["evidence"])
        action["implementation"] = {"revision": cohort_value["revision"], "base_tree": cohort_value["base_tree"], "command": "cargo build --release", "target": cohort_value["denominator"]["target"], "result": "PASS", "artifact": evidence[0]}
    if to_status == "LEDGER_RECORDED":
        action["attestation"] = {"steward": "steward", "reviewer": "reviewer", "statement": "reviewed qualifying ledger delta"}
    prior_time = dt.datetime.fromisoformat(events[-1]["recorded_at"].replace("Z", "+00:00"))
    recorded = (prior_time + dt.timedelta(seconds=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
    events.append({"sequence": len(events) + 1, "event_id": ident, "previous_hash": events[-1]["hash"], "steward": "steward", "reviewer": "reviewer", "recorded_at": recorded, "action": action, "state_digest": "0" * 64, "hash": "0" * 64})
    data["baseline_prefix"] = {"event_count": len(events), "tip_hash": "0" * 64}
    refresh_event(data, canonical_digest)


def append_migration_event(data, prior, canonical_digest, recorded=None):
    """Append a schema_migration event (3 -> 4)."""
    events = data["events"]
    ident = "schema-migration-v3-to-v4"
    changes = [
        {
            "section": "schema_version",
            "before_sha256": canonical_digest(prior["schema_version"]),
            "after_sha256": canonical_digest(data["schema_version"]),
        }
    ]
    evidence = [dict(data["events"][0]["action"]["evidence"][0])]
    action = {
        "kind": "schema_migration",
        "from_version": prior["schema_version"],
        "to_version": data["schema_version"],
        "changes": changes,
        "evidence": evidence,
    }
    if recorded is None:
        prior_time = dt.datetime.fromisoformat(events[-1]["recorded_at"].replace("Z", "+00:00"))
        recorded = (prior_time + dt.timedelta(seconds=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
    events.append({
        "sequence": len(events) + 1,
        "event_id": ident,
        "previous_hash": events[-1]["hash"],
        "steward": "steward",
        "reviewer": "reviewer",
        "recorded_at": recorded,
        "action": action,
        "state_digest": "0" * 64,
        "hash": "0" * 64,
    })
    data["baseline_prefix"] = {"event_count": len(events), "tip_hash": "0" * 64}
    refresh_event(data, canonical_digest)


def append_correction_event(data, prior, canonical_digest, recorded=None, evidence=None):
    """Append a record_correction event."""
    events = data["events"]
    ident = "correction-qemu-arm64"
    changed_sections = [k for k in ("subjects", "blockers") if data[k] != prior[k]]
    changes = [
        {
            "section": k,
            "before_sha256": canonical_digest(prior[k]),
            "after_sha256": canonical_digest(data[k]),
        }
        for k in changed_sections
    ]
    if evidence is None:
        evidence = [dict(data["events"][0]["action"]["evidence"][0])]
    action = {
        "kind": "record_correction",
        "changes": changes,
        "evidence": evidence,
    }
    if recorded is None:
        prior_time = dt.datetime.fromisoformat(events[-1]["recorded_at"].replace("Z", "+00:00"))
        recorded = (prior_time + dt.timedelta(seconds=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
    events.append({
        "sequence": len(events) + 1,
        "event_id": ident,
        "previous_hash": events[-1]["hash"],
        "steward": "steward",
        "reviewer": "reviewer",
        "recorded_at": recorded,
        "action": action,
        "state_digest": "0" * 64,
        "hash": "0" * 64,
    })
    data["baseline_prefix"] = {"event_count": len(events), "tip_hash": "0" * 64}
    refresh_event(data, canonical_digest)


def append_resolution_event(data, prior, blocker_id, github_approval_data, canonical_digest, recorded=None, evidence=None):
    """Append a blocker_resolution event."""
    events = data["events"]
    ident = f"resolution-{blocker_id.lower()}"
    changes = [
        {
            "section": "blockers",
            "before_sha256": canonical_digest(prior["blockers"]),
            "after_sha256": canonical_digest(data["blockers"]),
        }
    ]
    action = {
        "kind": "blocker_resolution",
        "blocker_id": blocker_id,
        "changes": changes,
        "evidence": evidence,
        "github_approval": github_approval_data,
    }
    if recorded is None:
        prior_time = dt.datetime.fromisoformat(events[-1]["recorded_at"].replace("Z", "+00:00"))
        recorded = (prior_time + dt.timedelta(seconds=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
    events.append({
        "sequence": len(events) + 1,
        "event_id": ident,
        "previous_hash": events[-1]["hash"],
        "steward": "steward",
        "reviewer": github_approval_data["approver"],
        "recorded_at": recorded,
        "action": action,
        "state_digest": "0" * 64,
        "hash": "0" * 64,
    })
    data["baseline_prefix"] = {"event_count": len(events), "tip_hash": "0" * 64}
    refresh_event(data, canonical_digest)
