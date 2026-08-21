"""Structured append-only lifecycle event validation."""

from __future__ import annotations

from pathlib import Path

from .checks import GIT, HEX, canonical_digest, exact, integer, safe_file, text, timestamp

LIFE = ["PLANNED", "IMPLEMENTED", "VERIFIED", "LEDGER_RECORDED"]


def artifacts(root: Path, value: object, label: str) -> None:
    """Require nonempty content-addressed event evidence."""
    if not isinstance(value, list) or not value:
        raise ValueError(f"{label} needs raw evidence")
    for item in value:
        exact(item, {"path", "sha256", "size_bytes", "kind"}, label)
        safe_file(root, item["path"], item["sha256"], item["size_bytes"], item["kind"])


def action(root: Path, value: object, states: dict[int, str]) -> tuple[int, str]:
    """Replay one exact seed or adjacent lifecycle transition action."""
    if not isinstance(value, dict) or value.get("kind") not in {"seed", "lifecycle_transition"}:
        raise ValueError("event action kind invalid")
    if value["kind"] == "seed":
        item = exact(value, {"kind", "phase", "to_status", "evidence"}, "seed action")
        if item["phase"] != 1 or item["to_status"] != "LEDGER_RECORDED" or states[1] != "PLANNED":
            raise ValueError("seed action invalid")
        artifacts(root, item["evidence"], "seed evidence")
        states[1] = item["to_status"]
        return 1, item["to_status"]
    keys = {"kind", "phase", "from_status", "to_status", "changes", "evidence"}
    if isinstance(value, dict) and value.get("to_status") == "VERIFIED":
        keys.add("independent_runner")
    if isinstance(value, dict) and value.get("to_status") == "IMPLEMENTED":
        keys.add("implementation")
    if isinstance(value, dict) and value.get("to_status") == "LEDGER_RECORDED":
        keys.add("attestation")
    item = exact(value, keys, "transition action")
    phase, before, after = item["phase"], item["from_status"], item["to_status"]
    if phase not in states or before != states[phase] or before not in LIFE or after not in LIFE or LIFE.index(after) != LIFE.index(before) + 1:
        raise ValueError("non-adjacent lifecycle action")
    if not isinstance(item["changes"], list) or (after == "LEDGER_RECORDED" and not item["changes"]):
        raise ValueError("ledger-recorded transition needs a meaningful ledger delta")
    for change in item["changes"]:
        exact(change, {"section", "before_sha256", "after_sha256"}, "transition change")
        text(change["section"], "transition section")
        if not HEX.fullmatch(text(change["before_sha256"], "before digest")) or not HEX.fullmatch(text(change["after_sha256"], "after digest")):
            raise ValueError("transition change digest invalid")
    artifacts(root, item["evidence"], "transition evidence")
    kinds = {artifact["kind"] for artifact in item["evidence"]}
    if after == "IMPLEMENTED":
        build = exact(item["implementation"], {"revision", "base_tree", "command", "target", "result", "artifact"}, "implementation record")
        if not GIT.fullmatch(text(build["revision"], "implementation revision")) or not GIT.fullmatch(text(build["base_tree"], "implementation tree")) or build["result"] != "PASS":
            raise ValueError("implementation build provenance invalid")
        text(build["command"], "implementation command")
        text(build["target"], "implementation target")
        exact(build["artifact"], {"path", "sha256", "size_bytes", "kind"}, "implementation artifact")
        safe_file(root, build["artifact"]["path"], build["artifact"]["sha256"], build["artifact"]["size_bytes"], build["artifact"]["kind"])
        if build["artifact"]["kind"] != "artifact" or build["artifact"] not in item["evidence"]:
            raise ValueError("implementation needs bound successful build artifact")
    if after == "VERIFIED":
        runner = text(item["independent_runner"], "verification runner")
        if not any(artifact["kind"] == "log" for artifact in item["evidence"]):
            raise ValueError("verified transition needs raw test log")
        if runner in {"", "unverified"}:
            raise ValueError("verified transition needs independent runner")
    if after == "LEDGER_RECORDED":
        attestation = exact(item["attestation"], {"steward", "reviewer", "statement"}, "ledger attestation")
        if text(attestation["steward"], "attestation steward") == text(attestation["reviewer"], "attestation reviewer") or not text(attestation["statement"], "attestation statement"):
            raise ValueError("ledger attestation is not independent")
    states[phase] = after
    return phase, after


def history(root: dict, path: Path, digest, as_of) -> tuple[dict[int, str], dict[str, tuple[int, str]]]:
    """Verify hash chain, state digest, unique IDs, and replayed lifecycle facts."""
    events, prior, states, ids = root["events"], "GENESIS", {phase: "PLANNED" for phase in range(1, 9)}, {}
    if not isinstance(events, list) or not events:
        raise ValueError("history is empty")
    last_time = None
    for number, event in enumerate(events, 1):
        exact(event, {"sequence", "event_id", "previous_hash", "steward", "reviewer", "recorded_at", "action", "state_digest", "hash"}, "event")
        ident = text(event["event_id"], "event id")
        if ident in ids or integer(event["sequence"], "event sequence") != number or event["previous_hash"] != prior:
            raise ValueError("event identity or order invalid")
        if text(event["steward"], "steward") == text(event["reviewer"], "reviewer"):
            raise ValueError("event needs independent reviewer")
        recorded = timestamp(event["recorded_at"], "event time")
        if recorded > as_of or (last_time is not None and recorded <= last_time):
            raise ValueError("event timestamps must increase through as-of")
        phase, status = action(path, event["action"], states)
        if event["action"]["kind"] == "lifecycle_transition" and status == "VERIFIED" and event["action"]["independent_runner"] in {event["steward"], event["reviewer"]}:
            raise ValueError("verification runner is not independent")
        if event["action"]["kind"] == "lifecycle_transition" and status == "LEDGER_RECORDED":
            attestation = event["action"]["attestation"]
            if attestation["steward"] != event["steward"] or attestation["reviewer"] != event["reviewer"]:
                raise ValueError("ledger attestation does not bind event reviewers")
        if not HEX.fullmatch(text(event["state_digest"], "state digest")) or event["hash"] != canonical_digest({key: value for key, value in event.items() if key != "hash"}):
            raise ValueError("event hash invalid")
        ids[ident], prior = (phase, status), event["hash"]
        last_time = recorded
    prefix = exact(root["baseline_prefix"], {"event_count", "tip_hash"}, "baseline prefix")
    if not 1 <= integer(prefix["event_count"], "prefix count") <= len(events) or prefix["tip_hash"] != events[prefix["event_count"] - 1]["hash"] or events[-1]["state_digest"] != digest(root):
        raise ValueError("history does not bind full state")
    return states, ids
