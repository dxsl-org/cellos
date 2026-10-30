#!/usr/bin/env python3
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-ssh-dev-persist-exclusion.py")
SPEC = importlib.util.spec_from_file_location("ssh_dev_persist_checker", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)

CUSTODY_BANNER = b"[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf"
PERSIST_BANNER = (
    b"[kms] DEV_POSTURE ssh-dev-persist: wrapped identity persisted to /srv, "
    b"dev root key co-resident, no rollback or crash-durability claim"
)
ROOT_LABEL = b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET"
CLEAN = b"\x7fELF fail-closed node identity service starting"
CHUNK = 64 * 1024


def run(features: str, payload: bytes | None, *extra: str):
    with tempfile.TemporaryDirectory() as directory:
        artifact = Path(directory) / "kms"
        if payload is not None:
            artifact.write_bytes(payload)
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--kms-features",
                features,
                "--kms-artifact",
                str(artifact),
                *extra,
            ],
            capture_output=True,
            text=True,
            check=False,
        )


class FeatureNamesTests(unittest.TestCase):
    def test_features_match_by_exact_name(self) -> None:
        self.assertEqual(
            CHECKER.feature_set("a,ssh-dev-persist, b  ssh-dev-custody"),
            {"a", "b", "ssh-dev-persist", "ssh-dev-custody"},
        )
        # Near misses are different names.
        for near in ("xssh-dev-persist", "ssh-dev-persist-x", "ssh-dev-custodyy"):
            with self.subTest(near=near):
                completed = run(near, CLEAN)
                self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_none_rejects_each_feature_by_exact_name(self) -> None:
        for feature in ("ssh-dev-custody", "ssh-dev-persist"):
            with self.subTest(feature=feature):
                completed = run(f"hardware-relay-provider,{feature}", CLEAN)
                self.assertEqual(completed.returncode, 1)
                self.assertIn(f"forbidden feature: {feature}", completed.stderr)

    def test_none_default_accepts_a_clean_production_like_artifact(self) -> None:
        completed = run("hardware-relay-provider", CLEAN)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("PASS: --expect none", completed.stdout)


