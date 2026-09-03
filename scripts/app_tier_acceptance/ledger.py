"""State and transition checks for the app-tier acceptance ledger."""

from __future__ import annotations

import datetime as dt
from pathlib import Path

from . import cohort, security_negative, source
from .checks import canonical_digest, exact, integer, text, timestamp
from .validator import LIFE, STATUS, artifact_list


def cells(root: dict, claim_map: dict, subject_map: dict, as_of, path: Path) -> bool:
    """Validate the exact imported 10x6 matrix and passing cohort denominator."""
    rows, imported, complete = root["rows"], source.matrix(path), True
    if not isinstance(rows, list) or [row.get("id") for row in rows] != [row[0] for row in imported]:
        raise ValueError("matrix row import drift")
    for row, imported_row in zip(rows, imported):
        exact(row, {"id", "aggregate", "cells"}, "matrix row")
        if not isinstance(row["cells"], list) or [cell.get("id") for cell in row["cells"]] != [f"{row['id']}/{axis}" for axis in source.CELL_AXES]:
            raise ValueError("matrix needs six cells per row")
        required = []
        for cell, raw in zip(row["cells"], imported_row[1]):
            allowed = {"id", "source_text", "source_availability", "applicability", "required_for_c9", "status", "evidence"}
            if set(cell) != allowed and set(cell) != allowed - {"applicability"}:
                raise ValueError("matrix cell schema invalid")
            available = source.availability(raw)
            expected_scope = source.applicability(row["id"], cell["id"])
            scope = cell.get("applicability", expected_scope)
            if cell["source_text"] != raw or cell["source_availability"] != available or scope != expected_scope or not isinstance(cell["required_for_c9"], bool):
                raise ValueError("matrix cell source import drift")
            expected_need = available not in {"N/A", "UNSUPPORTED"}
            if cell["required_for_c9"] != expected_need:
                raise ValueError("matrix applicability drift")
            if available == "USABLE":
                required_denominators = set(expected_scope["build_denominators"])
                seen_denominators = set()
                if not required_denominators:
                    raise ValueError("cell has no ratified build denominator")
                if not isinstance(cell["evidence"], list) or not cell["evidence"]:
                    raise ValueError("usable cell needs applicable cohort coverage")
                for evidence in cell["evidence"]:
                    claim = claim_map.get(evidence.get("claim_id"))
                    if claim is None:
                        raise ValueError("cohort claim is missing")
                    axis = claim["tuple"]["runtime_profile"] if "/T" not in cell["id"] else claim["tuple"]["tier"]
                    if claim["tuple"]["sdk_module"] != row["id"] or cell["id"].rsplit("/", 1)[1] != axis:
                        raise ValueError("cohort claim does not name this cell")
                    denominator = evidence.get("denominator", {})
                    key = source.denominator(denominator.get("target"), denominator.get("feature_selection"))
                    if key not in required_denominators or key in seen_denominators:
                        raise ValueError("cohort coverage is outside or duplicates applicability")
                    seen_denominators.add(key)
                    cohort.validate(path, evidence, claim, subject_map[claim["subject"]], as_of)
                if seen_denominators != required_denominators:
                    raise ValueError("usable cell lacks complete build-denominator coverage")
                expected = "PASS"
            else:
                expected = "PLANNED" if available == "PLANNED" else "BLOCKED"
                if cell["evidence"]:
                    raise ValueError("non-usable cell cannot carry evidence")
            if cell["status"] != expected:
                raise ValueError("matrix status inflation")
            if expected_need:
                required.append(cell)
        expected_aggregate = "PASS" if required and all(item["status"] == "PASS" for item in required) else "BLOCKED"
        if row["aggregate"] != expected_aggregate:
            raise ValueError("matrix aggregate drift")
        complete = complete and expected_aggregate == "PASS"
    return complete


RESOLUTION_KEYS = {
    "event_id", "subject", "architecture", "environment", "hardware", "firmware_sha256",
    "owner", "runner", "command", "recorded_at", "expires_at", "ttl_seconds", "artifacts",
}


