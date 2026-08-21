"""Behavioral contract tests for the fixture-only rust-std validator."""
from __future__ import annotations

import copy
import hashlib
import json
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from rust_std_promotion import validator  # noqa: E402

FIXTURES = ROOT / "tests/rust-std-promotion/fixtures"
CLI = ROOT / "scripts/validate-rust-std-promotion.py"
SCHEMA_DIGEST = hashlib.sha256((ROOT / "scripts/rust_std_promotion/benchmark-run.schema.json").read_bytes()).hexdigest()
REQUIRED_KERNEL_SECURITY_PATHS = {
    "kernel/Cargo.toml",
    "kernel/src/task/drivers.rs",
    "kernel/src/task/drivers/virtio_rng.rs",
    "kernel/src/task/syscall.rs",
    "libs/api/src/abi/syscall.rs",
    "libs/ostd/src/syscall.rs",
}


def fixture(name: str = "valid-pass") -> dict:
    return json.loads((FIXTURES / f"{name}.json").read_text())


def result(document: dict) -> validator.Result:
    return validator.validate(document, schema_digest=SCHEMA_DIGEST)


def refresh(run: dict) -> None:
    valid = [rep["raw_latency_ns"] for rep in run["repetitions"]]
    run["protocol"]["independent_rep_count"] = len(valid)
    run["summary"] = {"valid_n": len(valid), "p50_ns": validator.percentile(valid, 50), "p95_ns": validator.percentile(valid, 95), "p99_ns": validator.percentile(valid, 99)}
    run["provenance"]["raw_digest"] = hashlib.sha256(validator.canonical_bytes(run["repetitions"])).hexdigest()


def constant_arm(run: dict, latency: int) -> None:
    for rep in run["repetitions"]:
        rep["raw_latency_ns"] = latency
    refresh(run)


