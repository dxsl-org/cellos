#!/usr/bin/env python3
import argparse
import hashlib
import importlib.util
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("check-production-relay-image.py")
SPEC = importlib.util.spec_from_file_location("production_relay_checker", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)

BLOCKED_MESSAGE = (
    "BLOCKED_BY_ADR_0006: production relay images require a superseding "
    "GO ADR, an implemented hardware provider, hardware qualification, "
    "and authenticated build provenance"
)
KERNEL_FEATURES = "production-relay-image,release-signing-key"


def posture(kms_features: str) -> argparse.Namespace:
    return argparse.Namespace(
        kms_features=kms_features,
        net_features="verified-tls,tls-roots-embedded,tls-ca-private",
        kernel_features=KERNEL_FEATURES,
    )


class UnsafeFeatureMatrixTests(unittest.TestCase):
    def test_development_silo_provider_is_rejected_by_exact_name(self) -> None:
        errors = CHECKER.require_exact_posture(
            posture("hardware-relay-provider,development-silo-provider")
        )
        self.assertIn(
            "KMS forbidden features: development-silo-provider",
            errors,
        )

    def test_hardware_only_posture_has_no_feature_error(self) -> None:
        self.assertEqual(
            CHECKER.require_exact_posture(posture("hardware-relay-provider")),
            [],
        )

    def test_production_relay_image_without_release_anchor_is_rejected(self) -> None:
        arguments = posture("hardware-relay-provider")
        arguments.kernel_features = "production-relay-image"
        self.assertIn(
            "kernel must select release-signing-key",
            CHECKER.require_exact_posture(arguments),
        )


class LeakedDevCaTests(unittest.TestCase):
    LEAKED = "36335e7653f318be9a4024afa553dd81e828ae8a9745de839b57c1f9775925d1"

    @staticmethod
    def fake_root() -> bytes:
        # Same DER header and size as the leaked cert; the constant is patched to
        # its digest because a real preimage would require committing the cert.
        body = b"\x5a" * (CHECKER.LEAKED_DEV_CA_LEN - 4)
        return b"\x30\x82" + len(body).to_bytes(2, "big") + body

    def test_fingerprint_matches_the_service_net_build_guard(self) -> None:
        build_rs = (MODULE_PATH.parents[1] / "cells/services/net/build.rs").read_text()
        self.assertEqual(CHECKER.LEAKED_DEV_CA_SHA256, self.LEAKED)
        self.assertEqual(re.findall(r'"([0-9a-f]{64})"', build_rs), [self.LEAKED])

    def test_committed_private_root_is_not_the_leaked_ca(self) -> None:
        self.assertEqual(
            CHECKER.net_root_errors(CHECKER.DEFAULT_NET_ROOT, "tls-ca-private"), []
        )

    def test_leaked_private_root_is_rejected(self) -> None:
        root = self.fake_root()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "private.der"
            path.write_bytes(root)
            digest = hashlib.sha256(root).hexdigest()
            with mock.patch.object(CHECKER, "LEAKED_DEV_CA_SHA256", digest):
                errors = CHECKER.net_root_errors(path, "verified-tls,tls-ca-private")
                self.assertEqual(len(errors), 1)
                self.assertIn("is the leaked Cellos Dev CA", errors[0])
                # Other CA selectors do not embed private.der.
                self.assertEqual(CHECKER.net_root_errors(path, "tls-ca-amazon"), [])

    def test_leaked_root_embedded_in_an_artifact_is_rejected(self) -> None:
        root = self.fake_root()
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "net"
            artifact.write_bytes(b"\x30\x82\x01" + b"\0" * 4096 + root + b"tail")
            digest = hashlib.sha256(root).hexdigest()
            with mock.patch.object(CHECKER, "LEAKED_DEV_CA_SHA256", digest):
                errors = CHECKER.scan_artifact(artifact)
            self.assertEqual(len(errors), 1)
            self.assertIn("embeds the leaked Cellos Dev CA", errors[0])
            self.assertEqual(CHECKER.scan_artifact(artifact), [])