def blocker_resolution(path: Path, value: object, subject_id: str, subject: dict, as_of) -> str:
    """Bind a passing blocker to fresh raw evidence from its exact subject."""
    resolution = exact(value, RESOLUTION_KEYS, "blocker resolution")
    for key in ("event_id", "subject", "owner", "runner", "command"):
        text(resolution[key], f"blocker resolution {key}")
    if resolution["subject"] != subject_id:
        raise ValueError("blocker resolution subject mismatch")
    if resolution["owner"] == resolution["runner"]:
        raise ValueError("blocker resolution owner and runner must be independent")
    if resolution["architecture"] != subject["architecture"] or resolution["environment"] != subject["environment"]:
        raise ValueError("blocker resolution execution subject mismatch")
    hardware = subject["board_revision"] if subject["environment"] == "physical" else subject["host_vmm"]
    firmware = subject["firmware_digest"] if subject["environment"] == "physical" else "N/A"
    if resolution["hardware"] != hardware or resolution["firmware_sha256"] != firmware:
        raise ValueError("blocker resolution hardware identity mismatch")
    recorded = timestamp(resolution["recorded_at"], "blocker resolution recorded")
    expires = timestamp(resolution["expires_at"], "blocker resolution expiry")
    ttl = integer(resolution["ttl_seconds"], "blocker resolution ttl")
    if ttl <= 0 or expires != recorded + dt.timedelta(seconds=ttl) or recorded > as_of or expires <= as_of:
        raise ValueError("blocker resolution TTL invalid")
    artifact_list(path, resolution["artifacts"], "blocker resolution")
    if not any(artifact["kind"] == "log" for artifact in resolution["artifacts"]):
        raise ValueError("blocker resolution needs a raw log")
    return resolution["event_id"]


def blockers(root: dict, subject_map: dict, path: Path, as_of) -> bool:
    """Validate bounded blocker scope and subject-bound PASS resolution evidence."""
    for item in root["blockers"]:
        exact(item, {"id", "status", "subject", "scope", "evidence", "resolution"}, "blocker")
        if item["status"] not in STATUS or item["subject"] not in subject_map or not text(item["scope"], "blocker scope"):
            raise ValueError("blocker identity invalid")
        artifact_list(path, item["evidence"], "blocker evidence")
        if item["status"] == "PASS":
            event_id = blocker_resolution(path, item["resolution"], item["subject"], subject_map[item["subject"]], as_of)
            event = next((event for event in root["events"] if event["event_id"] == event_id), None)
            if event is None or not any(change["section"] == "blockers" for change in event["action"].get("changes", [])):
                raise ValueError("blocker resolution event is missing or unrelated")
            if root.get("schema_version") == 4:
                if event["action"].get("kind") != "blocker_resolution":
                    raise ValueError("schema 4 blocker resolution requires blocker_resolution event")
                if item["id"] == "B-AARCH64-SEMHOSTING" and (item["subject"] != "qemu-arm64" or item["resolution"]["architecture"] != "aarch64"):
                    raise ValueError("B-AARCH64-SEMHOSTING must resolve against qemu-arm64 aarch64 subject")
            if event["action"].get("kind") == "blocker_resolution" and event["action"].get("blocker_id") != item["id"]:
                raise ValueError("blocker resolution event blocker id mismatch")
        elif item["resolution"] is not None:
            raise ValueError("unresolved blocker cannot have resolution")
    return all(item["status"] == "PASS" for item in root["blockers"])


def negatives(root: dict, subject_map: dict, path: Path, as_of) -> bool:
    """Security success must have raw hostile-test evidence and a deny/fault result."""
    valid_expected, valid_observed = {"deny", "fault"}, {"not-run", "deny", "fault", "allow", "panic", "read"}
    if len(root["security_negatives"]) != len(security_negative.CASE_IDS) or {item.get("id") for item in root["security_negatives"]} != security_negative.CASE_IDS:
        raise ValueError("Spec 22 security-negative cases are incomplete")
    for item in root["security_negatives"]:
        exact(item, {"id", "status", "subject", "expected", "observed", "evidence", "witness"}, "security negative")
        if item["status"] not in STATUS or item["subject"] not in subject_map or item["expected"] not in valid_expected or item["observed"] not in valid_observed:
            raise ValueError("security-negative enum invalid")
        artifact_list(path, item["evidence"], "security-negative evidence")
        if item["observed"] in {"allow", "panic", "read"}:
            raise ValueError("security-negative failure dominates")
        if item["status"] == "PASS" and item["observed"] != item["expected"]:
            raise ValueError("security-negative PASS requires expected raw outcome")
        if item["status"] == "PASS":
            event_id = security_negative.validate(path, item["witness"], item, subject_map[item["subject"]], as_of)
            event = next((event for event in root["events"] if event["event_id"] == event_id), None)
            if event is None or not any(change["section"] == "security_negatives" for change in event["action"].get("changes", [])):
                raise ValueError("security-negative event is missing or unrelated")
        elif item["witness"] is not None:
            raise ValueError("non-passing security negative cannot carry a witness")
    return all(item["status"] == "PASS" for item in root["security_negatives"])