class ValidatorTests(unittest.TestCase):
    def test_valid_pass_matches_pinned_report_bytes(self) -> None:
        actual = validator.canonical_bytes(result(fixture()).report)
        self.assertEqual(actual, (FIXTURES / "expected-valid-pass.report.json").read_bytes())

    def test_valid_fail_regression_matches_report_and_exit_one(self) -> None:
        actual = result(fixture("valid-fail-regression"))
        self.assertEqual(actual.exit_code, 1)
        self.assertEqual(validator.canonical_bytes(actual.report), (FIXTURES / "expected-valid-fail-regression.report.json").read_bytes())

    def test_named_invalid_fixtures_exit_two(self) -> None:
        names = ("invalid-warmups", "invalid-repetitions", "invalid-parity", "invalid-drift", "invalid-interference", "invalid-raw-samples")
        self.assertEqual({name: result(fixture(name)).exit_code for name in names}, {name: 2 for name in names})

    def test_nearest_rank_percentile_boundaries_are_integer_exact(self) -> None:
        values = list(range(1, 101))
        self.assertEqual((validator.percentile(values, 50), validator.percentile(values, 95), validator.percentile(values, 99)), (50, 95, 99))
        self.assertEqual(validator.percentile([9] * 29 + [10], 99), 10)

    def test_inclusive_two_percent_drift_passes(self) -> None:
        document = fixture()
        constant_arm(document["runs"][0], 100); constant_arm(document["runs"][1], 105); constant_arm(document["runs"][2], 102)
        self.assertEqual(result(document).exit_code, 0)

    def test_drift_above_two_percent_is_invalid(self) -> None:
        document = fixture()
        constant_arm(document["runs"][0], 100); constant_arm(document["runs"][2], 103)
        self.assertEqual(result(document).exit_code, 2)

    def test_inclusive_five_percent_regression_passes(self) -> None:
        document = fixture()
        for run, latency in zip(document["runs"], (100, 105, 100)): constant_arm(run, latency)
        self.assertEqual(result(document).exit_code, 0)

    def test_regression_above_five_percent_is_valid_fail(self) -> None:
        document = fixture()
        for run, latency in zip(document["runs"], (100, 106, 100)): constant_arm(run, latency)
        self.assertEqual(result(document).exit_code, 1)

    def test_any_interference_flag_or_rejection_invalidates_document(self) -> None:
        flagged = fixture()
        flagged["runs"][0]["repetitions"][0]["interference"]["host_load"] = True
        refresh(flagged["runs"][0])
        rejected = fixture()
        rejected["runs"][1]["rejections"].append({"rep_id": rejected["runs"][1]["repetitions"][0]["rep_id"], "reason_code": "host_load", "observed": "2.1", "threshold": "2.0"})
        self.assertEqual((result(flagged).exit_code, result(rejected).exit_code), (2, 2))

    def test_physical_arm_order_is_not_repaired(self) -> None:
        document = fixture()
        document["runs"][0], document["runs"][1] = document["runs"][1], document["runs"][0]
        self.assertEqual(result(document).exit_code, 2)

    def test_captured_at_must_be_strictly_increasing_utc(self) -> None:
        equal = fixture()
        equal["runs"][1]["captured_at"] = equal["runs"][0]["captured_at"]
        reversed_time = fixture()
        reversed_time["runs"][1]["captured_at"] = "2026-08-20T23:59:59Z"
        offset = fixture()
        offset["runs"][1]["captured_at"] = "2026-08-21T01:00:00+01:00"
        self.assertEqual(tuple(result(doc).exit_code for doc in (equal, reversed_time, offset)), (2, 2, 2))

    def test_linker_input_manifests_are_closed_and_pinned(self) -> None:
        cases = []
        addition = fixture(); addition["runs"][0]["common_linker_inputs"].append(copy.deepcopy(addition["runs"][0]["common_linker_inputs"][0])); cases.append(addition)
        omission = fixture(); omission["runs"][0]["common_linker_inputs"].pop(); cases.append(omission)
        role_swap = fixture(); role_swap["runs"][0]["common_linker_inputs"][0]["role"], role_swap["runs"][0]["common_linker_inputs"][1]["role"] = role_swap["runs"][0]["common_linker_inputs"][1]["role"], role_swap["runs"][0]["common_linker_inputs"][0]["role"]; cases.append(role_swap)
        digest_swap = fixture(); digest_swap["runs"][0]["runtime_linker_inputs"][0]["digest"], digest_swap["runs"][0]["runtime_linker_inputs"][1]["digest"] = digest_swap["runs"][0]["runtime_linker_inputs"][1]["digest"], digest_swap["runs"][0]["runtime_linker_inputs"][0]["digest"]; cases.append(digest_swap)
        for identity in ("mlibc/v1", "posix-libc/v1", "benchmark-instrumentation/v1"):
            forbidden = fixture(); forbidden["runs"][1]["runtime_linker_inputs"][1]["identity"] = identity; cases.append(forbidden)
        for document in cases:
            run = next(run for run in document["runs"] if run["common_linker_inputs"] != validator.FIXTURE_COMMON_LINKER_INPUTS or run["runtime_linker_inputs"] != validator.FIXTURE_RUNTIME_LINKER_INPUTS[run["runtime_kind"]])
            run["common_linker_inputs_digest"] = hashlib.sha256(validator.canonical_bytes(run["common_linker_inputs"])).hexdigest()
            run["runtime_linker_inputs_digest"] = hashlib.sha256(validator.canonical_bytes(run["runtime_linker_inputs"])).hexdigest()
            self.assertEqual(result(document).exit_code, 2)

    def test_missing_expected_cell_is_invalid(self) -> None:
        document = fixture(); document["expected_cells"].append({"cell_id": "cell-b", "workload_id": "syscall-yield-v1"})
        self.assertEqual(result(document).exit_code, 2)

    def test_one_failing_cell_cannot_be_masked_by_a_passing_cell(self) -> None:
        document = fixture(); extra = copy.deepcopy(document["runs"])
        for run in document["runs"]: constant_arm(run, 100 if run["arm"] != "std" else 106)
        for run in extra:
            run["cell_id"] = "cell-b"; run["run_id"] += "-b"
            for rep in run["repetitions"]:
                rep["rep_id"] += "-b"; rep["fresh_instance_id"] += "-b"
            refresh(run)
        document["runs"].extend(extra); document["expected_cells"].append({"cell_id": "cell-b", "workload_id": "syscall-yield-v1"})
        self.assertEqual(result(document).report["overall_status"], "VALID_FAIL")

    def test_raw_samples_are_retained_in_report(self) -> None:
        arms = result(fixture()).report["cells"][0]["arms"]
        self.assertEqual([len(arm["raw_samples_ns"]) for arm in arms], [30, 30, 30])

    def test_schema_version_mismatch_is_invalid(self) -> None:
        document = fixture(); document["schema_version"] = "rust-std-benchmark-run/v2"
        self.assertEqual(result(document).exit_code, 2)

    def test_schema_digest_substitution_is_invalid(self) -> None:
        document = fixture(); document["runs"][0]["provenance"]["schema_digest"] = "0" * 64
        self.assertEqual(result(document).exit_code, 2)

    def test_unknown_field_is_invalid(self) -> None:
        document = fixture(); document["promotion_eligible"] = True
        self.assertEqual(result(document).exit_code, 2)

    def test_live_source_and_false_promotion_designation_are_invalid(self) -> None:
        live = fixture(); live["source_kind"] = "live_capture"
        promotion = fixture(); promotion["requested_designation"] = "promotion_evidence"
        self.assertEqual((result(live).exit_code, result(promotion).exit_code), (2, 2))

    def test_runtime_arm_mismatch_is_invalid(self) -> None:
        document = fixture(); document["runs"][1]["runtime_kind"] = "no_std"
        self.assertEqual(result(document).exit_code, 2)

    def test_operation_trace_substitution_is_invalid(self) -> None:
        document = fixture(); document["runs"][0]["operation_trace_digest"] = "0" * 64
        self.assertEqual(result(document).exit_code, 2)

    def test_hook_map_covers_every_pinned_sys_module(self) -> None:
        path = ROOT / ".agents/260821-1800-tier1-rust-std-pal-feasibility/artifacts/pal-hook-support-map.json"
        support_map = json.loads(path.read_text())
        source_root = Path(support_map["toolchain"]["rust_src_root"])
        lines = (source_root / "library/std/src/sys/mod.rs").read_text().splitlines()[2:30]
        declared = [line.strip().removeprefix("pub ").removeprefix("mod ").removesuffix(";") for line in lines if line.strip()]
        scoped = support_map["scope"]["sys_module_manifest"]
        self.assertEqual([row["module"] for row in scoped], declared)
        self.assertEqual((len(scoped), support_map["scope"]["mapped_sys_modules"], support_map["scope"]["omitted_sys_modules"]), (27, 27, []))
        self.assertTrue(all(row["hook_ids"] and row["source_sha256"] for row in scoped))
        self.assertEqual(support_map["summary"], {**support_map["summary"], "total": 36, "supported": 8, "unsupported": 10, "deferred": 18})
        hooks = {hook["hook_id"]: hook for hook in support_map["hooks"]}
        self.assertEqual(hooks["PAL-025"]["classification"], "Deferred")
        self.assertEqual((hooks["PAL-019"]["classification"], hooks["PAL-031"]["classification"]), ("Deferred", "Deferred"))
        self.assertTrue({"PAL-019", "PAL-031"} <= set(support_map["summary"]["blocking_deferred_hook_ids"]))

    def test_entropy_pointer_contract_and_kernel_inventory_cannot_drift(self) -> None:
        path = ROOT / ".agents/260821-1800-tier1-rust-std-pal-feasibility/artifacts/pal-hook-support-map.json"
        support_map = json.loads(path.read_text())
        hooks = {hook["hook_id"]: hook for hook in support_map["hooks"]}
        entropy = json.dumps(hooks["PAL-019"], sort_keys=True)
        pointer = json.dumps(hooks["PAL-031"], sort_keys=True)
        for required in ("current", "non-qualifying", "default", "dev-weak-rng", "real entropy", "zero/error", "hostile direct-syscall evidence"):
            self.assertIn(required, entropy)
        for required in ("validate_user_buf", "bounded caller-owned writable validation", "null", "overflow", "unmapped", "kernel", "peer", "hostile direct-syscall evidence"):
            self.assertIn(required, pointer)

        inventory = support_map["kernel_security_backing_inventory"]
        entries = {entry["path"]: entry for entry in inventory["entries"]}
        self.assertEqual(set(inventory["required_paths"]), REQUIRED_KERNEL_SECURITY_PATHS)
        self.assertEqual(set(entries), REQUIRED_KERNEL_SECURITY_PATHS)
        self.assertEqual(inventory["entry_count"], len(REQUIRED_KERNEL_SECURITY_PATHS))
        canonical_entries = json.dumps(inventory["entries"], sort_keys=True, separators=(",", ":")).encode()
        self.assertEqual(hashlib.sha256(canonical_entries).hexdigest(), inventory["inventory_digest"])
        for required_path, entry in entries.items():
            source = ROOT / required_path
            self.assertTrue(source.is_file())
            self.assertTrue(entry["roles"])
            self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), entry["sha256"])

    def test_cli_is_byte_deterministic_and_never_promotional(self) -> None:
        commands = [subprocess.run([sys.executable, str(CLI), str(FIXTURES / "valid-pass.json")], check=False, capture_output=True) for _ in range(2)]
        self.assertEqual((commands[0].returncode, commands[0].stdout), (commands[1].returncode, commands[1].stdout))
        self.assertFalse(json.loads(commands[0].stdout)["promotion_eligible"])


if __name__ == "__main__":
    unittest.main()
