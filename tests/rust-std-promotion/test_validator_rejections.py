"""Adversarial schema and global-identity regressions for the validator."""
from __future__ import annotations

import copy
import hashlib
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from rust_std_promotion import validator  # noqa: E402

FIXTURES = ROOT / "tests/rust-std-promotion/fixtures"
CLI = ROOT / "scripts/validate-rust-std-promotion.py"
SCHEMA_DIGEST = hashlib.sha256(validator.SCHEMA_PATH.read_bytes()).hexdigest()


def fixture() -> dict[str, Any]:
    return json.loads((FIXTURES / "valid-pass.json").read_text())


def result(document: Any) -> validator.Result:
    return validator.validate(document, schema_digest=SCHEMA_DIGEST)


def refresh(run: dict[str, Any]) -> None:
    valid = [rep["raw_latency_ns"] for rep in run["repetitions"]]
    run["protocol"]["independent_rep_count"] = len(valid)
    run["summary"] = {
        "valid_n": len(valid),
        "p50_ns": validator.percentile(valid, 50),
        "p95_ns": validator.percentile(valid, 95),
        "p99_ns": validator.percentile(valid, 99),
    }
    run["provenance"]["raw_digest"] = hashlib.sha256(validator.canonical_bytes(run["repetitions"])).hexdigest()

def add_second_cell(document: dict[str, Any]) -> list[dict[str, Any]]:
    extra = copy.deepcopy(document["runs"])
    for run in extra:
        run["cell_id"] = "cell-b"
        run["run_id"] += "-cell-b"
        for rep in run["repetitions"]:
            rep["rep_id"] += "-cell-b"
            rep["fresh_instance_id"] += "-cell-b"
        refresh(run)
    document["expected_cells"].append({"cell_id": "cell-b", "workload_id": "syscall-yield-v1"})
    document["runs"].extend(extra)
    return extra