class DevMarkerRejectionTests(unittest.TestCase):
    CHUNK = 64 * 1024

    def test_signed_time_classification_handoff_is_frozen(self) -> None:
        self.assertLessEqual(
            {"DEV_REFERENCE", "SOFTWARE_HARNESS", "AWS_DEV_SIGNED_TIME",
             "aws-dev-signed-time", "cellos-dev-time-v1"},
            CHECKER.DEV_MARKER_NAMES,
        )

    def test_kssh_dev_custody_handoff_is_frozen(self) -> None:
        self.assertLessEqual(
            {"CELLOS-KSSH-T1-DEV-ROOT-v1", "DEV_POSTURE ssh-dev-custody",
             "DEV_POSTURE ssh-dev-persist", "ssh-dev-custody",
             "ssh-dev-persist"},
            CHECKER.DEV_MARKER_NAMES,
        )

    def run_checker(self, kms_features: str, artifact_payloads: list[bytes]):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            names = ("kms", "net", "kernel")
            artifacts = [Path(directory) / name for name in names]
            for artifact, payload in zip(artifacts, artifact_payloads):
                artifact.write_bytes(payload)
            return subprocess.run(
                [
                    sys.executable,
                    str(MODULE_PATH),
                    "--kms-features",
                    kms_features,
                    "--net-features",
                    "verified-tls,tls-roots-embedded,tls-ca-private",
                    "--kernel-features",
                    KERNEL_FEATURES,
                    "--kms-artifact",
                    str(artifacts[0]),
                    "--net-artifact",
                    str(artifacts[1]),
                    "--kernel-artifact",
                    str(artifacts[2]),
                ],
                cwd=root,
                capture_output=True,
                text=True,
                check=False,
            )

    def test_each_dev_marker_in_features_exits_one_naming_it(self) -> None:
        clean = b"qualified-candidate"
        for marker in CHECKER.FROZEN_DEV_MARKERS:
            with self.subTest(marker=marker):
                completed = self.run_checker(
                    f"hardware-relay-provider,{marker}",
                    [clean, clean, clean],
                )
                self.assertEqual(completed.returncode, 1)
                self.assertIn(
                    f"FAIL: KMS forbidden DEV marker: {marker}",
                    completed.stderr,
                )

    def test_each_dev_marker_in_artifact_content_exits_one_naming_it(self) -> None:
        clean = b"qualified-candidate"
        for marker in CHECKER.FROZEN_DEV_MARKERS:
            with self.subTest(marker=marker):
                payload = b"qualified-candidate " + marker.encode("ascii")
                completed = self.run_checker(
                    "hardware-relay-provider",
                    [payload, clean, clean],
                )
                self.assertEqual(completed.returncode, 1)
                self.assertIn(
                    f"contains forbidden DEV marker: {marker}",
                    completed.stderr,
                )

    def test_kssh_custody_and_persist_artifacts_exit_one_naming_markers(self) -> None:
        clean = b"qualified-candidate"
        banners = {
            "ssh-dev-custody": (
                b"[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf",
                ["DEV_POSTURE ssh-dev-custody", "ssh-dev-custody"],
            ),
            "ssh-dev-persist": (
                b"[kms] DEV_POSTURE ssh-dev-persist: wrapped identity persisted to "
                b"/srv, dev root key co-resident, no rollback or crash-durability claim "
                b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET",
                ["CELLOS-KSSH-T1-DEV-ROOT-v1", "DEV_POSTURE ssh-dev-persist",
                 "ssh-dev-persist"],
            ),
        }
        for variant, (payload, names) in banners.items():
            for slot in range(3):
                with self.subTest(variant=variant, slot=slot):
                    payloads = [clean, clean, clean]
                    payloads[slot] = payload
                    completed = self.run_checker("hardware-relay-provider", payloads)
                    self.assertEqual(completed.returncode, 1)
                    for name in names:
                        self.assertIn(
                            f"contains forbidden DEV marker: {name}",
                            completed.stderr,
                        )

    def test_dev_marker_split_across_chunk_boundary_is_detected(self) -> None:
        clean = b"qualified-candidate"
        for marker in CHECKER.FROZEN_DEV_MARKERS:
            encoded = marker.encode("ascii")
            with self.subTest(marker=marker):
                filler = b"\0" * (self.CHUNK - len(encoded) + 1)
                payload = filler + encoded
                completed = self.run_checker(
                    "hardware-relay-provider",
                    [clean, payload, clean],
                )
                self.assertEqual(completed.returncode, 1)
                self.assertIn(
                    f"contains forbidden DEV marker: {marker}",
                    completed.stderr,
                )


class ProductionBlockReasonTests(unittest.TestCase):
    def test_checker_reports_the_adr_block_after_validating_inputs(self) -> None:
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            artifacts = [Path(directory) / name for name in ("kms", "net", "kernel")]
            for artifact in artifacts:
                artifact.write_bytes(b"qualified-candidate")
            completed = subprocess.run(
                [
                    sys.executable,
                    str(MODULE_PATH),
                    "--kms-features",
                    "hardware-relay-provider",
                    "--net-features",
                    "verified-tls,tls-roots-embedded,tls-ca-private",
                    "--kernel-features",
                    KERNEL_FEATURES,
                    "--kms-artifact",
                    str(artifacts[0]),
                    "--net-artifact",
                    str(artifacts[1]),
                    "--kernel-artifact",
                    str(artifacts[2]),
                ],
                cwd=root,
                capture_output=True,
                text=True,
                check=False,
            )
        self.assertEqual(completed.returncode, 3)
        self.assertEqual(completed.stdout, "")
        self.assertEqual(completed.stderr, f"{BLOCKED_MESSAGE}\n")

    def test_builder_reports_the_same_adr_block(self) -> None:
        root = Path(__file__).resolve().parents[1]
        environment = os.environ.copy()
        environment.update(
            {
                "CELLOS_PRODUCTION_ROOT_PRODUCT": "reviewed-candidate",
                "CELLOS_PRODUCTION_ROOT_FIRMWARE_SHA256": "0" * 64,
                "CELLOS_PRODUCTION_ROOT_PROVIDER_SOURCE": (
                    "cells/services/kms/src/storage/capability.rs"
                ),
            }
        )
        completed = subprocess.run(
            ["bash", "scripts/build-production-relay-image.sh", "unused-target"],
            cwd=root,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 3)
        self.assertEqual(completed.stdout, "")
        self.assertEqual(completed.stderr, f"{BLOCKED_MESSAGE}\n")


if __name__ == "__main__":
    unittest.main()
