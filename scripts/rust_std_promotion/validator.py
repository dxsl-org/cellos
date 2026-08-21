"""Fail-closed, fixture-only rust-std benchmark validator."""
from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from rust_std_promotion.schema_validation import SchemaError, reject_nonfinite, validate_schema

SCHEMA_PATH = Path(__file__).with_name("benchmark-run.schema.json")
SCHEMA = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
SCHEMA_VERSION = "rust-std-benchmark-run/v1"
VALIDATOR_VERSION = "rust-std-promotion-validator/v1"
ARMS = ("no_std_pre", "std", "no_std_post")
ROOT_KEYS = {"schema_version", "source_kind", "fixture_id", "requested_designation", "expected_cells", "runs"}
RUN_KEYS = {"schema_version", "run_id", "captured_at", "cell_id", "workload_id", "workload_version", "arm", "arm_order", "toolchain", "source_revision", "source_digest", "binary_digest", "runtime_kind", "build_profile", "codegen_flags_digest", "common_linker_inputs", "common_linker_inputs_digest", "runtime_linker_inputs", "runtime_linker_inputs_digest", "admission_manifest_digest", "capability_manifest_digest", "payload_digest", "operation_trace_digest", "environment", "protocol", "repetitions", "rejections", "summary", "provenance"}
WORKLOAD_DIGESTS = {
    "syscall-yield-v1": (hashlib.sha256(b"yield-empty-payload").hexdigest(), hashlib.sha256(b"yield:enter,sys_yield,return").hexdigest()),
    "ipc-echo-64-v1": (hashlib.sha256(bytes(range(64))).hexdigest(), hashlib.sha256(b"ipc:enter,send64,recv64,verify64,return").hexdigest()),
}
FIXTURE_COMMON_LINKER_INPUTS = [
    {"role": "linker-script", "identity": "cellos-benchmark-linker-script/v1", "digest": "4b810eb2108a421bfd1c3b4aa28844516b6702d3489d592ea2110fe97697a941"},
    {"role": "target-spec", "identity": "cellos-private-target-spec/v1", "digest": "76a868857c65ef1ce4a2b2ce3c8ff1f836c5b0195a1493dc9e8ec233c24aca35"},
    {"role": "compiler-builtins", "identity": "rust-compiler-builtins/f53b654a8", "digest": "e724458ee843dda78517a3a28175192c0305ad81641990250f45014cd9b929bc"},
]
FIXTURE_RUNTIME_LINKER_INPUTS = {
    "no_std": [
        {"role": "entrypoint", "identity": "cellos-ostd-entry/v1", "digest": "3dcdc2a6dd5b8975157833a867ba28c7c9791a077937c11e3a9484e8a82d95c4"},
        {"role": "runtime", "identity": "cellos-ostd-no-std-runtime/v1", "digest": "687358ba921782ac72070fd00260a74bfb84ffeb58fa768122cd031b190fd3b9"},
    ],
    "std": [
        {"role": "entrypoint", "identity": "cellos-rust-std-entry/v1", "digest": "182ec4c8a966084952386a4e4ce5da8eecde707eaf809a51f9e603f86d58a3f6"},
        {"role": "runtime", "identity": "cellos-rust-std-pal-runtime/v1", "digest": "13cbfae90c2fdc4db501ddc918d2bd79be8f3435f9cdc12b6bb31bb86c5ce101"},
    ],
}
TOOLCHAIN_KEYS = {"channel", "rustc_version", "commit_hash", "rust_src_digest"}
ENV_KEYS = {"architecture", "environment_kind", "board_model", "board_revision", "qemu_binary_digest", "qemu_version", "machine", "firmware_digest", "cpu_model", "cpu_count", "hart_count", "frequency_policy", "timer_source", "timer_frequency_hz", "target_spec_digest", "service_topology_digest", "service_state_digest"}
PROTOCOL_KEYS = {"warmup_count", "independent_rep_count", "operations_per_rep", "reset_rule", "predeclared_interference_codes"}
REP_KEYS = {"rep_id", "fresh_instance_id", "raw_latency_ns", "monotonic_clock", "interference"}
INTERFERENCE_KEYS = {"threshold_profile", "host_load", "steal_time", "thermal_throttle", "frequency_transition", "competing_process", "service_restart", "topology_change", "timer_anomaly"}
SUMMARY_KEYS = {"valid_n", "p50_ns", "p95_ns", "p99_ns"}
PROVENANCE_KEYS = {"producer", "schema_digest", "raw_digest"}
LINKER_INPUT_KEYS = {"role", "identity", "digest"}
INTERFERENCE_CODES = set(INTERFERENCE_KEYS) - {"threshold_profile"}
PARITY_PATHS = (("cell_id",), ("workload_id",), ("workload_version",), ("source_revision",), ("source_digest",), ("build_profile",), ("codegen_flags_digest",), ("common_linker_inputs",), ("common_linker_inputs_digest",), ("admission_manifest_digest",), ("capability_manifest_digest",), ("payload_digest",), ("operation_trace_digest",), *(("toolchain", k) for k in sorted(TOOLCHAIN_KEYS)), *(("environment", k) for k in sorted(ENV_KEYS)))