def lifecycle(root: dict, states: dict[int, str], event_ids: dict[str, tuple[int, str]]) -> bool:
    """Require lifecycle items to name the exact replayed event and final state."""
    items = root["phase_lifecycle"]
    if not isinstance(items, list) or [item.get("phase") for item in items] != list(range(1, 9)):
        raise ValueError("phase lifecycle shape invalid")
    for item in items:
        exact(item, {"phase", "status", "event_id"}, "phase lifecycle")
        event = event_ids.get(item["event_id"])
        if item["status"] not in LIFE or states[item["phase"]] != item["status"] or (item["status"] == "PLANNED" and item["event_id"] is not None) or (item["status"] != "PLANNED" and event != (item["phase"], item["status"])):
            raise ValueError("phase lifecycle state invalid")
    return all(item["status"] == "LEDGER_RECORDED" for item in items)


def baseline(current: dict, prior: object) -> None:
    """Permit one appended event and exactly one adjacent lifecycle state transition or governance action."""
    if not isinstance(prior, dict):
        raise ValueError("external trusted baseline required")
    previous_events = prior.get("events")
    if not isinstance(previous_events, list) or current["events"][: len(previous_events)] != previous_events or len(current["events"]) != len(previous_events) + 1:
        raise ValueError("candidate must append exactly one event to baseline")
    action = current["events"][-1]["action"]
    kind = action.get("kind")

    if kind == "schema_migration":
        if prior.get("schema_version") != 3 or current.get("schema_version") != 4:
            raise ValueError("schema migration must transition schema_version 3 to 4")
        if current["phase_lifecycle"] != prior.get("phase_lifecycle"):
            raise ValueError("schema migration cannot modify phase lifecycle")
        immutable = set(current) - {"events", "baseline_prefix", "schema_version"}
        if any(current[key] != prior.get(key) for key in immutable):
            raise ValueError("schema migration changed unauthorized section")
        expected = [{"section": "schema_version", "before_sha256": canonical_digest(3), "after_sha256": canonical_digest(4)}]
        if action.get("changes") != expected:
            raise ValueError("schema migration changes mismatch")
        return

    if kind == "record_correction":
        if prior.get("schema_version") != 4 or current.get("schema_version") != 4:
            raise ValueError("record correction requires schema version 4")
        if current["phase_lifecycle"] != prior.get("phase_lifecycle"):
            raise ValueError("record correction cannot modify phase lifecycle")
        allowed = {"events", "baseline_prefix", "subjects", "blockers"}
        if any(current[key] != prior.get(key) for key in set(current) - allowed):
            raise ValueError("record correction changed unauthorized section")
        prior_sub = prior.get("subjects", [])
        if current["subjects"][: len(prior_sub)] != prior_sub:
            raise ValueError("record correction cannot modify existing subjects")
        prior_b, current_b = prior.get("blockers", []), current.get("blockers", [])
        if len(current_b) != len(prior_b):
            raise ValueError("record correction cannot add or remove blockers")
        for before, after in zip(prior_b, current_b):
            if before["id"] != after["id"] or before["evidence"] != after["evidence"]:
                raise ValueError("record correction cannot modify blocker id or evidence")
            if after["status"] != "BLOCKED" or after["resolution"] is not None:
                raise ValueError("record correction must keep blocker BLOCKED and resolution null")
        changed = [k for k in ("subjects", "blockers") if current[k] != prior[k]]
        if not changed:
            raise ValueError("record correction must change subjects or blockers")
        expected = [{"section": k, "before_sha256": canonical_digest(prior[k]), "after_sha256": canonical_digest(current[k])} for k in changed]
        if action.get("changes") != expected:
            raise ValueError("record correction changes mismatch")
        return

    if kind == "blocker_resolution":
        if prior.get("schema_version") != 4 or current.get("schema_version") != 4:
            raise ValueError("blocker resolution requires schema version 4")
        if current["phase_lifecycle"] != prior.get("phase_lifecycle"):
            raise ValueError("blocker resolution cannot modify phase lifecycle")
        allowed = {"events", "baseline_prefix", "blockers"}
        if any(current[key] != prior.get(key) for key in set(current) - allowed):
            raise ValueError("blocker resolution changed unauthorized section")
        blocker_id = action.get("blocker_id")
        prior_b, current_b = prior.get("blockers", []), current.get("blockers", [])
        if len(current_b) != len(prior_b):
            raise ValueError("blocker resolution cannot add or remove blockers")
        changed = [(b, a) for b, a in zip(prior_b, current_b) if b != a]
        if len(changed) != 1 or changed[0][1]["id"] != blocker_id:
            raise ValueError("blocker resolution must modify only the named blocker")
        before, after = changed[0]
        if before["status"] != "BLOCKED" or before["resolution"] is not None:
            raise ValueError("resolved blocker must have been BLOCKED without resolution")
        if after["status"] != "PASS" or not isinstance(after["resolution"], dict):
            raise ValueError("resolved blocker must be PASS with resolution")
        if (after["id"], after["subject"], after["scope"], after["evidence"]) != (before["id"], before["subject"], before["scope"], before["evidence"]):
            raise ValueError("resolved blocker cannot change identity, subject, scope, or historical evidence")
        if after["resolution"]["event_id"] != current["events"][-1]["event_id"]:
            raise ValueError("resolved blocker must bind current event id")
        expected = [{"section": "blockers", "before_sha256": canonical_digest(prior_b), "after_sha256": canonical_digest(current_b)}]
        if action.get("changes") != expected:
            raise ValueError("blocker resolution changes mismatch")
        historical = {(art["path"], art["sha256"]) for b in current_b for art in b["evidence"]}
        if {(art["path"], art["sha256"]) for art in action.get("evidence", [])} & historical:
            raise ValueError("blocker resolution reuses historical blocker evidence")
        return

    if kind != "lifecycle_transition":
        raise ValueError("unsupported baseline transition action")

    old = prior.get("phase_lifecycle")
    if not isinstance(old, list) or len(old) != 8:
        raise ValueError("baseline lifecycle invalid")
    changed = [(before, after) for before, after in zip(old, current["phase_lifecycle"]) if before["status"] != after["status"]]
    if len(changed) != 1 or changed[0][0]["phase"] != changed[0][1]["phase"] or LIFE.index(changed[0][1]["status"]) != LIFE.index(changed[0][0]["status"]) + 1:
        raise ValueError("lifecycle must advance one external-baseline step")
    if prior.get("schema_version") == 4 and (current["subjects"] != prior["subjects"] or current["blockers"] != prior["blockers"]):
        raise ValueError("lifecycle transition cannot bundle correction or resolution")
    mutable = ("source_binding", "subjects", "blockers", "rows", "security_negatives", "claims", "c9")
    immutable = set(current) - {"events", "baseline_prefix", "phase_lifecycle", *mutable}
    if any(current[key] != prior.get(key) for key in immutable):
        raise ValueError("immutable ledger section changed")
    expected_changes = [
        {"section": key, "before_sha256": canonical_digest(prior[key]), "after_sha256": canonical_digest(current[key])}
        for key in mutable
        if current[key] != prior[key]
    ]
    expected_changes.append({
        "section": "phase_lifecycle",
        "before_sha256": canonical_digest(prior["phase_lifecycle"]),
        "after_sha256": canonical_digest(current["phase_lifecycle"]),
    })
    expected_action = {
        "kind": "lifecycle_transition",
        "phase": changed[0][1]["phase"],
        "from_status": changed[0][0]["status"],
        "to_status": changed[0][1]["status"],
        "changes": expected_changes,
        "evidence": action.get("evidence"),
    }
    if action["to_status"] == "VERIFIED":
        expected_action["independent_runner"] = action.get("independent_runner")
    if action["to_status"] == "IMPLEMENTED":
        expected_action["implementation"] = action.get("implementation")
    if action["to_status"] == "LEDGER_RECORDED":
        expected_action["attestation"] = action.get("attestation")
    if action != expected_action:
        raise ValueError("event action does not bind the complete state delta")
    blocker_evidence = {
        (artifact["path"], artifact["sha256"])
        for blocker in current["blockers"] for artifact in blocker["evidence"]
    }
    if {(artifact["path"], artifact["sha256"]) for artifact in action["evidence"]} & blocker_evidence:
        raise ValueError("lifecycle transition reuses blocker evidence")
    for before, after in zip(prior["blockers"], current["blockers"]):
        if tuple(before[key] for key in ("id", "subject", "scope", "evidence")) != tuple(after[key] for key in ("id", "subject", "scope", "evidence")):
            raise ValueError("blocker scope/evidence immutable")