class MarkerTests(unittest.TestCase):
    def test_each_marker_in_the_artifact_fails_none(self) -> None:
        for marker in CHECKER.MARKERS:
            with self.subTest(marker=marker):
                completed = run("", CLEAN + b" " + marker)
                self.assertEqual(completed.returncode, 1)
                self.assertIn(
                    f"forbidden marker present: {marker.decode('ascii')}",
                    completed.stderr,
                )

    def test_the_marker_set_is_the_closed_five(self) -> None:
        self.assertEqual(
            [marker.decode("ascii") for marker in CHECKER.MARKERS],
            [
                "CELLOS-KSSH-T1-DEV-ROOT-v1",
                "DEV_POSTURE ssh-dev-persist",
                "DEV_POSTURE ssh-dev-custody",
                "ssh-dev-persist",
                "ssh-dev-custody",
            ],
        )

    def test_every_marker_is_in_the_production_checker_frozen_set(self) -> None:
        # Read-only import of the Phase 02 checker: B1 hand-off (the production
        # checker rejects the same five names), so a removal there fails here too.
        path = Path(__file__).with_name("check-production-relay-image.py")
        spec = importlib.util.spec_from_file_location("production_relay_checker", path)
        assert spec is not None and spec.loader is not None
        production = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(production)
        self.assertLessEqual(
            {marker.decode("ascii") for marker in CHECKER.MARKERS},
            production.DEV_MARKER_NAMES,
        )

    def test_a_marker_split_across_a_chunk_boundary_is_detected(self) -> None:
        for marker in CHECKER.MARKERS:
            with self.subTest(marker=marker):
                for shift in (1, len(marker) // 2, len(marker) - 1):
                    filler = b"\0" * (CHUNK - shift)
                    completed = run("", filler + marker)
                    self.assertEqual(completed.returncode, 1, shift)
                    self.assertIn(marker.decode("ascii"), completed.stderr)

    def test_a_marker_ending_exactly_on_the_boundary_is_detected(self) -> None:
        marker = CHECKER.ROOT_LABEL
        completed = run("", b"\0" * (CHUNK - len(marker)) + marker + CLEAN)
        self.assertEqual(completed.returncode, 1)

    def test_missing_and_empty_artifacts_fail(self) -> None:
        for expect in ("none", "custody", "persist"):
            with self.subTest(expect=expect, kind="missing"):
                completed = run("ssh-dev-persist", None, "--expect", expect)
                self.assertEqual(completed.returncode, 1)
                self.assertIn("missing or empty artifact", completed.stderr)
            with self.subTest(expect=expect, kind="empty"):
                completed = run("ssh-dev-persist", b"", "--expect", expect)
                self.assertEqual(completed.returncode, 1)
                self.assertIn("missing or empty artifact", completed.stderr)


class ExpectModeTests(unittest.TestCase):
    def test_custody_both_ways(self) -> None:
        ok = run("ssh-dev-custody", CLEAN + CUSTODY_BANNER, "--expect", "custody")
        self.assertEqual(ok.returncode, 0, ok.stderr)
        no_banner = run("ssh-dev-custody", CLEAN, "--expect", "custody")
        self.assertEqual(no_banner.returncode, 1)
        self.assertIn("expected marker missing: DEV_POSTURE ssh-dev-custody", no_banner.stderr)
        with_label = run(
            "ssh-dev-custody", CLEAN + CUSTODY_BANNER + ROOT_LABEL, "--expect", "custody"
        )
        self.assertEqual(with_label.returncode, 1)
        self.assertIn("forbidden marker present: CELLOS-KSSH-T1-DEV-ROOT-v1", with_label.stderr)
        persist_binary = run(
            "ssh-dev-custody", CLEAN + PERSIST_BANNER + ROOT_LABEL, "--expect", "custody"
        )
        self.assertEqual(persist_binary.returncode, 1)
        wrong_features = run(
            "ssh-dev-custody,ssh-dev-persist", CLEAN + CUSTODY_BANNER, "--expect", "custody"
        )
        self.assertEqual(wrong_features.returncode, 1)
        self.assertIn("forbidden feature: ssh-dev-persist", wrong_features.stderr)
        missing_feature = run("", CLEAN + CUSTODY_BANNER, "--expect", "custody")
        self.assertEqual(missing_feature.returncode, 1)

    def test_persist_both_ways(self) -> None:
        ok = run(
            "ssh-dev-persist", CLEAN + PERSIST_BANNER + ROOT_LABEL, "--expect", "persist"
        )
        self.assertEqual(ok.returncode, 0, ok.stderr)
        both_features = run(
            "ssh-dev-custody,ssh-dev-persist",
            CLEAN + PERSIST_BANNER + ROOT_LABEL,
            "--expect",
            "persist",
        )
        self.assertEqual(both_features.returncode, 0, both_features.stderr)
        no_label = run("ssh-dev-persist", CLEAN + PERSIST_BANNER, "--expect", "persist")
        self.assertEqual(no_label.returncode, 1)
        self.assertIn(
            "expected marker missing: CELLOS-KSSH-T1-DEV-ROOT-v1", no_label.stderr
        )
        no_banner = run("ssh-dev-persist", CLEAN + ROOT_LABEL, "--expect", "persist")
        self.assertEqual(no_banner.returncode, 1)
        custody_binary = run(
            "ssh-dev-persist", CLEAN + CUSTODY_BANNER, "--expect", "persist"
        )
        self.assertEqual(custody_binary.returncode, 1)
        with_custody_banner = run(
            "ssh-dev-persist",
            CLEAN + PERSIST_BANNER + ROOT_LABEL + CUSTODY_BANNER,
            "--expect",
            "persist",
        )
        self.assertEqual(with_custody_banner.returncode, 1)
        self.assertIn(
            "forbidden marker present: DEV_POSTURE ssh-dev-custody",
            with_custody_banner.stderr,
        )
        no_feature = run("", CLEAN + PERSIST_BANNER + ROOT_LABEL, "--expect", "persist")
        self.assertEqual(no_feature.returncode, 1)

    def test_none_fails_both_dev_variants(self) -> None:
        for payload in (CUSTODY_BANNER, PERSIST_BANNER + ROOT_LABEL):
            with self.subTest(payload=payload[:40]):
                completed = run("", CLEAN + payload, "--expect", "none")
                self.assertEqual(completed.returncode, 1)

    def test_a_marker_split_across_a_boundary_decides_the_expect_modes(self) -> None:
        filler = b"\0" * (CHUNK - 10)
        completed = run(
            "ssh-dev-persist",
            filler + PERSIST_BANNER + b" " + ROOT_LABEL,
            "--expect",
            "persist",
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)


class UsageTests(unittest.TestCase):
    def test_missing_required_arguments_and_bad_expect_exit_two(self) -> None:
        for arguments in (
            [],
            ["--kms-features", ""],
            ["--kms-artifact", "x"],
            ["--kms-features", "", "--kms-artifact", "x", "--expect", "both"],
        ):
            with self.subTest(arguments=arguments):
                completed = subprocess.run(
                    [sys.executable, str(SCRIPT), *arguments],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(completed.returncode, 2)


if __name__ == "__main__":
    unittest.main()
