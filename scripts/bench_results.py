#!/usr/bin/env python3
"""Collect, validate, and compare private Cellos benchmark captures."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import statistics
import selectors
import subprocess
import sys
import tempfile
from dataclasses import dataclass
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

SCHEMA = "cellos.benchmark.capture/v2"
STATE_SCHEMA = "cellos.benchmark.regression-state/v2"
PROFILE = "rv64-qemu-virt-2h-256m-v2"
THRESHOLD_PERCENT = 10.0
CONSECUTIVE_REQUIRED = 3
HISTORY_WINDOW = 20
HASH_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$")


@dataclass(frozen=True)
class MetricSpec:
    name: str
    n: int
    unit: str
    kind: str
    direction: str


METRICS = (
    MetricSpec("context_switch", 1000, "ns", "latency", "lower"),
    MetricSpec("ipc_send_recv", 1000, "ns", "latency", "lower"),
    MetricSpec("syscall_yield", 1000, "ns", "latency", "lower"),
    MetricSpec("memory_footprint", 1, "bytes", "footprint", "lower"),
    MetricSpec("preempt_latency", 500, "ns", "rt", "lower"),
    MetricSpec("control_loop", 200, "ns", "rt", "lower"),
    MetricSpec("ipc_send_recv_idle", 1000, "ns", "latency", "lower"),
    MetricSpec("syscall_yield_idle", 1000, "ns", "latency", "lower"),
    MetricSpec("ipc_send_recv_load", 1000, "ns", "latency", "lower"),
    MetricSpec("syscall_yield_load", 1000, "ns", "latency", "lower"),
    MetricSpec("smp_spawn_rate", 8, "operations/sec", "value", "higher"),
    MetricSpec("smp_ipc_throughput", 1000, "operations/sec", "value", "higher"),
    MetricSpec("smp_work_distribution", 2, "scale_x100", "value", "higher"),
    MetricSpec("stage_encode_request_x1000", 10, "ns per 1000 operations", "latency", "lower"),
    MetricSpec("stage_decode_reply_x1000", 10, "ns per 1000 operations", "latency", "lower"),
    MetricSpec("stage_ecall_roundtrip_x1000", 10, "ns per 1000 operations", "latency", "lower"),
    MetricSpec("total_typed_roundtrip_x1000", 10, "ns per 1000 operations", "latency", "lower"),
)
SPECS = {spec.name: spec for spec in METRICS}
PROFILE_CONTRACT = [
    {
        "name": spec.name,
        "n": spec.n,
        "unit": spec.unit,
        "kind": spec.kind,
        "direction": spec.direction,
    }
    for spec in METRICS
]
REQUIRED_ARTIFACTS = {"kernel", "bench", "probe", "disk", "ramdisk"}


class BenchError(ValueError):
    pass


def _reject_constant(value: str) -> None:
    raise BenchError(f"non-finite JSON number {value}")


def _finite_float(value: str) -> float:
    parsed = float(value)
    if not math.isfinite(parsed):
        raise BenchError(f"non-finite JSON number {value}")
    return parsed

def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise BenchError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def load_json_text(text: str) -> Any:
    return json.loads(
        text,
        parse_constant=_reject_constant,
        parse_float=_finite_float,
        object_pairs_hook=_unique_object,
    )


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def _plain_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= (2**64 - 1)

def _exit_code(value: Any) -> bool:
    return value is None or (isinstance(value, int) and not isinstance(value, bool) and -(2**31) <= value < 2**31)


def _parse_utc(value: Any) -> bool:
    if not isinstance(value, str) or not value.endswith("Z"):
        return False
    try:
        parsed = datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        return False
    return parsed.tzinfo is not None and parsed.utcoffset() == timezone.utc.utcoffset(parsed)


def _exact_keys(record: dict[str, Any], expected: set[str], label: str, errors: list[str]) -> None:
    actual = set(record)
    if actual != expected:
        errors.append(f"{label} fields must be {sorted(expected)}, got {sorted(actual)}")


def parse_guest_output(raw: str) -> tuple[list[dict[str, Any]], list[str]]:
    """Extract strict JSON objects from a multiplexed serial transcript.

    Cellos prefixes userspace output with ``USER:`` and kernel diagnostics may
    share the remainder of the same physical line.  The JSON object itself is
    still parsed strictly; only transport text before and after it is ignored.
    """
    records: list[dict[str, Any]] = []
    errors: list[str] = []
    decoder = json.JSONDecoder(
        parse_constant=_reject_constant,
        parse_float=_finite_float,
        object_pairs_hook=_unique_object,
    )
    for line_number, line in enumerate(raw.splitlines(), 1):
        object_start = line.find("{")
        if object_start < 0:
            continue
        candidate = line[object_start:]
        try:
            value, _ = decoder.raw_decode(candidate)
        except (json.JSONDecodeError, BenchError) as exc:
            if "bench_event" in candidate or '"name"' in candidate:
                errors.append(f"malformed JSON record on raw line {line_number}: {exc}")
            continue
        if not isinstance(value, dict):
            errors.append(f"JSON record on raw line {line_number} is not an object")
            continue
        if "bench_event" not in value and "name" not in value:
            errors.append(f"unknown JSON record on raw line {line_number}")
            continue
        records.append(value)
    return records, errors


def validate_records(records: Any, profile: str) -> list[str]:
    errors: list[str] = []
    if not isinstance(records, list):
        return ["records must be an array"]

    starts: list[tuple[int, dict[str, Any]]] = []
    completes: list[tuple[int, dict[str, Any]]] = []
    invalids: list[tuple[int, dict[str, Any]]] = []
    results: dict[str, dict[str, Any]] = {}

    for index, record in enumerate(records):
        label = f"record {index}"
        if not isinstance(record, dict):
            errors.append(f"{label} is not an object")
            continue
        if "bench_event" in record:
            event = record.get("bench_event")
            if event == "start":
                _exact_keys(record, {"bench_event", "profile"}, label, errors)
                starts.append((index, record))
                if record.get("profile") != profile:
                    errors.append(f"{label} start profile does not match capture profile")
            elif event == "complete":
                _exact_keys(record, {"bench_event", "profile", "invalid"}, label, errors)
                completes.append((index, record))
                if record.get("profile") != profile:
                    errors.append(f"{label} complete profile does not match capture profile")
                invalid = record.get("invalid")
                if not _plain_int(invalid) or invalid < 0:
                    errors.append(f"{label} invalid count must be a non-negative integer")
            elif event == "invalid":
                _exact_keys(record, {"bench_event", "scenario", "stage"}, label, errors)
                invalids.append((index, record))
                if not isinstance(record.get("scenario"), str) or not record.get("scenario"):
                    errors.append(f"{label} invalid scenario must be a non-empty string")
                if not isinstance(record.get("stage"), str) or not record.get("stage"):
                    errors.append(f"{label} invalid stage must be a non-empty string")
            else:
                errors.append(f"{label} has unknown bench_event {event!r}")
            continue

        name = record.get("name")
        if not isinstance(name, str) or name not in SPECS:
            errors.append(f"{label} has unknown metric name {name!r}")
            continue
        if name in results:
            errors.append(f"duplicate metric record {name}")
            continue
        results[name] = record

    if len(starts) != 1:
        errors.append(f"expected exactly one start event, found {len(starts)}")
    if len(completes) != 1:
        errors.append(f"expected exactly one complete event, found {len(completes)}")
    if starts and completes and starts[0][0] >= completes[0][0]:
        errors.append("start event must precede complete event")
    if starts:
        start_index = starts[0][0]
        if start_index != 0:
            errors.append("start event must be the first structured record")
    if completes:
        complete_index, complete = completes[0]
        if complete_index != len(records) - 1:
            errors.append("complete event must be the last structured record")
        invalid_count = complete.get("invalid")
        if _plain_int(invalid_count) and invalid_count != len(invalids):
            errors.append(
                f"complete invalid count {invalid_count} does not match {len(invalids)} invalid event(s)"
            )
        if invalid_count != 0:
            errors.append(f"producer reported {invalid_count!r} invalid scenario(s)")

    missing = sorted(set(SPECS) - set(results))
    if missing:
        errors.append(f"missing required metric records: {', '.join(missing)}")

    for name, record in results.items():
        spec = SPECS[name]
        if spec.kind == "latency":
            required = {"name", "n", "min", "p50", "p99", "max"}
            ordered = ("min", "p50", "p99", "max")
        elif spec.kind == "rt":
            required = {"name", "n", "min", "p50", "p99", "p999", "max", "jitter", "miss"}
            ordered = ("min", "p50", "p99", "p999", "max")
        elif spec.kind == "footprint":
            required = {"name", "n", "bytes"}
            ordered = ("bytes",)
        else:
            required = {"name", "n", "value"}
            ordered = ("value",)
        _exact_keys(record, required, f"metric {name}", errors)
        if record.get("n") != spec.n or not _plain_int(record.get("n")):
            errors.append(f"metric {name} n must be {spec.n}")
        values: list[int] = []
        for field in ordered:
            value = record.get(field)
            if not _plain_int(value) or value < 0:
                errors.append(f"metric {name} {field} must be a non-negative integer")
            else:
                values.append(value)
        if len(values) == len(ordered):
            if any(left > right for left, right in zip(values, values[1:])):
                errors.append(f"metric {name} latency statistics are not ordered")
            if values[-1] == 0:
                errors.append(f"metric {name} measurement must be greater than zero")
        if spec.kind == "rt":
            jitter = record.get("jitter")
            miss = record.get("miss")
            if not _plain_int(jitter) or jitter < 0:
                errors.append(f"metric {name} jitter must be a non-negative integer")
            elif _plain_int(record.get("max")) and _plain_int(record.get("min")) and jitter != record["max"] - record["min"]:
                errors.append(f"metric {name} jitter must equal max - min")
            if not _plain_int(miss) or miss < 0 or miss > spec.n:
                errors.append(f"metric {name} miss must be an integer between 0 and {spec.n}")

    return errors


def _partition_records(records: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    return (
        [record for record in records if "bench_event" in record],
        [record for record in records if "name" in record],
    )


def target_verdict(records: list[dict[str, Any]]) -> dict[str, Any]:
    by_name = {record["name"]: record for record in records if isinstance(record, dict) and record.get("name") in SPECS}
    rows: list[dict[str, Any]] = []

    def ceiling(name: str, field: str, limit: int, informational: bool = False) -> None:
        value = by_name[name][field]
        met = value <= limit
        status = ("INFORMATIONAL_MET" if met else "INFORMATIONAL_MISS") if informational else ("PASS" if met else "FAIL")
        rows.append({"name": name, "field": field, "value": value, "operator": "<=", "target": limit, "status": status})

    def floor(name: str, limit: int) -> None:
        value = by_name[name]["value"]
        rows.append({"name": name, "field": "value", "value": value, "operator": ">=", "target": limit, "status": "PASS" if value >= limit else "FAIL"})

    ceiling("context_switch", "p99", 100_000)
    ceiling("ipc_send_recv", "p99", 50_000, informational=True)
    ceiling("syscall_yield", "p99", 40_000)
    ceiling("memory_footprint", "bytes", 10 * 1024 * 1024)
    ceiling("preempt_latency", "p99", 200_000)
    rows.append({"name": "preempt_latency/deadline_miss", "field": "miss", "value": by_name["preempt_latency"]["miss"], "operator": "==", "target": 0, "status": "PASS" if by_name["preempt_latency"]["miss"] == 0 else "FAIL"})
    rows.append({"name": "control_loop/deadline_miss", "field": "miss", "value": by_name["control_loop"]["miss"], "operator": "==", "target": 0, "status": "PASS" if by_name["control_loop"]["miss"] == 0 else "FAIL"})
    floor("smp_spawn_rate", 10)
    floor("smp_ipc_throughput", 5_000)
    floor("smp_work_distribution", 140)
    return {"verdict": "FAIL" if any(row["status"] == "FAIL" for row in rows) else "PASS", "rows": rows}



def _normalized_qemu_command(command: list[str]) -> list[str] | None:
    """Return the frozen profile command with revision-specific paths normalized."""
    if len(command) != 18 or Path(command[0]).name != "qemu-system-riscv64":
        return None
    expected = {
        1: "-machine",
        2: "virt",
        3: "-accel",
        4: "tcg",
        5: "-smp",
        6: "2",
        7: "-nographic",
        8: "-bios",
        9: "default",
        10: "-kernel",
        12: "-m",
        13: "256M",
        14: "-drive",
        16: "-device",
        17: "virtio-blk-device,drive=hd0",
    }
    if any(command[index] != value for index, value in expected.items()):
        return None
    drive_parts = command[15].split(",")
    if (
        not command[11]
        or len(drive_parts) != 4
        or not drive_parts[0].startswith("file=")
        or not drive_parts[0][5:]
        or drive_parts[1:] != ["format=raw", "id=hd0", "if=none"]
    ):
        return None
    normalized = list(command)
    normalized[0] = "qemu-system-riscv64"
    normalized[11] = "<kernel>"
    normalized[15] = "file=<disk>,format=raw,id=hd0,if=none"
    return normalized


def validate_capture(document: Any, results_dir: Path, verify_raw: bool = True) -> list[str]:
    errors: list[str] = []
    if not isinstance(document, dict):
        return ["capture document must be an object"]
    _exact_keys(
        document,
        {"schema", "capture", "producer", "records", "events", "results", "validity", "target"},
        "capture document",
        errors,
    )
    if document.get("schema") != SCHEMA:
        errors.append(f"schema must be {SCHEMA}")
    capture = document.get("capture")
    if not isinstance(capture, dict):
        return errors + ["capture metadata must be an object"]
    _exact_keys(
        capture,
        {"id", "captured_at", "profile", "repetition", "profile_contract", "source", "build", "qemu", "artifacts"},
        "capture metadata",
        errors,
    )
    capture_id = capture.get("id")
    if not isinstance(capture_id, str) or not ID_RE.fullmatch(capture_id):
        errors.append("capture id is missing or unsafe")
    if not _parse_utc(capture.get("captured_at")):
        errors.append("captured_at must be an ISO-8601 UTC timestamp ending in Z")
    if capture.get("profile") != PROFILE:
        errors.append(f"profile must be {PROFILE}")
    if not _plain_int(capture.get("repetition")) or capture.get("repetition", 0) < 1:
        errors.append("repetition must be a positive integer")
    if capture.get("profile_contract") != PROFILE_CONTRACT:
        errors.append("profile contract has wrong units, counts, kinds, directions, or required metrics")

    source = capture.get("source")
    if not isinstance(source, dict):
        errors.append("source metadata must be an object")
    else:
        _exact_keys(source, {"repository", "ref", "commit", "inputs"}, "source metadata", errors)
        for field in ("repository", "ref", "commit"):
            if not isinstance(source.get(field), str) or not source[field]:
                errors.append(f"source {field} must be a non-empty string")
        inputs = source.get("inputs")
        if not isinstance(inputs, dict) or not inputs:
            errors.append("source inputs and hashes are required")
        elif any(not isinstance(name, str) or not HASH_RE.fullmatch(value) for name, value in inputs.items()):
            errors.append("source input hashes must be named lowercase SHA-256 values")

    build = capture.get("build")
    if not isinstance(build, dict):
        errors.append("build metadata must be an object")
    else:
        _exact_keys(build, {"toolchain", "features"}, "build metadata", errors)
        if not isinstance(build.get("toolchain"), str) or not build["toolchain"]:
            errors.append("build toolchain is required")
        features = build.get("features")
        if not isinstance(features, list) or not features or any(not isinstance(value, str) or not value for value in features):
            errors.append("build features must be a non-empty string array")

    qemu = capture.get("qemu")
    if not isinstance(qemu, dict):
        errors.append("QEMU metadata must be an object")
    else:
        _exact_keys(qemu, {"version", "command", "machine", "accelerator", "harts", "ram_mib"}, "QEMU metadata", errors)
        if not isinstance(qemu.get("version"), str) or not qemu["version"] or qemu["version"] == "unavailable":
            errors.append("actual QEMU version is required")
        command = qemu.get("command")
        if not isinstance(command, list) or not command or any(not isinstance(value, str) or not value for value in command):
            errors.append("actual QEMU command must be a non-empty string array")
        if qemu.get("machine") != "virt" or qemu.get("accelerator") != "tcg" or qemu.get("harts") != 2 or qemu.get("ram_mib") != 256:
            errors.append("QEMU environment does not match the frozen profile")
        if isinstance(command, list) and command:
            normalized_command = _normalized_qemu_command(command)
            if normalized_command is None:
                errors.append("recorded QEMU command does not match the frozen profile command")

    artifacts = capture.get("artifacts")
    if not isinstance(artifacts, dict) or set(artifacts) != REQUIRED_ARTIFACTS:
        errors.append(f"artifact metadata must contain exactly {sorted(REQUIRED_ARTIFACTS)}")
    else:
        for name, artifact in artifacts.items():
            if isinstance(artifact, dict):
                _exact_keys(artifact, {"path", "sha256"}, f"artifact {name}", errors)
            if not isinstance(artifact, dict) or not isinstance(artifact.get("path"), str) or not HASH_RE.fullmatch(artifact.get("sha256", "")):
                errors.append(f"artifact {name} must bind path and SHA-256")

    records = document.get("records")
    record_errors = validate_records(records, capture.get("profile"))
    errors.extend(record_errors)
    if isinstance(records, list):
        events, results = _partition_records([record for record in records if isinstance(record, dict)])
        if document.get("events") != events:
            errors.append("events do not match ordered raw records")
        if document.get("results") != results:
            errors.append("results do not match ordered raw records")

    producer = document.get("producer")
    if not isinstance(producer, dict):
        errors.append("producer metadata must be an object")
    else:
        _exact_keys(
            producer,
            {"status", "exit_code", "timed_out", "timeout_seconds", "launch_error", "raw_log", "stdin_line", "stdin_after"},
            "producer metadata",
            errors,
        )
        status = producer.get("status")
        if status not in {"completed", "intentional_termination_after_complete", "timeout_before_completion", "unexpected_exit", "launch_failed"}:
            errors.append("producer status is invalid")
        if status not in {"completed", "intentional_termination_after_complete"}:
            errors.append(f"producer output is incomplete: {status}")
        if not _plain_int(producer.get("timeout_seconds")) or producer.get("timeout_seconds", 0) <= 0:
            errors.append("producer timeout_seconds must be positive")
        exit_code = producer.get("exit_code")
        timed_out = producer.get("timed_out")
        launch_error = producer.get("launch_error")
        if not _exit_code(exit_code) or not isinstance(timed_out, bool):
            errors.append("producer exit_code/timed_out metadata is invalid")
        elif status == "completed" and (exit_code != 0 or timed_out or launch_error is not None):
            errors.append("completed producer status conflicts with exit/timeout metadata")
        elif status == "intentional_termination_after_complete" and (timed_out or launch_error is not None):
            errors.append("intentional termination status conflicts with exit/timeout metadata")
        elif status == "timeout_before_completion" and (not timed_out or launch_error is not None):
            errors.append("timeout status conflicts with exit/timeout metadata")
        elif status == "unexpected_exit" and (timed_out or exit_code is None or launch_error is not None):
            errors.append("unexpected-exit status conflicts with exit/timeout metadata")
        elif status == "launch_failed" and (timed_out or exit_code is not None or not isinstance(launch_error, str) or not launch_error):
            errors.append("launch-failed status conflicts with exit/timeout metadata")
        stdin_line = producer.get("stdin_line")
        if stdin_line is not None and (
            not isinstance(stdin_line, str)
            or not stdin_line
            or len(stdin_line.encode("utf-8")) > 128
            or any(character in stdin_line for character in "\r\n")
        ):
            errors.append("producer stdin_line must be one non-empty UTF-8 line of at most 128 bytes")
        stdin_after = producer.get("stdin_after")
        if stdin_after is not None and (
            not isinstance(stdin_after, str)
            or not stdin_after
            or len(stdin_after.encode("utf-8")) > 128
            or any(character in stdin_after for character in "\r\n")
        ):
            errors.append("producer stdin_after must be one non-empty UTF-8 marker of at most 128 bytes")
        if (stdin_line is None) != (stdin_after is None):
            errors.append("producer stdin_line and stdin_after must be present together")
        raw_meta = producer.get("raw_log")
        if isinstance(raw_meta, dict):
            _exact_keys(raw_meta, {"path", "sha256"}, "raw log metadata", errors)
        expected_raw_path = f"raw/{capture_id}.log" if isinstance(capture_id, str) else None
        if not isinstance(raw_meta, dict) or raw_meta.get("path") != expected_raw_path or not HASH_RE.fullmatch(raw_meta.get("sha256", "")):
            errors.append("raw log must use the capture-bound path and a SHA-256")
        elif verify_raw:
            raw_path = results_dir / raw_meta["path"]
            try:
                raw_bytes = raw_path.read_bytes()
            except OSError as exc:
                errors.append(f"raw log is unavailable: {exc}")
            else:
                if sha256_bytes(raw_bytes) != raw_meta["sha256"]:
                    errors.append("raw log SHA-256 does not match capture")
                else:
                    # Serial output is an exact byte artifact, not a UTF-8 text
                    # protocol. Decode one-to-one so interleaved multibyte human
                    # logs cannot invalidate intact ASCII JSON evidence records.
                    raw_text = raw_bytes.decode("latin-1")
                    parsed, parse_errors = parse_guest_output(raw_text)
                    errors.extend(parse_errors)
                    if parsed != records:
                        errors.append("structured records do not match retained raw log")
                    lowered = raw_text.lower()
                    if "panicked at" in lowered or "kernel panic" in lowered or "[panic]" in lowered or "panic:" in lowered:
                        errors.append("raw producer output contains a panic")

    if not record_errors and isinstance(records, list):
        expected_target = target_verdict(records)
        if document.get("target") != expected_target:
            errors.append("stored target verdict does not match validated records")

    expected_validity = "VALID" if not errors else "INVALID"
    validity = document.get("validity")
    if not isinstance(validity, dict) or validity.get("verdict") != expected_validity or validity.get("errors") != errors:
        errors.append("stored validity verdict does not match capture validation")
    return errors


def _core_validate_capture(document: dict[str, Any], results_dir: Path) -> list[str]:
    saved = document.get("validity")
    document["validity"] = {"verdict": "VALID", "errors": []}
    errors = validate_capture(document, results_dir)
    if errors and errors[-1] == "stored validity verdict does not match capture validation":
        errors.pop()
    document["validity"] = saved
    return errors


def _parse_mapping(values: Iterable[str], label: str) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for value in values:
        name, separator, raw_path = value.partition("=")
        if not separator or not name or not raw_path or name in result:
            raise BenchError(f"{label} must use unique NAME=PATH entries")
        result[name] = Path(raw_path)
    return result


def _qemu_version(executable: str) -> str:
    try:
        completed = subprocess.run(
            [executable, "--version"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=15,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return "unavailable"
    first_line = completed.stdout.splitlines()
    return first_line[0].strip() if completed.returncode == 0 and first_line else "unavailable"


def _exclusive_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    except FileExistsError:
        if path.read_bytes() == data:
            return
        raise BenchError(f"conflicting immutable capture path already exists: {path}")
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def collect(args: argparse.Namespace) -> int:
    if not ID_RE.fullmatch(args.capture_id):
        raise BenchError("capture id contains unsafe characters")
    if args.profile != PROFILE:
        raise BenchError(f"collector only supports frozen profile {PROFILE}")
    if not args.command:
        raise BenchError("QEMU command is required after --")
    command = args.command[1:] if args.command[0] == "--" else args.command
    if not command:
        raise BenchError("QEMU command is required after --")
    if args.repetition < 1:
        raise BenchError("repetition must be positive")
    if args.timeout_seconds <= 0:
        raise BenchError("timeout must be positive")
    if args.stdin_line is not None and (
        not args.stdin_line
        or len(args.stdin_line.encode("utf-8")) > 128
        or any(character in args.stdin_line for character in "\r\n")
    ):
        raise BenchError("stdin-line must be one non-empty UTF-8 line of at most 128 bytes")
    if not args.toolchain or not args.feature or any(not feature for feature in args.feature):
        raise BenchError("toolchain and at least one non-empty feature are required")
    if args.stdin_after is not None and (
        not args.stdin_after
        or len(args.stdin_after.encode("utf-8")) > 128
        or any(character in args.stdin_after for character in "\r\n")
    ):
        raise BenchError("stdin-after must be one non-empty UTF-8 marker of at most 128 bytes")
    if (args.stdin_line is None) != (args.stdin_after is None):
        raise BenchError("stdin-line and stdin-after must be supplied together")

    artifacts = _parse_mapping(args.artifact, "artifact")
    if set(artifacts) != REQUIRED_ARTIFACTS:
        raise BenchError(f"artifacts must contain exactly {sorted(REQUIRED_ARTIFACTS)}")
    source_inputs = _parse_mapping(args.source_input, "source input")
    if not source_inputs:
        raise BenchError("at least one source input hash is required")
    for path in [*artifacts.values(), *source_inputs.values()]:
        if not path.is_file():
            raise BenchError(f"hash input is not a file: {path}")
    source_hashes = {name: sha256_file(path) for name, path in sorted(source_inputs.items())}
    artifact_metadata = {
        name: {"path": str(path), "sha256": sha256_file(path)}
        for name, path in sorted(artifacts.items())
    }
    captured_at = utc_now()

    results_dir = Path(args.results_dir)
    raw_relative = Path("raw") / f"{args.capture_id}.log"
    raw_path = results_dir / raw_relative
    result_path = results_dir / f"perf-{args.capture_id}.json"
    if raw_path.exists() or result_path.exists():
        raise BenchError(f"capture id {args.capture_id!r} already exists; refusing to overwrite immutable evidence")
    raw_path.parent.mkdir(parents=True, exist_ok=True)

    qemu_version = _qemu_version(command[0])
    timed_out = False
    completion_observed = False
    return_code: int | None = None
    launch_error: str | None = None
    with raw_path.open("xb") as raw_stream:
        try:
            process = subprocess.Popen(
                command,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
            )
        except OSError as exc:
            launch_error = str(exc)
            raw_stream.write(f"[collector] producer launch failed: {exc}\n".encode())
        else:
            assert process.stdout is not None
            assert process.stdin is not None
            trigger = args.stdin_after.encode("utf-8") if args.stdin_after else None
            command_line = (args.stdin_line + "\n").encode("utf-8") if args.stdin_line else None
            sent = command_line is None
            recent = bytearray()
            deadline = time.monotonic() + args.timeout_seconds
            selector = selectors.DefaultSelector()
            selector.register(process.stdout, selectors.EVENT_READ)
            try:
                while process.poll() is None and not completion_observed:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        timed_out = True
                        break
                    for key, _ in selector.select(min(0.25, remaining)):
                        chunk = os.read(key.fileobj.fileno(), 4096)
                        if not chunk:
                            continue
                        raw_stream.write(chunk)
                        recent.extend(chunk)
                        if len(recent) > 4096:
                            del recent[:-4096]
                        if not sent and trigger in recent:
                            process.stdin.write(command_line)
                            process.stdin.flush()
                            sent = True
                        complete_index = recent.rfind(b'{"bench_event":"complete"')
                        if complete_index >= 0 and b"\n" in recent[complete_index:]:
                            completion_observed = True
                            break
                if timed_out or completion_observed:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                remainder = process.stdout.read()
                if remainder:
                    raw_stream.write(remainder)
                return_code = process.returncode
            finally:
                selector.close()
        raw_stream.flush()
        os.fsync(raw_stream.fileno())

    raw_bytes = raw_path.read_bytes()
    # UART output is byte-oriented and may interleave multibyte human log
    # messages. Structured benchmark records are deliberately ASCII JSON.
    raw_text = raw_bytes.decode("latin-1")
    records, parse_errors = parse_guest_output(raw_text)
    guest_errors = parse_errors + validate_records(records, args.profile)
    panic = any(marker in raw_text.lower() for marker in ("panicked at", "kernel panic", "[panic]", "panic:"))
    guest_complete = not guest_errors and not panic
    if launch_error is not None:
        producer_status = "launch_failed"
    elif completion_observed:
        producer_status = (
            "intentional_termination_after_complete" if guest_complete else "unexpected_exit"
        )
    elif timed_out:
        producer_status = "timeout_before_completion"
    elif return_code == 0 and guest_complete:
        producer_status = "completed"
    else:
        producer_status = "unexpected_exit"


    events, results = _partition_records(records)
    document: dict[str, Any] = {
        "schema": SCHEMA,
        "capture": {
            "id": args.capture_id,
            "captured_at": captured_at,
            "profile": args.profile,
            "repetition": args.repetition,
            "profile_contract": PROFILE_CONTRACT,
            "source": {
                "repository": args.repository,
                "ref": args.source_ref,
                "commit": args.commit,
                "inputs": source_hashes,
            },
            "build": {"toolchain": args.toolchain, "features": sorted(set(args.feature))},
            "qemu": {
                "version": qemu_version,
                "command": command,
                "machine": "virt",
                "accelerator": "tcg",
                "harts": 2,
                "ram_mib": 256,
            },
            "artifacts": artifact_metadata,
        },
        "producer": {
            "status": producer_status,
            "exit_code": return_code,
            "timed_out": timed_out,
            "timeout_seconds": args.timeout_seconds,
            "launch_error": launch_error,
            "stdin_line": args.stdin_line,
            "stdin_after": args.stdin_after,
            "raw_log": {"path": raw_relative.as_posix(), "sha256": sha256_bytes(raw_bytes)},
        },
        "records": records,
        "events": events,
        "results": results,
        "validity": {"verdict": "INVALID", "errors": []},
        "target": target_verdict(records) if not guest_errors else {"verdict": "NOT_EVALUATED", "rows": []},
    }
    errors = _core_validate_capture(document, results_dir)
    document["validity"] = {"verdict": "VALID" if not errors else "INVALID", "errors": errors}
    _exclusive_bytes(result_path, canonical_bytes(document))

    print(f"[collect] raw log: {raw_path}")
    print(f"[collect] capture: {result_path}")
    print(f"[collect] producer: {producer_status} (exit={return_code}, timeout={timed_out})")
    print(f"[collect] validity: {document['validity']['verdict']}")
    for line in raw_text.splitlines():
        if line.startswith(("[bench]", "[rt]", "[smp]", "[breakdown]")) or line.lstrip().startswith("{"):
            print(line)
    return 0


def _capture_digest(document: dict[str, Any]) -> str:
    return sha256_bytes(canonical_bytes(document))


def _compatibility_key(document: dict[str, Any]) -> str:
    capture = document["capture"]
    value = {
        "profile": capture["profile"],
        "profile_contract": capture["profile_contract"],
        "toolchain": capture["build"],
        "qemu": {
            key: capture["qemu"][key]
            for key in ("version", "machine", "accelerator", "harts", "ram_mib")
        },
        "qemu_command": _normalized_qemu_command(capture["qemu"]["command"]),
    }
    return sha256_bytes(canonical_bytes(value))


def _observations(document: dict[str, Any]) -> dict[str, tuple[float, str]]:
    observations: dict[str, tuple[float, str]] = {}
    for record in document["results"]:
        spec = SPECS[record["name"]]
        if spec.kind in {"latency", "rt"}:
            observations[f"{spec.name}/p99"] = (float(record["p99"]), "lower")
            if spec.kind == "rt":
                observations[f"{spec.name}/p999"] = (float(record["p999"]), "lower")
                observations[f"{spec.name}/jitter"] = (float(record["jitter"]), "lower")
                observations[f"{spec.name}/miss"] = (float(record["miss"]), "lower")
        elif spec.kind == "footprint":
            observations[f"{spec.name}/bytes"] = (float(record["bytes"]), "lower")
        else:
            observations[f"{spec.name}/value"] = (float(record["value"]), "higher")
    return observations


def _is_regression(current: float, baseline: float, direction: str) -> tuple[bool, float | None]:
    if baseline == 0:
        if direction == "lower":
            return current > 0, None
        return False, None
    change = ((current - baseline) / baseline) * 100.0
    return (change > THRESHOLD_PERCENT if direction == "lower" else change < -THRESHOLD_PERCENT), change


def replay_group(documents: list[dict[str, Any]]) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    ordered = sorted(documents, key=lambda item: (item["capture"]["captured_at"], item["capture"]["id"]))
    values: dict[str, list[float]] = {}
    streaks: dict[str, int] = {}
    outcomes: dict[str, dict[str, Any]] = {}
    for document in ordered:
        capture_id = document["capture"]["id"]
        rows: list[dict[str, Any]] = []
        observations = _observations(document)
        for metric in sorted(observations):
            current, direction = observations[metric]
            prior = values.get(metric, [])[-HISTORY_WINDOW:]
            if not prior:
                streaks[metric] = 0
                rows.append({"metric": metric, "status": "BASELINE_ONLY", "current": current, "median": None, "change_percent": None, "streak": 0})
                values.setdefault(metric, []).append(current)
            else:
                baseline = float(statistics.median(prior))
                bad, change = _is_regression(current, baseline, direction)
                streaks[metric] = streaks.get(metric, 0) + 1 if bad else 0
                status = "FAIL" if streaks[metric] >= CONSECUTIVE_REQUIRED else ("REGRESSION" if bad else "PASS")
                rows.append({"metric": metric, "status": status, "current": current, "median": baseline, "change_percent": change, "streak": streaks[metric]})
                # Candidate regressions cannot move the baseline used to decide
                # whether their own streak is sustained.
                if not bad:
                    values.setdefault(metric, []).append(current)
        if all(row["status"] == "BASELINE_ONLY" for row in rows):
            verdict = "BASELINE_ONLY"
        elif any(row["status"] == "FAIL" for row in rows):
            verdict = "FAIL"
        else:
            verdict = "PASS"
        outcomes[capture_id] = {"verdict": verdict, "rows": rows}
    state = {
        "profile": ordered[-1]["capture"]["profile"],
        "processed_run_ids": [item["capture"]["id"] for item in ordered],
        "metrics": {metric: {"streak": streaks.get(metric, 0), "values": series[-HISTORY_WINDOW:]} for metric, series in sorted(values.items())},
        "last_history_verdict": outcomes[ordered[-1]["capture"]["id"]]["verdict"],
    }
    return state, outcomes


def _read_state(path: Path) -> tuple[str, dict[str, Any] | None]:
    if not path.exists():
        return "missing", None
    try:
        value = load_json_text(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError, BenchError):
        return "corrupt", None
    if (
        not isinstance(value, dict)
        or set(value) != {
            "schema",
            "threshold_percent",
            "consecutive_required",
            "history_window",
            "captures",
            "groups",
        }
        or value.get("schema") != STATE_SCHEMA
        or value.get("threshold_percent") != THRESHOLD_PERCENT
        or value.get("consecutive_required") != CONSECUTIVE_REQUIRED
        or value.get("history_window") != HISTORY_WINDOW
        or not isinstance(value.get("captures"), list)
        or not isinstance(value.get("groups"), dict)
    ):
        return "corrupt", None
    for capture in value["captures"]:
        if (
            not isinstance(capture, dict)
            or set(capture) != {"id", "sha256"}
            or not isinstance(capture.get("id"), str)
            or not HASH_RE.fullmatch(capture.get("sha256", ""))
        ):
            return "corrupt", None
    for key, group in value["groups"].items():
        if (
            not HASH_RE.fullmatch(key)
            or not isinstance(group, dict)
            or not isinstance(group.get("profile"), str)
            or not isinstance(group.get("processed_run_ids"), list)
            or any(not isinstance(run_id, str) for run_id in group.get("processed_run_ids", []))
            or not isinstance(group.get("metrics"), dict)
            or group.get("last_history_verdict") not in {"BASELINE_ONLY", "PASS", "FAIL"}
        ):
            return "corrupt", None
    return "valid", value


def _atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory_fd = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def compare(args: argparse.Namespace) -> int:
    results_dir = Path(args.results_dir).resolve()
    current_path = Path(args.current).resolve()
    state_path = results_dir / "regression-state.json"
    if not results_dir.is_dir():
        print(f"[compare] VALIDITY: INVALID — results directory not found: {results_dir}")
        return 1
    if current_path.parent != results_dir or not current_path.is_file():
        print("[compare] VALIDITY: INVALID — --current must select an existing capture directly in results-dir")
        return 1

    state_status, old_state = _read_state(state_path)
    loaded: list[tuple[Path, dict[str, Any]]] = []
    malformed_history: list[str] = []
    for path in sorted(results_dir.glob("perf-*.json")):
        try:
            value = load_json_text(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError, BenchError) as exc:
            if path == current_path:
                print(f"[compare] VALIDITY: INVALID — malformed current capture: {exc}")
                return 1
            malformed_history.append(path.name)
            continue
        if not isinstance(value, dict):
            if path == current_path:
                print("[compare] VALIDITY: INVALID — current capture is not a JSON object")
                return 1
            malformed_history.append(path.name)
            continue
        loaded.append((path, value))

    current_matches = [value for path, value in loaded if path == current_path]
    if len(current_matches) != 1:
        print("[compare] VALIDITY: INVALID — explicitly selected current capture was not loaded exactly once")
        return 1
    current = current_matches[0]
    actual_id = current.get("capture", {}).get("id") if isinstance(current.get("capture"), dict) else None
    if actual_id != args.current_id:
        print(f"[compare] VALIDITY: INVALID — current identity mismatch: expected {args.current_id!r}, got {actual_id!r}")
        return 1

    by_id: dict[str, tuple[dict[str, Any], str]] = {}
    conflicts: set[str] = set()
    for _, document in loaded:
        capture = document.get("capture")
        capture_id = capture.get("id") if isinstance(capture, dict) else None
        if not isinstance(capture_id, str):
            continue
        digest = _capture_digest(document)
        if capture_id in by_id and by_id[capture_id][1] != digest:
            conflicts.add(capture_id)
        else:
            by_id[capture_id] = (document, digest)
    if conflicts:
        print(f"[compare] VALIDITY: INVALID — conflicting duplicate capture IDs: {', '.join(sorted(conflicts))}")
        return 1

    current_errors = validate_capture(current, results_dir)
    if current_errors:
        print("[compare] VALIDITY: INVALID")
        for error in current_errors:
            print(f"[compare]   {error}")
        print("[compare] TARGET: NOT_EVALUATED")
        print("[compare] HISTORY: NOT_EVALUATED")
        return 1
    print(f"[compare] CURRENT: {actual_id}")
    print("[compare] VALIDITY: VALID")
    target = target_verdict(current["records"])
    print(f"[compare] TARGET: {target['verdict']}")
    for row in target["rows"]:
        print(f"[compare]   {row['name']}: {row['status']} ({row['value']} {row['operator']} {row['target']})")

    valid_documents: list[dict[str, Any]] = []
    invalid_same_profile = False
    for _, document in loaded:
        capture = document.get("capture")
        same_profile = isinstance(capture, dict) and capture.get("profile") == PROFILE
        if validate_capture(document, results_dir):
            if same_profile and document is not current:
                invalid_same_profile = True
            continue
        if all(document["capture"]["id"] != seen["capture"]["id"] for seen in valid_documents):
            valid_documents.append(document)

    current_key = _compatibility_key(current)
    groups: dict[str, list[dict[str, Any]]] = {}
    for document in valid_documents:
        groups.setdefault(_compatibility_key(document), []).append(document)
    current_group = groups[current_key]
    prior_compatible = [document for document in current_group if document["capture"]["id"] != actual_id and (document["capture"]["captured_at"], document["capture"]["id"]) < (current["capture"]["captured_at"], actual_id)]

    known_current = False
    known_profile = False
    if state_status == "valid" and old_state is not None:
        for group in old_state["groups"].values():
            if isinstance(group, dict) and group.get("profile") == PROFILE:
                known_profile = True
                if actual_id in group.get("processed_run_ids", []):
                    known_current = True
    same_profile_other = any(
        document["capture"].get("profile") == PROFILE and document["capture"]["id"] != actual_id
        for document in valid_documents
    ) or invalid_same_profile

    if not prior_compatible and not known_current and (same_profile_other or known_profile or state_status == "corrupt" or malformed_history):
        print("[compare] HISTORY: INVALID/BLOCKED — existing profile has no reconstructable compatible prior-run state")
        if malformed_history:
            print(f"[compare]   non-comparable malformed history: {', '.join(malformed_history)}")
        return 1

    state_groups: dict[str, Any] = {}
    outcomes_by_group: dict[str, dict[str, dict[str, Any]]] = {}
    for key, documents in sorted(groups.items()):
        group_state, outcomes = replay_group(documents)
        state_groups[key] = group_state
        outcomes_by_group[key] = outcomes
    outcome = outcomes_by_group[current_key][actual_id]
    print(f"[compare] HISTORY: {outcome['verdict']}")
    for row in outcome["rows"]:
        if row["status"] != "PASS":
            print(f"[compare]   {row['metric']}: {row['status']} current={row['current']:.0f} median={row['median']} streak={row['streak']}")
    if state_status != "valid":
        print(f"[compare] STATE: reconstructed from immutable captures ({state_status})")
    if malformed_history:
        print(f"[compare] HISTORY NOTE: ignored non-comparable malformed files: {', '.join(malformed_history)}")

    new_state = {
        "schema": STATE_SCHEMA,
        "threshold_percent": THRESHOLD_PERCENT,
        "consecutive_required": CONSECUTIVE_REQUIRED,
        "history_window": HISTORY_WINDOW,
        "captures": [
            {"id": document["capture"]["id"], "sha256": _capture_digest(document)}
            for document in sorted(valid_documents, key=lambda item: (item["capture"]["captured_at"], item["capture"]["id"]))
        ],
        "groups": state_groups,
    }
    _atomic_write(state_path, canonical_bytes(new_state))
    return 1 if target["verdict"] == "FAIL" or outcome["verdict"] == "FAIL" else 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="operation", required=True)

    collect_parser = subparsers.add_parser("collect", help="run QEMU and serialize one immutable capture")
    collect_parser.add_argument("--results-dir", required=True)
    collect_parser.add_argument("--capture-id", required=True)
    collect_parser.add_argument("--profile", required=True)
    collect_parser.add_argument("--repetition", type=int, required=True)
    collect_parser.add_argument("--repository", required=True)
    collect_parser.add_argument("--source-ref", required=True)
    collect_parser.add_argument("--commit", required=True)
    collect_parser.add_argument("--toolchain", required=True)
    collect_parser.add_argument("--feature", action="append", required=True)
    collect_parser.add_argument("--source-input", action="append", default=[])
    collect_parser.add_argument("--artifact", action="append", default=[])
    collect_parser.add_argument("--timeout-seconds", type=int, default=300)
    collect_parser.add_argument(
        "--stdin-line",
        help="send one UTF-8 command line after --stdin-after appears",
    )
    collect_parser.add_argument(
        "--stdin-after",
        help="wait for this UTF-8 producer output marker before sending --stdin-line",
    )
    collect_parser.add_argument("command", nargs=argparse.REMAINDER)
    collect_parser.set_defaults(handler=collect)

    compare_parser = subparsers.add_parser("compare", help="validate explicit current capture and replay compatible history")
    compare_parser.add_argument("results_dir")
    compare_parser.add_argument("--current", required=True)
    compare_parser.add_argument("--current-id", required=True)
    compare_parser.set_defaults(handler=compare)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.handler(args)
    except BenchError as exc:
        print(f"[bench-results] ERROR: {exc}", file=sys.stderr)
        return 2
    except OSError as exc:
        print(f"[bench-results] ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