@dataclass(frozen=True)
class Result:
    report: dict[str, Any]
    exit_code: int

class Invalid(ValueError):
    pass

def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()

def _sha(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()

def _closed(value: Any, keys: set[str], name: str) -> None:
    if not isinstance(value, dict) or set(value) != keys:
        raise Invalid(f"{name}:fields")

def _at(run: dict[str, Any], path: tuple[str, ...]) -> Any:
    value: Any = run
    for part in path:
        value = value[part]
    return value

def _utc_timestamp(value: Any) -> datetime:
    if not isinstance(value, str) or not value.endswith("Z"):
        raise Invalid("captured_at:utc_required")
    try:
        stamp = datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError as error:
        raise Invalid("captured_at:invalid") from error
    return stamp.astimezone(timezone.utc)

def percentile(values: list[int], numerator: int, denominator: int = 100) -> int:
    if not values:
        raise Invalid("raw_samples:empty")
    ordered = sorted(values)
    rank = (numerator * len(ordered) + denominator - 1) // denominator
    return ordered[rank - 1]

def _linker_inputs(run: dict[str, Any]) -> None:
    for name in ("common_linker_inputs", "runtime_linker_inputs"):
        if not isinstance(run[name], list):
            raise Invalid(f"{name}:type")
        for item in run[name]:
            _closed(item, LINKER_INPUT_KEYS, name)
    expected_runtime = FIXTURE_RUNTIME_LINKER_INPUTS[run["runtime_kind"]]
    if run["common_linker_inputs"] != FIXTURE_COMMON_LINKER_INPUTS or run["runtime_linker_inputs"] != expected_runtime:
        raise Invalid("linker_inputs:not_pinned_allowlist")
    if run["common_linker_inputs_digest"] != _sha(run["common_linker_inputs"]) or run["runtime_linker_inputs_digest"] != _sha(run["runtime_linker_inputs"]):
        raise Invalid("linker_inputs:digest")

def _run(run: Any, schema_digest: str) -> tuple[dict[str, Any], list[str], str]:
    _closed(run, RUN_KEYS, "run")
    for value, keys, name in ((run["toolchain"], TOOLCHAIN_KEYS, "toolchain"), (run["environment"], ENV_KEYS, "environment"), (run["protocol"], PROTOCOL_KEYS, "protocol"), (run["summary"], SUMMARY_KEYS, "summary"), (run["provenance"], PROVENANCE_KEYS, "provenance")):
        _closed(value, keys, name)
    if run["schema_version"] != SCHEMA_VERSION or run["arm"] not in ARMS or run["arm_order"] != ARMS.index(run["arm"]) + 1:
        raise Invalid("schema_or_arm")
    expected_runtime = "std" if run["arm"] == "std" else "no_std"
    if run["runtime_kind"] != expected_runtime:
        raise Invalid("runtime_kind:arm_mismatch")
    _linker_inputs(run)
    expected_digests = WORKLOAD_DIGESTS.get(run["workload_id"])
    if run["workload_version"] != 1 or expected_digests != (run["payload_digest"], run["operation_trace_digest"]):
        raise Invalid("workload:identity_or_trace")
    if run["provenance"]["producer"] != "cellos-rust-std-synthetic-fixture-builder/v1" or run["provenance"]["schema_digest"] != schema_digest:
        raise Invalid("provenance:schema_or_producer")
    if run["protocol"]["warmup_count"] < 5 or run["protocol"]["operations_per_rep"] != 1:
        raise Invalid("protocol:warmups_or_operations")
    declared = run["protocol"]["predeclared_interference_codes"]
    if not isinstance(declared, list) or len(declared) != len(set(declared)) or not set(declared) <= INTERFERENCE_CODES:
        raise Invalid("interference:declarations")
    if run["rejections"]:
        raise Invalid("interference:document_invalidated")
    valid: list[int] = []
    ids, instances, threshold_profiles = set(), set(), set()
    for rep in run["repetitions"]:
        _closed(rep, REP_KEYS, "repetition"); _closed(rep["interference"], INTERFERENCE_KEYS, "interference")
        rid, instance, latency = rep["rep_id"], rep["fresh_instance_id"], rep["raw_latency_ns"]
        if rid in ids or instance in instances or not isinstance(latency, int) or isinstance(latency, bool) or latency <= 0 or rep["monotonic_clock"] is not True:
            raise Invalid("repetition:identity_or_latency")
        if any(not isinstance(rep["interference"][key], bool) for key in INTERFERENCE_CODES):
            raise Invalid("interference:type")
        if any(rep["interference"][key] for key in INTERFERENCE_CODES):
            raise Invalid("interference:document_invalidated")
        ids.add(rid); instances.add(instance); valid.append(latency); threshold_profiles.add(rep["interference"]["threshold_profile"])
    if len(threshold_profiles) != 1 or run["protocol"]["independent_rep_count"] != len(valid) or len(valid) < 30:
        raise Invalid("repetition:count_or_threshold_profile")
    calc = {"valid_n": len(valid), "p50_ns": percentile(valid, 50), "p95_ns": percentile(valid, 95), "p99_ns": percentile(valid, 99)}
    if run["summary"] != calc or run["environment"]["timer_frequency_hz"] <= 0:
        raise Invalid("summary_or_timer")
    if run["provenance"]["raw_digest"] != hashlib.sha256(canonical_bytes(run["repetitions"])).hexdigest():
        raise Invalid("provenance:raw_digest")
    return {"arm": run["arm"], "summary": calc, "raw_samples_ns": valid, "rejected_n": 0}, list(instances), threshold_profiles.pop()

def validate(document: Any, *, schema_digest: str) -> Result:
    reasons: list[str] = []
    cells: list[dict[str, Any]] = []
    try:
        validate_schema(document, SCHEMA); _closed(document, ROOT_KEYS, "document")
        if document["source_kind"] != "synthetic_fixture" or document["requested_designation"] != "fixture_validation_only":
            raise Invalid("fixture_only")
        if not isinstance(document["expected_cells"], list) or any(not isinstance(x, dict) or set(x) != {"cell_id", "workload_id"} for x in document["expected_cells"]):
            raise Invalid("cells:declaration")
        expected = [(x["cell_id"], x["workload_id"]) for x in document["expected_cells"]]
        if len(expected) != len(set(expected)) or len(document["runs"]) != len(expected) * len(ARMS):
            raise Invalid("cells:missing_or_unexpected")
        run_ids, rep_ids, instance_ids = set(), set(), set()
        for run in document["runs"]:
            if run["run_id"] in run_ids:
                raise Invalid("run:duplicate_id")
            run_ids.add(run["run_id"])
            for rep in run["repetitions"]:
                if rep["rep_id"] in rep_ids or rep["fresh_instance_id"] in instance_ids:
                    raise Invalid("repetition:not_globally_unique")
                rep_ids.add(rep["rep_id"]); instance_ids.add(rep["fresh_instance_id"])
        for index, key in enumerate(expected):
            runs = document["runs"][index * 3:index * 3 + 3]
            if any((run.get("cell_id"), run.get("workload_id")) != key for run in runs):
                raise Invalid("cells:physical_order")
            if tuple(run.get("arm") for run in runs) != ARMS or tuple(run.get("arm_order") for run in runs) != (1, 2, 3):
                raise Invalid("arms:physical_order")
            timestamps = [_utc_timestamp(run.get("captured_at")) for run in runs]
            if not timestamps[0] < timestamps[1] < timestamps[2]:
                raise Invalid("captured_at:not_strictly_increasing")
            first = runs[0]
            if any(_at(run, path) != _at(first, path) for run in runs[1:] for path in PARITY_PATHS):
                raise Invalid("parity:tuple")
            if any(run["protocol"][k] != first["protocol"][k] for run in runs[1:] for k in ("operations_per_rep", "reset_rule", "predeclared_interference_codes")):
                raise Invalid("parity:protocol")
            arm_data, all_instances, profiles = [], [], []
            for run in runs:
                data, instances, profile = _run(run, schema_digest); arm_data.append(data); all_instances.extend(instances); profiles.append(profile)
            if len(set(profiles)) != 1 or len(all_instances) != len(set(all_instances)):
                raise Invalid("parity:threshold_or_independence")
            pre, std, post = (x["summary"]["p99_ns"] for x in arm_data)
            if abs(post - pre) * 100 > pre * 2:
                raise Invalid("drift:gt_2pct")
            baseline = percentile(arm_data[0]["raw_samples_ns"] + arm_data[2]["raw_samples_ns"], 99)
            status = "VALID_PASS" if (std - baseline) * 100 <= baseline * 5 else "VALID_FAIL"
            cells.append({"cell_id": key[0], "workload_id": key[1], "status": status, "reasons": [] if status == "VALID_PASS" else ["regression:gt_5pct"], "baseline_p99_ns": baseline, "arms": arm_data})
        overall = "VALID_FAIL" if any(cell["status"] == "VALID_FAIL" for cell in cells) else "VALID_PASS"
        code = 1 if overall == "VALID_FAIL" else 0
    except (Invalid, SchemaError, KeyError, TypeError, IndexError) as error:
        overall, code, cells, reasons = "INVALID", 2, [], [str(error) or error.__class__.__name__]
    report = {"schema_version": SCHEMA_VERSION, "validator_version": VALIDATOR_VERSION, "schema_digest": schema_digest, "fixture_only": True, "promotion_eligible": False, "fixture_id": document.get("fixture_id") if isinstance(document, dict) else None, "canonical_input_digest": _sha(document), "overall_status": overall, "reasons": sorted(reasons), "cells": cells}
    return Result(report, code)
def load_and_validate(path: Path) -> Result:
    schema_digest = hashlib.sha256(SCHEMA_PATH.read_bytes()).hexdigest()
    document = json.loads(path.read_text(encoding="utf-8"), parse_constant=reject_nonfinite)
    return validate(document, schema_digest=schema_digest)
