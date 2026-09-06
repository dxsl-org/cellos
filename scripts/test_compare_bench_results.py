#!/usr/bin/env python3
"""Subprocess regression tests for the private benchmark capture contract."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import subprocess
import tempfile
import sys
import unittest
from pathlib import Path
from typing import Any

SCRIPTS = Path(__file__).resolve().parent
WRAPPER = SCRIPTS / "compare-bench-results.sh"
SPEC = importlib.util.spec_from_file_location("bench_results", SCRIPTS / "bench_results.py")
assert SPEC is not None and SPEC.loader is not None
BENCH = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = BENCH
SPEC.loader.exec_module(BENCH)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def result_record(spec: Any, p99: int = 100) -> dict[str, Any]:
    if spec.kind == "latency":
        return {"name": spec.name, "n": spec.n, "min": 10, "p50": 50, "p99": p99, "max": max(200, p99)}
    if spec.kind == "rt":
        maximum = max(200, p99)
        return {
            "name": spec.name,
            "n": spec.n,
            "min": 10,
            "p50": 50,
            "p99": p99,
            "p999": max(p99, 150),
            "max": maximum,
            "jitter": maximum - 10,
            "miss": 0,
        }
    if spec.kind == "footprint":
        return {"name": spec.name, "n": spec.n, "bytes": 1024}
    values = {
        "smp_spawn_rate": 20,
        "smp_ipc_throughput": 10_000,
        "smp_work_distribution": 180,
    }
    return {"name": spec.name, "n": spec.n, "value": values[spec.name]}


def write_capture(
    root: Path,
    capture_id: str,
    order: int,
    *,
    stage_p99: int = 100,
    qemu_version: str = "QEMU emulator version 10.0.0",
    mutate_records: Any = None,
    producer_status: str = "completed",
) -> Path:
    records: list[dict[str, Any]] = [
        {"bench_event": "start", "profile": BENCH.PROFILE},
        *[
            result_record(spec, stage_p99 if spec.name == "stage_encode_request_x1000" else 100)
            for spec in BENCH.METRICS
        ],
        {"bench_event": "complete", "profile": BENCH.PROFILE, "invalid": 0},
    ]
    if mutate_records is not None:
        mutate_records(records)
    raw = "\n".join(json.dumps(record, separators=(",", ":")) for record in records).encode() + b"\n"
    raw_relative = Path("raw") / f"{capture_id}.log"
    raw_path = root / raw_relative
    raw_path.parent.mkdir(parents=True, exist_ok=True)
    raw_path.write_bytes(raw)
    events = [record for record in records if "bench_event" in record]
    results = [record for record in records if "name" in record]
    document = {
        "schema": BENCH.SCHEMA,
        "capture": {
            "id": capture_id,
            "captured_at": f"2026-09-{order:02d}T12:00:00Z",
            "profile": BENCH.PROFILE,
            "repetition": 1,
            "profile_contract": BENCH.PROFILE_CONTRACT,
            "source": {
                "repository": "example/cellos",
                "ref": "refs/heads/main",
                "commit": f"commit-{capture_id}",
                "inputs": {"Cargo.lock": "1" * 64},
            },
            "build": {"toolchain": "nightly-2026-05-01", "features": ["release-default"]},
            "qemu": {
                "version": qemu_version,
                "command": [
                    "qemu-system-riscv64",
                    "-machine",
                    "virt",
                    "-accel",
                    "tcg",
                    "-smp",
                    "2",
                    "-nographic",
                    "-bios",
                    "default",
                    "-kernel",
                    "kernel",
                    "-m",
                    "256M",
                    "-drive",
                    "file=disk,format=raw,id=hd0,if=none",
                    "-device",
                    "virtio-blk-device,drive=hd0",
                ],
                "machine": "virt",
                "accelerator": "tcg",
                "harts": 2,
                "ram_mib": 256,
            },
            "artifacts": {
                name: {"path": name, "sha256": str(index) * 64}
                for index, name in enumerate(sorted(BENCH.REQUIRED_ARTIFACTS), 1)
            },
        },
        "producer": {
            "status": producer_status,
            "exit_code": 0 if producer_status == "completed" else 1,
            "timed_out": False,
            "timeout_seconds": 300,
            "launch_error": None,
            "stdin_line": None,
            "stdin_after": None,
            "raw_log": {"path": raw_relative.as_posix(), "sha256": digest(raw)},
        },
        "records": records,
        "events": events,
        "results": results,
        "validity": {"verdict": "VALID", "errors": []},
        "target": BENCH.target_verdict(records) if set(BENCH.SPECS) == {record.get("name") for record in results} else {"verdict": "NOT_EVALUATED", "rows": []},
    }
    path = root / f"perf-{capture_id}.json"
    path.write_text(json.dumps(document, sort_keys=True), encoding="utf-8")
    return path


class ComparatorContractTests(unittest.TestCase):
    def compare(self, root: Path, current: Path, expected_id: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(WRAPPER), str(root), "--current", str(current), "--current-id", expected_id],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            check=False,
        )

    def test_empty_current_fails_even_with_valid_history(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "history", 1)
            current = root / "perf-current.json"
            current.write_text("", encoding="utf-8")
            completed = self.compare(root, current, "current")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("malformed current capture", completed.stdout)

    def test_malformed_current_fails_even_with_valid_history(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "history", 1)
            current = root / "perf-current.json"
            current.write_text('{"capture":', encoding="utf-8")
            completed = self.compare(root, current, "current")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("VALIDITY: INVALID", completed.stdout)

    def test_missing_and_duplicate_required_records_are_rejected(self) -> None:
        cases = {
            "missing": lambda records: records.pop(1),
            "duplicate": lambda records: records.insert(-1, dict(records[1])),
        }
        for name, mutation in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                write_capture(root, f"history-{name}", 1)
                current = write_capture(root, name, 2, mutate_records=mutation)
                completed = self.compare(root, current, name)
                self.assertNotEqual(completed.returncode, 0)
                self.assertIn("VALIDITY: INVALID", completed.stdout)
                self.assertIn("metric", completed.stdout)

    def test_wrong_count_and_profile_units_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def wrong_count(records: list[dict[str, Any]]) -> None:
                records[1]["n"] = 999

            current = write_capture(root, "wrong-count", 1, mutate_records=wrong_count)
            completed = self.compare(root, current, "wrong-count")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("n must be 1000", completed.stdout)

            document = json.loads(current.read_text(encoding="utf-8"))
            document["capture"]["profile_contract"][0]["unit"] = "ticks"
            current.write_text(json.dumps(document), encoding="utf-8")
            completed = self.compare(root, current, "wrong-count")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("wrong units", completed.stdout)

    def test_explicit_current_identity_is_required(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "actual", 1)
            completed = self.compare(root, current, "different")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("current identity mismatch", completed.stdout)

    def test_first_valid_profile_is_baseline_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "first", 1)
            completed = self.compare(root, current, "first")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("VALIDITY: VALID", completed.stdout)
            self.assertIn("TARGET: PASS", completed.stdout)
            self.assertIn("HISTORY: BASELINE_ONLY", completed.stdout)

    def test_repeated_same_id_does_not_advance_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "same", 1)
            first = self.compare(root, current, "same")
            self.assertEqual(first.returncode, 0, first.stdout)
            state = (root / "regression-state.json").read_bytes()
            second = self.compare(root, current, "same")
            self.assertEqual(second.returncode, 0, second.stdout)
            self.assertEqual((root / "regression-state.json").read_bytes(), state)
            parsed = json.loads(state)
            group = next(iter(parsed["groups"].values()))
            self.assertEqual(group["processed_run_ids"], ["same"])

    def test_corrupt_state_reconstructs_and_third_distinct_bad_run_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "base", 1, stage_p99=100)
            write_capture(root, "bad-1", 2, stage_p99=120)
            write_capture(root, "bad-2", 3, stage_p99=140)
            current = write_capture(root, "bad-3", 4, stage_p99=160)
            (root / "regression-state.json").write_text("not-json", encoding="utf-8")
            completed = self.compare(root, current, "bad-3")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("STATE: reconstructed", completed.stdout)
            self.assertIn("HISTORY: FAIL", completed.stdout)
            state = json.loads((root / "regression-state.json").read_text(encoding="utf-8"))
            group = next(iter(state["groups"].values()))
            self.assertEqual(group["processed_run_ids"], ["base", "bad-1", "bad-2", "bad-3"])

    def test_stable_candidate_regression_cannot_move_its_own_baseline(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "base", 1, stage_p99=100)
            write_capture(root, "bad-1", 2, stage_p99=120)
            write_capture(root, "bad-2", 3, stage_p99=120)
            current = write_capture(root, "bad-3", 4, stage_p99=120)
            completed = self.compare(root, current, "bad-3")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("HISTORY: FAIL", completed.stdout)

    def test_performance_affecting_qemu_options_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "icount", 1)
            document = json.loads(current.read_text(encoding="utf-8"))
            document["capture"]["qemu"]["command"].extend(["-icount", "shift=3"])
            current.write_text(json.dumps(document), encoding="utf-8")
            completed = self.compare(root, current, "icount")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("frozen profile command", completed.stdout)

    def test_existing_incompatible_profile_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "old", 1, qemu_version="QEMU emulator version 9.2.0")
            current = write_capture(root, "new", 2, qemu_version="QEMU emulator version 10.0.0")
            completed = self.compare(root, current, "new")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("HISTORY: INVALID/BLOCKED", completed.stdout)
            self.assertFalse((root / "regression-state.json").exists())

    def test_incomplete_producer_and_conflicting_ids_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "producer", 1, producer_status="unexpected_exit")
            completed = self.compare(root, current, "producer")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("producer output is incomplete", completed.stdout)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "collision", 1)
            conflict = json.loads(current.read_text(encoding="utf-8"))
            conflict["capture"]["source"]["commit"] = "different"
            (root / "perf-other-name.json").write_text(json.dumps(conflict), encoding="utf-8")
            completed = self.compare(root, current, "collision")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("conflicting duplicate capture IDs", completed.stdout)

    def test_non_finite_current_value_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "non-finite", 1)
            document = json.loads(current.read_text(encoding="utf-8"))
            document["records"][1]["p99"] = float("nan")
            current.write_text(json.dumps(document), encoding="utf-8")
            completed = self.compare(root, current, "non-finite")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("non-finite JSON number", completed.stdout)

    def test_collector_retains_raw_log_when_producer_cannot_launch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            hash_input = root / "input"
            hash_input.write_bytes(b"content")
            capture_id = "launch-failure"
            command = [
                sys.executable,
                str(SCRIPTS / "bench_results.py"),
                "collect",
                "--results-dir",
                str(root),
                "--capture-id",
                capture_id,
                "--profile",
                BENCH.PROFILE,
                "--repetition",
                "1",
                "--repository",
                "example/cellos",
                "--source-ref",
                "refs/heads/main",
                "--commit",
                "deadbeef",
                "--toolchain",
                "nightly-2026-05-01",
                "--feature",
                "release-default",
                "--source-input",
                f"Cargo.lock={hash_input}",
            ]
            for name in sorted(BENCH.REQUIRED_ARTIFACTS):
                command.extend(("--artifact", f"{name}={hash_input}"))
            command.extend(
                (
                    "--",
                    str(root / "missing-qemu"),
                    "-machine",
                    "virt",
                    "-accel",
                    "tcg",
                    "-smp",
                    "2",
                    "-m",
                    "256M",
                )
            )
            completed = subprocess.run(
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stdout)
            raw = root / "raw" / f"{capture_id}.log"
            self.assertTrue(raw.is_file())
            self.assertIn("producer launch failed", raw.read_text(encoding="utf-8"))
            capture = json.loads((root / f"perf-{capture_id}.json").read_text(encoding="utf-8"))
            self.assertEqual(capture["producer"]["status"], "launch_failed")
            self.assertEqual(capture["producer"]["raw_log"]["sha256"], digest(raw.read_bytes()))

    def test_invalid_current_does_not_modify_existing_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            valid = write_capture(root, "valid", 1)
            self.assertEqual(self.compare(root, valid, "valid").returncode, 0)
            before = (root / "regression-state.json").read_bytes()
            current = root / "perf-broken.json"
            current.write_text("[]", encoding="utf-8")
            completed = self.compare(root, current, "broken")
            self.assertNotEqual(completed.returncode, 0)
            self.assertEqual((root / "regression-state.json").read_bytes(), before)

    def test_serial_parser_accepts_transport_prefix_suffix_and_non_utf8_human_log(self) -> None:
        expected = {"name": "context_switch", "n": 1000, "min": 1, "p50": 2, "p99": 3, "max": 4}
        raw_bytes = (
            b"\xffhuman log\nUSER: "
            + json.dumps(expected, separators=(",", ":")).encode("ascii")
            + b"[ WARN] multiplexed kernel text\n"
        )
        records, errors = BENCH.parse_guest_output(raw_bytes.decode("latin-1"))
        self.assertEqual(errors, [])
        self.assertEqual(records, [expected])


if __name__ == "__main__":
    unittest.main()
