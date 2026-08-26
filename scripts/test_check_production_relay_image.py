#!/usr/bin/env python3
import argparse
import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

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


def posture(kms_features: str) -> argparse.Namespace:
    return argparse.Namespace(
        kms_features=kms_features,
        net_features="verified-tls,tls-roots-embedded,tls-ca-private",
        kernel_features="production-relay-image",
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
                    "production-relay-image",
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
