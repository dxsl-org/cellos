#!/usr/bin/env python3
import argparse
import importlib.util
from pathlib import Path
import unittest

MODULE_PATH = Path(__file__).with_name("check-production-relay-image.py")
SPEC = importlib.util.spec_from_file_location("production_relay_checker", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


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


if __name__ == "__main__":
    unittest.main()
