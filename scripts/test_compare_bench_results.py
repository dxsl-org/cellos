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


RV64 = BENCH.RV64_PROFILE
X86 = BENCH.X86_PROFILE
X86_CLOCK_MARKER = "[INFO] x86_64 timers initialized from ACPI (HPET + LAPIC)"


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


def counters_record(spec: Any, mode: str) -> dict[str, Any]:
    record: dict[str, Any] = {"name": f"{spec.name}.counters", "tier": spec.tier, "path": spec.path}
    for field in BENCH.COUNTER_FIELDS:
        if mode == "unknown":
            record[field] = "unknown"
        elif field == "domain_syscall_cr3":
            record[field] = 2200 if spec.tier == "domain" else 0
        else:
            record[field] = 1000
    return record


def suite_records(profile: str, stage_p99: int = 100, counters: str | None = None) -> list[dict[str, Any]]:
    body: list[dict[str, Any]] = []
    for spec in BENCH.PROFILES[profile].metrics:
        if counters is not None:
            body.append(counters_record(spec, counters))
        body.append(result_record(spec, stage_p99 if spec.name == "stage_encode_request_x1000" else 100))
    return [
        {"bench_event": "start", "profile": profile},
        *body,
        {"bench_event": "complete", "profile": profile, "invalid": 0},
    ]


def serial_transcript(profile: str, records: list[dict[str, Any]]) -> bytes:
    lines = [json.dumps(record, separators=(",", ":")) for record in records]
    if profile == X86:
        # x86 cells print through the kernel console with the `USER: ` prefix.
        lines = [X86_CLOCK_MARKER, "USER: Cellos > "] + [f"USER: {line}" for line in lines]
    return "\n".join(lines).encode() + b"\n"


def qemu_command(profile: str, accelerator: str) -> list[str]:
    if profile == RV64:
        return [
            "qemu-system-riscv64", "-machine", "virt", "-accel", "tcg", "-smp", "2", "-nographic",
            "-bios", "default", "-kernel", "kernel", "-m", "256M",
            "-drive", "file=disk,format=raw,id=hd0,if=none", "-device", "virtio-blk-device,drive=hd0",
        ]
    return [
        "qemu-system-x86_64", "-machine", "q35", "-accel", accelerator,
        "-cpu", BENCH.X86_CPU_MODELS[accelerator], "-smp", "2", "-m", "256M",
        "-nographic", "-no-reboot", "-boot", "d", "-cdrom", "vicell.iso",
    ]


