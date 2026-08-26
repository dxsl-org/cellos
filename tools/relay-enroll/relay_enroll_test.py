"""Unit tests for the supervisor enrollment planner."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from relay_enroll import (
    CHUNK_CAPACITY,
    CSR_MAX_BYTES,
    EnrollmentPlanError,
    ManifestFacts,
    check_chunk_sequence,
    load_enrollment_facts,
    plan_enrollment,
)


def facts() -> ManifestFacts:
    return ManifestFacts(
        hostname="relay.example.internal",
        pending_generation=13,
        node_id_sha256="ab" * 32,
        active_ca_sha256="cd" * 32,
        next_ca_sha256=None,
        policy_epoch=12,
    )


class ManifestTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls._temp.name)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._temp.cleanup()

    def _write(self, text: str) -> Path:
        manifest = self.root / "enrollment-manifest.toml"
        manifest.write_text(text, encoding="utf-8")
        return manifest

    def _valid_template(self) -> str:
        template = Path(__file__).with_name(
            "mtls-mount-manifest.template.toml"
        ).read_text(encoding="utf-8")
        replacements = {
            'hostname = "REQUIRED: DNS name in relay server certificate SAN"': (
                'hostname = "relay.example.internal"'
            ),
            'active_ca_sha256 = "REQUIRED: 64 lowercase hex characters"': (
                f'active_ca_sha256 = "{"0" * 64}"'
            ),
            'next_ca_sha256 = "OPTIONAL: 64 lowercase hex characters during overlap"': (
                'next_ca_sha256 = ""'
            ),
            'node_id_sha256 = "REQUIRED: 64 lowercase hex characters"': (
                f'node_id_sha256 = "{"1" * 64}"'
            ),
        }
        for placeholder, value in replacements.items():
            template = template.replace(placeholder, value)
        return template

    def test_valid_phase_three_template_has_explicit_enrollment_facts(self) -> None:
        loaded = load_enrollment_facts(self._write(self._valid_template()))
        self.assertEqual(loaded.hostname, "relay.example.internal")
        self.assertEqual(loaded.pending_generation, 1)
        self.assertEqual(loaded.policy_epoch, 1)

    def test_enrollment_must_be_a_table(self) -> None:
        before_enrollment, separator, _ = self._valid_template().partition(
            "[enrollment]\n"
        )
        self.assertTrue(separator)
        manifest = self._write("enrollment = 1\n\n" + before_enrollment)
        with self.assertRaisesRegex(EnrollmentPlanError, r"\[enrollment\] must be a table"):
            load_enrollment_facts(manifest)

    def test_unknown_enrollment_field_is_rejected(self) -> None:
        manifest = self._write(
            self._valid_template().replace(
                "policy_epoch = 1", "policy_epoch = 1\npolicy_epcoh = 1"
            )
        )
        with self.assertRaisesRegex(
            EnrollmentPlanError, r"unexpected enrollment\.policy_epcoh"
        ):
            load_enrollment_facts(manifest)

    def test_missing_required_enrollment_fields_are_rejected(self) -> None:
        for field in ("pending_generation", "policy_epoch"):
            with self.subTest(field=field):
                manifest = self._write(
                    self._valid_template().replace(f"{field} = 1\n", "")
                )
                with self.assertRaisesRegex(
                    EnrollmentPlanError, rf"missing enrollment\.{field}"
                ):
                    load_enrollment_facts(manifest)

    def test_enrollment_fields_are_positive_u64_integers(self) -> None:
        invalid_values = ("true", "0", str(1 << 64))
        for field in ("pending_generation", "policy_epoch"):
            for invalid in invalid_values:
                with self.subTest(field=field, value=invalid):
                    manifest = self._write(
                        self._valid_template().replace(
                            f"{field} = 1", f"{field} = {invalid}"
                        )
                    )
                    with self.assertRaisesRegex(
                        EnrollmentPlanError, rf"enrollment\.{field} must be an integer"
                    ):
                        load_enrollment_facts(manifest)


class PlanTest(unittest.TestCase):
    def test_commit_only_after_staging(self) -> None:
        plan = plan_enrollment(facts(), "77" * 32)
        self.assertEqual(plan.ops[0][0], 9)
        self.assertEqual(plan.ops[-1][0], 11)
        # Chunk reads are strictly ordered and bounded.
        reads = [op for op in plan.ops if op[0] == 10]
        self.assertEqual(len(reads), (CSR_MAX_BYTES + CHUNK_CAPACITY - 1) // CHUNK_CAPACITY)

    def test_unstaged_enrollment_refuses_to_commit(self) -> None:
        with self.assertRaises(EnrollmentPlanError):
            plan_enrollment(facts(), None)

    def test_staged_profile_digest_must_be_exact_sha256_hex(self) -> None:
        for invalid in ("", "7" * 63, "G" * 64, "7" * 65):
            with self.subTest(value=invalid):
                with self.assertRaisesRegex(
                    EnrollmentPlanError,
                    "staged profile digest must be 64 lowercase hex characters",
                ):
                    plan_enrollment(facts(), invalid)

    def test_ordered_chunks_reassemble_exactly(self) -> None:
        csr = bytes(range(256)) * 3
        chunks = [csr[i : i + CHUNK_CAPACITY] for i in range(0, len(csr), CHUNK_CAPACITY)]
        self.assertEqual(check_chunk_sequence(chunks, len(csr)), csr)

    def test_oversized_or_torn_sequences_fail_closed(self) -> None:
        with self.assertRaises(EnrollmentPlanError):
            check_chunk_sequence([], CSR_MAX_BYTES + 1)
        torn = [b"a" * CHUNK_CAPACITY, b""]
        with self.assertRaises(EnrollmentPlanError):
            check_chunk_sequence(torn, CHUNK_CAPACITY)
        short = [b"a" * 8]
        with self.assertRaises(EnrollmentPlanError):
            check_chunk_sequence(short, 16)


if __name__ == "__main__":
    unittest.main()
