"""Behavioral tests for pinned Phase 04 prequalification validation."""

from __future__ import annotations

import copy
import json
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from admission_prequalification import validator  # noqa: E402

CATALOG_BYTES = (ROOT / "scripts/admission_prequalification/catalog.json").read_bytes()
CATALOG = json.loads(CATALOG_BYTES)
CLI = ROOT / "scripts/validate-admission-prequalification.py"


def log_bytes(*, terminator: bool = True) -> bytes:
    lines = [f"[INFO] [selftest] {case_id}: PASS\n" for case_id in validator.EXPECTED_RUNTIME_IDS]
    if terminator:
        lines.append(f"[INFO] {validator.AGGREGATE_PASS}\n")
    return "".join(lines).encode()


def changed_catalog(change) -> bytes:
    catalog = copy.deepcopy(CATALOG)
    change(catalog)
    return json.dumps(catalog, indent=2).encode() + b"\n"


class CatalogPinTests(unittest.TestCase):
    def reject_catalog(self, change) -> None:
        with self.assertRaisesRegex(ValueError, "authoritative pinned catalog"):
            validator.validate_catalog_bytes(changed_catalog(change))

    def test_exact_authoritative_catalog_and_mappings_are_pinned(self) -> None:
        self.assertEqual(validator.validate_catalog_bytes(CATALOG_BYTES), validator.EXPECTED_IDS)
        self.assertEqual(validator.digest_bytes(CATALOG_BYTES), validator.CATALOG_SHA256)

    def test_any_byte_only_change_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "authoritative pinned catalog"):
            validator.validate_catalog_bytes(CATALOG_BYTES + b"\n")

    def test_semantic_row_relabel_is_rejected(self) -> None:
        def relabel(item) -> None:
            first, second = item["matrix"][7], item["matrix"][8]
            first["scenario"], second["scenario"] = second["scenario"], first["scenario"]
            first["required_result"], second["required_result"] = second["required_result"], first["required_result"]

        self.reject_catalog(relabel)

    def test_executable_name_and_mapping_swap_is_rejected(self) -> None:
        def swap(item) -> None:
            first, second = item["executables"][0], item["executables"][1]
            first["name"], second["name"] = second["name"], first["name"]
            first["matrix_rows"], second["matrix_rows"] = second["matrix_rows"], first["matrix_rows"]
            for row in item["matrix"]:
                row["executable_ids"] = [
                    "C3-ADM-002" if value == "C3-ADM-001" else
                    "C3-ADM-001" if value == "C3-ADM-002" else value
                    for value in row["executable_ids"]
                ]

        self.reject_catalog(swap)

    def test_promotion_is_rejected(self) -> None:
        self.reject_catalog(lambda item: item.update(production_admission="ENABLED"))


class CatalogCliTests(unittest.TestCase):
    def run_cli(self, *arguments: str) -> subprocess.CompletedProcess[bytes]:
        return subprocess.run(
            [sys.executable, CLI, *arguments],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

    def test_cli_validates_only_the_authoritative_catalog(self) -> None:
        completed = self.run_cli()
        self.assertEqual(completed.returncode, 0)
        self.assertIn(b"PREQUALIFICATION INFRASTRUCTURE COMPLETE", completed.stdout)
        self.assertIn(b"ADMISSIBLE EVIDENCE BLOCKED", completed.stdout)
        self.assertEqual(completed.stderr, b"")

    def test_cli_rejects_capture_and_caller_execution_inputs(self) -> None:
        rejected = (
            ("--capture",),
            ("--log", "caller.log"),
            ("--context", "caller.json"),
            ("--kernel", "caller-kernel"),
            ("--backend", "caller-backend"),
            ("--catalog", "caller-catalog"),
            ("--source", "caller-source"),
            ("--output", "caller-output"),
        )
        for arguments in rejected:
            with self.subTest(arguments=arguments):
                completed = self.run_cli(*arguments)
                self.assertEqual(completed.returncode, 2)
                self.assertIn(b"unrecognized arguments", completed.stderr)


class RuntimeLogTests(unittest.TestCase):
    def reject_log(self, content: bytes) -> None:
        with self.assertRaises(ValueError):
            validator.validate_log(content)

    def test_ordered_unique_cases_and_one_trailing_terminator_pass(self) -> None:
        self.assertEqual(validator.validate_log(log_bytes()), validator.EXPECTED_RUNTIME_IDS)

    def test_failed_missing_duplicate_unknown_and_reordered_cases_are_rejected(self) -> None:
        good = log_bytes()
        self.reject_log(good.replace(b"C3-ADM-007: PASS", b"C3-ADM-007: FAIL"))
        self.reject_log(good.replace(b"[INFO] [selftest] C3-ADM-033: PASS\n", b""))
        self.reject_log(good.replace(b"[INFO] [selftest] C3-ADM-001: PASS\n", b"[INFO] [selftest] C3-ADM-001: PASS\n" * 2))
        self.reject_log(good.replace(b"C3-ADM-001: PASS", b"C3-ADM-999: PASS"))
        first = b"[INFO] [selftest] C3-ADM-001: PASS\n"
        second = b"[INFO] [selftest] C3-ADM-002: PASS\n"
        self.reject_log(good.replace(first + second, second + first))

    def test_truncation_after_last_case_is_rejected(self) -> None:
        self.reject_log(log_bytes(terminator=False))

    def test_terminator_before_last_case_is_rejected(self) -> None:
        cases = log_bytes(terminator=False)
        final = f"[INFO] [selftest] {validator.EXPECTED_RUNTIME_IDS[-1]}: PASS\n".encode()
        terminator = f"[INFO] {validator.AGGREGATE_PASS}\n".encode()
        self.reject_log(cases.replace(final, terminator + final))

    def test_duplicate_or_failed_aggregate_is_rejected(self) -> None:
        terminator = f"[INFO] {validator.AGGREGATE_PASS}\n".encode()
        self.reject_log(log_bytes() + terminator)
        self.reject_log(log_bytes() + f"[INFO] {validator.AGGREGATE_FAIL}\n".encode())
        self.reject_log(log_bytes() + b"[INFO] unrelated self-test FAIL\n")
        self.reject_log(log_bytes() + b"[INFO] unrelated summary: 1 FAIL\n")
        self.reject_log(log_bytes() + b"[INFO] summary: 0 FAIL; another FAIL\n")

    def test_single_numeric_zero_fail_summary_is_accepted(self) -> None:
        validator.validate_log(log_bytes() + b"USER: [vfs-test] Results: 84 PASS, 0 FAIL\n")


if __name__ == "__main__":
    unittest.main()