def write_capture(
    root: Path,
    capture_id: str,
    order: int,
    *,
    profile: str = RV64,
    accelerator: str = "tcg",
    cpu: str = "Test CPU A",
    kernel_release: str = "6.1.0-test",
    governor: str = "performance",
    stage_p99: int = 100,
    qemu_version: str = "QEMU emulator version 10.0.0",
    mutate_records: Any = None,
    producer_status: str = "completed",
    counters: str | None = None,
    features: tuple[str, ...] = ("release-default",),
) -> Path:
    spec = BENCH.PROFILES[profile]
    records = suite_records(profile, stage_p99, counters)
    if mutate_records is not None:
        mutate_records(records)
    raw = serial_transcript(profile, records)
    raw_relative = Path("raw") / f"{capture_id}.log"
    raw_path = root / raw_relative
    raw_path.parent.mkdir(parents=True, exist_ok=True)
    raw_path.write_bytes(raw)
    events, results, counter_records = BENCH._partition_records(records)
    document = {
        "schema": BENCH.SCHEMA,
        "capture": {
            "id": capture_id,
            "captured_at": f"2026-09-{order:02d}T12:00:00Z",
            "profile": profile,
            "repetition": 1,
            "profile_contract": spec.contract,
            "source": {
                "repository": "example/cellos",
                "ref": "refs/heads/main",
                "commit": f"commit-{capture_id}",
                "inputs": {"Cargo.lock": "1" * 64},
            },
            "build": {"toolchain": "nightly-2026-05-01", "features": sorted(features)},
            "qemu": {
                "version": qemu_version,
                "command": qemu_command(profile, accelerator),
                "machine": spec.machine,
                "accelerator": accelerator,
                "accelerator_observed": accelerator,
                "harts": 2,
                "ram_mib": 256,
            },
            "fingerprint": {
                "host_cpu_model": cpu,
                "host_kernel_release": kernel_release,
                "cpu_governor": governor,
                "qemu_version": qemu_version,
                "accelerator": accelerator,
                "vcpus": 2,
                "ram_mib": 256,
                "guest_clocksource": BENCH._guest_clocksource(spec, raw.decode("latin-1")),
            },
            "artifacts": {
                name: {"path": name, "sha256": str(index) * 64}
                for index, name in enumerate(sorted(spec.required_artifacts), 1)
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
        "counters": counter_records,
        "validity": {"verdict": "VALID", "errors": []},
        "target": BENCH.target_verdict(profile, records) if set(spec.specs) == {record.get("name") for record in results} else {"verdict": "NOT_EVALUATED", "rows": []},
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
                RV64,
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
            for name in sorted(BENCH.PROFILES[RV64].required_artifacts):
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

    def test_serial_parser_rejoins_a_record_a_kernel_line_interrupted(self) -> None:
        expected = {"name": "context_switch", "n": 1000, "min": 1, "p50": 2, "p99": 3, "max": 4}
        text = json.dumps(expected, separators=(",", ":"))
        raw = f"USER: {text[:20]}\n[ INFO] kernel line\n[ WARN] another\nUSER+ {text[20:]}\n"
        records, errors = BENCH.parse_guest_output(raw)
        self.assertEqual(errors, [])
        self.assertEqual(records, [expected])


FAKE_QEMU = r'''
import sys
import time
from pathlib import Path

here = Path(__file__).resolve().parent
if sys.argv[1:] == ["--version"]:
    print("QEMU emulator version 8.2.2 (fake)")
    raise SystemExit(0)
with (here / "invocations.log").open("a") as log:
    log.write(" ".join(sys.argv[1:]) + "\n")
if (here / "mode").read_text().strip() == "kvm-fail":
    sys.stderr.write("Could not access KVM kernel module: No such file or directory\n")
    sys.stderr.write("qemu-system-x86_64: failed to initialize kvm: No such file or directory\n")
    raise SystemExit(1)
head, prompt, body = (here / "transcript.txt").read_bytes().partition(b"USER: Cellos > \n")
sys.stdout.buffer.write(head + prompt)
sys.stdout.flush()
sys.stdin.readline()
sys.stdout.buffer.write(body)
sys.stdout.flush()
time.sleep(60)
'''


class X86ProfileTests(unittest.TestCase):
    compare = ComparatorContractTests.compare

    def fake_qemu(self, root: Path, mode: str) -> Path:
        directory = root / "fake"
        directory.mkdir()
        executable = directory / "qemu-system-x86_64"
        executable.write_text(f"#!{sys.executable}\n{FAKE_QEMU}", encoding="utf-8")
        executable.chmod(0o755)
        (directory / "mode").write_text(mode, encoding="utf-8")
        (directory / "transcript.txt").write_bytes(serial_transcript(X86, suite_records(X86)))
        return executable

    def collect_x86(self, root: Path, qemu: Path, capture_id: str, accelerator: str) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
        hash_input = root / "input"
        hash_input.write_bytes(b"content")
        results = root / "results"
        command = [
            sys.executable, str(SCRIPTS / "bench_results.py"), "collect",
            "--results-dir", str(results), "--capture-id", capture_id, "--profile", X86,
            "--repetition", "1", "--repository", "example/cellos", "--source-ref", "refs/heads/main",
            "--commit", "deadbeef", "--toolchain", "nightly-2026-05-01", "--feature", "release-default",
            "--source-input", f"Cargo.lock={hash_input}", "--stdin-line", "bench", "--stdin-after", "Cellos >",
            "--timeout-seconds", "30",
        ]
        for name in sorted(BENCH.PROFILES[X86].required_artifacts):
            command.extend(("--artifact", f"{name}={hash_input}"))
        command.extend(("--", str(qemu), *qemu_command(X86, accelerator)[1:]))
        completed = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, check=False)
        self.assertEqual(completed.returncode, 0, completed.stdout)
        return completed, json.loads((results / f"perf-{capture_id}.json").read_text(encoding="utf-8"))

    def test_profiles_label_tier_path_and_accept_ipc_fastpath(self) -> None:
        for profile in (RV64, X86):
            contract = {entry["name"]: entry for entry in BENCH.PROFILES[profile].contract}
            self.assertEqual((contract["ipc_fastpath"]["tier"], contract["ipc_fastpath"]["path"]), ("sas", "ring"))
            self.assertEqual((contract["ipc_send_recv"]["tier"], contract["ipc_send_recv"]["path"]), ("sas", "copy"))
            self.assertTrue(all(entry["tier"] in {"sas", "domain"} and entry["path"] for entry in contract.values()))
        records, errors = BENCH.parse_guest_output(serial_transcript(X86, suite_records(X86)).decode("latin-1"))
        self.assertEqual(errors, [])
        self.assertIn({"name": "ipc_fastpath", "n": 1000, "min": 10, "p50": 50, "p99": 100, "max": 200}, records)
        self.assertEqual(BENCH.validate_records(records, X86), [])
        self.assertTrue(any("profile does not match" in error for error in BENCH.validate_records(records, RV64)))
        without_ring = [record for record in records if record.get("name") != "ipc_fastpath"]
        self.assertIn("missing required metric records: ipc_fastpath", BENCH.validate_records(without_ring, X86))

    def test_x86_tcg_capture_is_valid_but_report_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "tcg", 1, profile=X86, accelerator="tcg")
            completed = self.compare(root, current, "tcg")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("VALIDITY: VALID", completed.stdout)
            self.assertIn("TARGET: NOT_CONFIGURED", completed.stdout)
            self.assertIn("BUDGET: REPORT_ONLY", completed.stdout)

    def test_tcg_regression_is_never_budget_pass_or_fail(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "base", 1, profile=X86, accelerator="tcg", stage_p99=100)
            write_capture(root, "bad-1", 2, profile=X86, accelerator="tcg", stage_p99=150)
            write_capture(root, "bad-2", 3, profile=X86, accelerator="tcg", stage_p99=150)
            current = write_capture(root, "bad-3", 4, profile=X86, accelerator="tcg", stage_p99=150)
            completed = self.compare(root, current, "bad-3")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("HISTORY (informational): FAIL", completed.stdout)
            self.assertIn("BUDGET: REPORT_ONLY", completed.stdout)
            self.assertNotIn("BUDGET: PASS", completed.stdout)
            self.assertNotIn("BUDGET: FAIL", completed.stdout)

    def test_unmatched_kvm_capture_is_no_baseline_not_pass(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "kvm", 1, profile=X86, accelerator="kvm")
            completed = self.compare(root, current, "kvm")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("BUDGET: NO_BASELINE", completed.stdout)
            self.assertNotIn("BUDGET: PASS", completed.stdout)

    def test_mixed_profile_accelerator_and_fingerprint_are_never_merged(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            others = [
                write_capture(root, "x86-tcg-1", 1, profile=X86, accelerator="tcg"),
                write_capture(root, "x86-tcg-2", 2, profile=X86, accelerator="tcg"),
                write_capture(root, "rv64-tcg", 3, profile=RV64, accelerator="tcg"),
                write_capture(root, "kvm-cpu-b-1", 4, profile=X86, accelerator="kvm", cpu="Test CPU B"),
                write_capture(root, "kvm-cpu-b-2", 5, profile=X86, accelerator="kvm", cpu="Test CPU B"),
            ]
            # Ten times slower than every other capture: any merge would regress it.
            current = write_capture(root, "kvm-cpu-a", 6, profile=X86, accelerator="kvm", stage_p99=1000)
            keys = {BENCH._compatibility_key(json.loads(path.read_text(encoding="utf-8"))) for path in (others[0], others[2], others[3], current)}
            self.assertEqual(len(keys), 4)
            completed = self.compare(root, current, "kvm-cpu-a")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("EXCLUDED: 5 valid capture(s)", completed.stdout)
            self.assertIn("HISTORY: BASELINE_ONLY", completed.stdout)
            self.assertIn("BUDGET: NO_BASELINE", completed.stdout)
            state = json.loads((root / "regression-state.json").read_text(encoding="utf-8"))
            groups = sorted(group["processed_run_ids"] for group in state["groups"].values())
            self.assertEqual(groups, [["kvm-cpu-a"], ["kvm-cpu-b-1", "kvm-cpu-b-2"], ["rv64-tcg"], ["x86-tcg-1", "x86-tcg-2"]])

    def test_first_capture_compared_after_later_matched_runs_is_not_drift(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = write_capture(root, "first", 1, profile=X86, accelerator="kvm")
            write_capture(root, "second", 2, profile=X86, accelerator="kvm")
            completed = self.compare(root, first, "first")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("HISTORY: BASELINE_ONLY", completed.stdout)
            self.assertIn("BUDGET: NO_BASELINE", completed.stdout)

    def test_fingerprint_drift_on_one_machine_class_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "old-kernel", 1, profile=X86, accelerator="kvm", kernel_release="6.1.0-test")
            current = write_capture(root, "new-kernel", 2, profile=X86, accelerator="kvm", kernel_release="6.2.0-test")
            completed = self.compare(root, current, "new-kernel")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("HISTORY: INVALID/BLOCKED", completed.stdout)
            self.assertIn("BUDGET: BLOCKED", completed.stdout)

    def test_matched_kvm_deliberate_regression_fails_budget(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "base", 1, profile=X86, accelerator="kvm", stage_p99=100)
            stable = write_capture(root, "stable", 2, profile=X86, accelerator="kvm", stage_p99=104)
            completed = self.compare(root, stable, "stable")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("BUDGET: PASS", completed.stdout)
            write_capture(root, "slow-1", 3, profile=X86, accelerator="kvm", stage_p99=115)
            slow_2 = write_capture(root, "slow-2", 4, profile=X86, accelerator="kvm", stage_p99=115)
            completed = self.compare(root, slow_2, "slow-2")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("sas/codec/stage_encode_request_x1000/p99: REGRESSION", completed.stdout)
            slow_3 = write_capture(root, "slow-3", 5, profile=X86, accelerator="kvm", stage_p99=115)
            completed = self.compare(root, slow_3, "slow-3")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("HISTORY: FAIL", completed.stdout)
            self.assertIn("sas/codec/stage_encode_request_x1000/p99: FAIL", completed.stdout)
            self.assertIn("BUDGET: FAIL", completed.stdout)

    def test_unknown_machine_metadata_disables_budget_qualification(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "base", 1, profile=X86, accelerator="kvm", governor="unknown", stage_p99=100)
            write_capture(root, "slow-1", 2, profile=X86, accelerator="kvm", governor="unknown", stage_p99=150)
            write_capture(root, "slow-2", 3, profile=X86, accelerator="kvm", governor="unknown", stage_p99=150)
            current = write_capture(root, "slow-3", 4, profile=X86, accelerator="kvm", governor="unknown", stage_p99=150)
            completed = self.compare(root, current, "slow-3")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("HISTORY: FAIL", completed.stdout)
            self.assertIn("BUDGET: UNQUALIFIED (unknown: cpu_governor)", completed.stdout)
            self.assertNotIn("BUDGET: PASS", completed.stdout)

    def test_requested_kvm_launch_failure_is_invalid_without_tcg_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            qemu = self.fake_qemu(root, "kvm-fail")
            completed, capture = self.collect_x86(root, qemu, "kvm-fail", "kvm")
            self.assertIn("producer: kvm_unavailable", completed.stdout)
            self.assertEqual(capture["producer"]["status"], "kvm_unavailable")
            self.assertIn("no fallback attempted", capture["producer"]["launch_error"])
            self.assertEqual(capture["capture"]["qemu"]["accelerator"], "kvm")
            self.assertEqual(capture["capture"]["fingerprint"]["accelerator"], "kvm")
            self.assertEqual(capture["capture"]["qemu"]["accelerator_observed"], "unknown")
            self.assertEqual(capture["validity"]["verdict"], "INVALID")
            raw = (root / "results" / "raw" / "kvm-fail.log").read_text(encoding="utf-8")
            self.assertIn("failed to initialize kvm", raw)
            invocations = (qemu.parent / "invocations.log").read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(invocations), 1)
            self.assertIn("-accel kvm", invocations[0])
            compared = self.compare(root / "results", root / "results" / "perf-kvm-fail.json", "kvm-fail")
            self.assertNotEqual(compared.returncode, 0)
            self.assertIn("VALIDITY: INVALID", compared.stdout)
            self.assertIn("producer output is incomplete: kvm_unavailable", compared.stdout)

    def test_requested_kvm_not_observed_is_invalid_even_with_complete_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            qemu = self.fake_qemu(root, "run")
            _, capture = self.collect_x86(root, qemu, "no-kvm-fd", "kvm")
            self.assertEqual(capture["producer"]["status"], "kvm_unavailable")
            self.assertEqual(capture["capture"]["qemu"]["accelerator"], "kvm")
            self.assertEqual(capture["capture"]["qemu"]["accelerator_observed"], "tcg")
            self.assertEqual(len(capture["results"]), len(BENCH.PROFILES[X86].metrics))
            self.assertEqual(capture["validity"]["verdict"], "INVALID")
            self.assertEqual(len((qemu.parent / "invocations.log").read_text(encoding="utf-8").splitlines()), 1)

    def test_collector_records_x86_tcg_capture_with_fingerprint(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            qemu = self.fake_qemu(root, "run")
            completed, capture = self.collect_x86(root, qemu, "tcg", "tcg")
            self.assertEqual(capture["validity"], {"verdict": "VALID", "errors": []}, completed.stdout)
            self.assertEqual(capture["producer"]["status"], "intentional_termination_after_complete")
            fingerprint = capture["capture"]["fingerprint"]
            self.assertEqual(fingerprint["accelerator"], "tcg")
            self.assertEqual(fingerprint["guest_clocksource"], "acpi-hpet+lapic")
            self.assertEqual(fingerprint["host_cpu_model"], BENCH._host_cpu_model())
            self.assertEqual(fingerprint["qemu_version"], "QEMU emulator version 8.2.2 (fake)")
            self.assertEqual((fingerprint["vcpus"], fingerprint["ram_mib"]), (2, 256))
            self.assertEqual(capture["target"], {"verdict": "NOT_CONFIGURED", "rows": []})


class CountersAndDomainTests(unittest.TestCase):
    compare = ComparatorContractTests.compare
    COUNTERS = ("bench-counters", "release-default")

    def test_counter_records_parse_and_are_labelled(self) -> None:
        records, errors = BENCH.parse_guest_output(serial_transcript(X86, suite_records(X86, counters="known")).decode("latin-1"))
        self.assertEqual(errors, [])
        self.assertEqual(BENCH.validate_records(records, X86), [])
        self.assertEqual(BENCH.counter_feature_errors(records, list(self.COUNTERS)), [])
        domain = next(record for record in records if record.get("name") == "ipc_send_recv_domain.counters")
        self.assertEqual((domain["tier"], domain["path"], domain["domain_syscall_cr3"]), ("domain", "copy", 2200))
        _, results, counters = BENCH._partition_records(records)
        self.assertEqual(len(counters), len(BENCH.PROFILES[X86].metrics))
        self.assertFalse(any(record["name"].endswith(".counters") for record in results))

        def mislabel(records: list[dict[str, Any]]) -> None:
            next(record for record in records if record.get("name") == "ipc_send_recv_domain.counters")["tier"] = "sas"

        records = suite_records(X86, counters="known")
        mislabel(records)
        self.assertIn("counters ipc_send_recv_domain.counters must be labelled tier=domain path=copy", BENCH.validate_records(records, X86))
        records = suite_records(X86, counters="known")
        records.insert(1, {**records[1], "syscalls": -1})
        errors = BENCH.validate_records(records, X86)
        self.assertTrue(any("duplicate counters record" in error for error in errors))
        self.assertTrue(any("syscalls must be a non-negative integer or 'unknown'" in error for error in errors))

    def test_unknown_counters_are_preserved_never_zero(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = write_capture(root, "plain", 1, profile=X86, accelerator="kvm", counters="unknown")
            completed = self.compare(root, current, "plain")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("COUNTERS (informational) domain/copy/ipc_send_recv_domain.counters: syscalls=unknown domain_syscall_cr3=unknown", completed.stdout)
            stored = json.loads(current.read_text(encoding="utf-8"))
            self.assertTrue(all(record[field] == "unknown" for record in stored["counters"] for field in BENCH.COUNTER_FIELDS))

    def test_counter_values_must_agree_with_kernel_features(self) -> None:
        cases = {
            "values-without-feature": ("known", ("release-default",), "report values without the bench-counters kernel feature"),
            "unknown-with-feature": ("unknown", self.COUNTERS, "bench-counters capture has unknown counters"),
            "missing-with-feature": (None, self.COUNTERS, "bench-counters capture lacks counters records"),
        }
        for name, (counters, features, message) in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                current = write_capture(root, name, 1, profile=X86, accelerator="kvm", counters=counters, features=features)
                completed = self.compare(root, current, name)
                self.assertNotEqual(completed.returncode, 0)
                self.assertIn("VALIDITY: INVALID", completed.stdout)
                self.assertIn(message, completed.stdout)

    def test_kernel_feature_mismatch_is_a_separate_series(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            instrumented = [
                write_capture(root, f"counters-{index}", index, profile=X86, accelerator="kvm", counters="known", features=self.COUNTERS)
                for index in (1, 2, 3)
            ]
            # Ten times slower: merging with the instrumented series would regress it.
            current = write_capture(root, "plain", 4, profile=X86, accelerator="kvm", counters="unknown", stage_p99=1000)
            documents = [json.loads(path.read_text(encoding="utf-8")) for path in (instrumented[0], current)]
            self.assertNotEqual(BENCH._compatibility_key(documents[0]), BENCH._compatibility_key(documents[1]))
            completed = self.compare(root, current, "plain")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("EXCLUDED: 3 valid capture(s)", completed.stdout)
            self.assertIn("HISTORY: BASELINE_ONLY", completed.stdout)
            self.assertIn("BUDGET: NO_BASELINE", completed.stdout)
            completed = self.compare(root, instrumented[2], "counters-3")
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("BUDGET: PASS", completed.stdout)

    def test_domain_metric_is_labelled_and_compared_separately(self) -> None:
        x86 = {entry["name"]: entry for entry in BENCH.PROFILES[X86].contract}
        self.assertEqual((x86["ipc_send_recv_domain"]["tier"], x86["ipc_send_recv_domain"]["path"]), ("domain", "copy"))
        self.assertNotIn("ipc_send_recv_domain", {entry["name"] for entry in BENCH.PROFILES[RV64].contract})

        def slow_domain(records: list[dict[str, Any]]) -> None:
            record = next(record for record in records if record.get("name") == "ipc_send_recv_domain")
            record["p99"] = 150

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_capture(root, "base", 1, profile=X86, accelerator="kvm")
            write_capture(root, "slow-1", 2, profile=X86, accelerator="kvm", mutate_records=slow_domain)
            write_capture(root, "slow-2", 3, profile=X86, accelerator="kvm", mutate_records=slow_domain)
            current = write_capture(root, "slow-3", 4, profile=X86, accelerator="kvm", mutate_records=slow_domain)
            completed = self.compare(root, current, "slow-3")
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("domain/copy/ipc_send_recv_domain/p99: FAIL", completed.stdout)
            self.assertNotIn("sas/copy/ipc_send_recv/p99", completed.stdout)
            self.assertIn("BUDGET: FAIL", completed.stdout)


if __name__ == "__main__":
    unittest.main()