class SchemaRejectionTests(unittest.TestCase):
    def assert_invalid(self, document: Any) -> None:
        actual = result(document)
        self.assertEqual(actual.exit_code, 2)
        self.assertEqual(actual.report["overall_status"], "INVALID")
        self.assertTrue(actual.report["fixture_only"])
        self.assertFalse(actual.report["promotion_eligible"])

    def test_empty_document_cells_and_runs_are_invalid(self) -> None:
        self.assert_invalid({})
        for key in ("expected_cells", "runs"):
            document = fixture()
            document[key] = []
            self.assert_invalid(document)

    def test_numeric_types_are_exact_and_nonfinite_values_are_invalid(self) -> None:
        cases: list[tuple[str, Callable[[dict[str, Any]], None]]] = [
            ("nan-warmup", lambda doc: doc["runs"][0]["protocol"].__setitem__("warmup_count", float("nan"))),
            ("inf-warmup", lambda doc: doc["runs"][0]["protocol"].__setitem__("warmup_count", float("inf"))),
            ("bool-latency", lambda doc: doc["runs"][0]["repetitions"][0].__setitem__("raw_latency_ns", True)),
            ("bool-summary", lambda doc: doc["runs"][0]["summary"].__setitem__("p99_ns", True)),
            ("numeric-clock", lambda doc: doc["runs"][0]["repetitions"][0].__setitem__("monotonic_clock", 1)),
            ("numeric-fixture", lambda doc: doc.__setitem__("fixture_id", 1)),
        ]
        for name, mutate in cases:
            with self.subTest(name=name):
                document = fixture()
                mutate(document)
                self.assert_invalid(document)

    def test_pinned_constants_enums_digests_and_datetime_are_invalid_when_wrong(self) -> None:
        cases: list[tuple[str, Callable[[dict[str, Any]], None]]] = [
            ("channel", lambda doc: doc["runs"][0]["toolchain"].__setitem__("channel", "nightly")),
            ("rustc", lambda doc: doc["runs"][0]["toolchain"].__setitem__("rustc_version", "1.98.0-nightly")),
            ("commit", lambda doc: doc["runs"][0]["toolchain"].__setitem__("commit_hash", "deadbeef")),
            ("profile", lambda doc: doc["runs"][0].__setitem__("build_profile", "debug")),
            ("timestamp", lambda doc: doc["runs"][0].__setitem__("captured_at", "2026-02-30T25:61:00Z")),
            ("digest", lambda doc: doc["runs"][0].__setitem__("binary_digest", "ABC")),
            ("architecture", lambda doc: doc["runs"][0]["environment"].__setitem__("architecture", "mips")),
        ]
        for name, mutate in cases:
            with self.subTest(name=name):
                document = fixture()
                mutate(document)
                self.assert_invalid(document)

    def test_empty_or_mismatched_threshold_profiles_are_invalid(self) -> None:
        document = fixture()
        document["runs"][0]["repetitions"][0]["interference"]["threshold_profile"] = ""
        self.assert_invalid(document)
        document = fixture()
        for rep in document["runs"][1]["repetitions"]:
            rep["interference"]["threshold_profile"] = "different-profile"
        refresh(document["runs"][1])
        self.assert_invalid(document)

    def test_run_rep_and_instance_ids_are_globally_unique(self) -> None:
        document = fixture()
        document["runs"][1]["run_id"] = document["runs"][0]["run_id"]
        self.assert_invalid(document)
        document = fixture()
        document["runs"][1]["repetitions"][0]["rep_id"] = document["runs"][0]["repetitions"][0]["rep_id"]
        refresh(document["runs"][1])
        self.assert_invalid(document)
        for identity in ("rep_id", "fresh_instance_id"):
            document = fixture()
            extra = add_second_cell(document)
            extra[0]["repetitions"][0][identity] = document["runs"][0]["repetitions"][0][identity]
            refresh(extra[0])
            self.assert_invalid(document)

    def test_rejection_observed_and_threshold_must_be_strings(self) -> None:
        for field in ("observed", "threshold"):
            document = fixture()
            run = document["runs"][0]
            rep = copy.deepcopy(run["repetitions"][0])
            rep["rep_id"] = "rejected-extra"
            rep["fresh_instance_id"] = "rejected-extra-instance"
            rep["interference"]["host_load"] = True
            run["repetitions"].append(rep)
            rejection = {"rep_id": rep["rep_id"], "reason_code": "host_load", "observed": "2.1", "threshold": "2.0"}
            rejection[field] = 2
            run["rejections"].append(rejection)
            refresh(run)
            self.assert_invalid(document)

    def test_nonfinite_json_constants_and_empty_json_exit_two(self) -> None:
        inputs = [("empty", "{}")]
        for constant, value in (("NaN", float("nan")), ("Infinity", float("inf")), ("-Infinity", float("-inf"))):
            document = fixture()
            document["runs"][0]["protocol"]["warmup_count"] = value
            payload = json.dumps(document, allow_nan=True, separators=(",", ":"))
            self.assertIn(f'"warmup_count":{constant}', payload)
            inputs.append((constant, payload))
        for name, payload in inputs:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "fixture.json"
                path.write_text(payload)
                completed = subprocess.run([sys.executable, str(CLI), str(path)], check=False, capture_output=True)
                report = json.loads(completed.stdout)
                self.assertEqual((completed.returncode, report["overall_status"]), (2, "INVALID"))
                self.assertTrue(report["fixture_only"])
                self.assertFalse(report["promotion_eligible"])

    def test_approval_manifest_closes_inputs_without_self_reference(self) -> None:
        package = ROOT / ".agents/260821-1800-tier1-rust-std-pal-feasibility"
        manifest_path = package / "artifacts/approval-input-manifest.json"
        manifest = json.loads(manifest_path.read_text())
        listed = {entry["path"] for entry in manifest["inputs"]}
        self.assertTrue(set(manifest["excluded_self_referential_records"]).isdisjoint(listed))
        support_map = json.loads((package / "artifacts/pal-hook-support-map.json").read_text())
        required_kernel_paths = set(support_map["kernel_security_backing_inventory"]["required_paths"])
        approval_kernel_paths = {entry["path"] for entry in manifest["inputs"] if entry["role"] == "kernel-security-backing-source"}
        self.assertEqual(approval_kernel_paths, required_kernel_paths)
        for entry in manifest["inputs"]:
            path = Path(manifest["source_roots"]["rust-src"]) / entry["path"][11:] if entry["path"].startswith("rust-src://") else ROOT / entry["path"]
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), entry["sha256"])
        digest = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
        for record in manifest["excluded_self_referential_records"][1:]:
            self.assertIn(digest, (ROOT / record).read_text())
        approval_records = [record for record in manifest["excluded_self_referential_records"] if "/approvals/" in record]
        self.assertEqual(len(approval_records), 4)
        for record in approval_records:
            bound_digests = set(re.findall(r"\b[0-9a-f]{64}\b", (ROOT / record).read_text()))
            self.assertEqual(bound_digests, {digest})

    def test_cli_rejects_removed_output_option(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "report.json"
            completed = subprocess.run([sys.executable, str(CLI), str(FIXTURES / "valid-pass.json"), "--output", str(output)], check=False, capture_output=True)
            self.assertEqual(completed.returncode, 2)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
