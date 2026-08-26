#!/usr/bin/env python3
"""Reject unsafe candidates and block unqualified production relay artifacts."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

PROVIDERS = {
    "hardware-relay-provider",
    "development-silo-provider",
    "fixture-provider",
}
KMS_FORBIDDEN = {
    "development-silo-provider",
    "fixture-provider",
    "test-hooks",
    "raw-relay-provider",
    "k1-fallback",
}
NET_FORBIDDEN = {"tls-insecure", "raw-relay", "k1-fallback"}
KERNEL_FORBIDDEN = {
    "dev-policy-key",
    "dev-signing-key",
    "dev-weak-rng",
    "test-hooks",
    "maintenance-mode",
}
CA_FEATURES = {
    "tls-ca-private",
    "tls-ca-amazon",
    "tls-ca-letsencrypt",
    "tls-ca-rsa",
}
FORBIDDEN_MARKERS = tuple(
    marker.encode("ascii")
    for marker in sorted(KMS_FORBIDDEN | NET_FORBIDDEN | KERNEL_FORBIDDEN)
)


def feature_set(value: str) -> set[str]:
    return {item.strip() for item in value.split(",") if item.strip()}


def require_exact_posture(arguments: argparse.Namespace) -> list[str]:
    errors: list[str] = []
    kms = feature_set(arguments.kms_features)
    net = feature_set(arguments.net_features)
    kernel = feature_set(arguments.kernel_features)

    if kms & PROVIDERS != {"hardware-relay-provider"}:
        errors.append("KMS must select exactly hardware-relay-provider")
    if forbidden := kms & KMS_FORBIDDEN:
        errors.append(f"KMS forbidden features: {','.join(sorted(forbidden))}")
    if not {"verified-tls", "tls-roots-embedded"} <= net:
        errors.append("net must select verified-tls and tls-roots-embedded")
    if len(net & CA_FEATURES) != 1:
        errors.append("net must select exactly one trusted CA feature")
    if forbidden := net & NET_FORBIDDEN:
        errors.append(f"net forbidden features: {','.join(sorted(forbidden))}")
    if "production-relay-image" not in kernel:
        errors.append("kernel must select production-relay-image")
    if forbidden := kernel & KERNEL_FORBIDDEN:
        errors.append(f"kernel forbidden features: {','.join(sorted(forbidden))}")
    return errors


def scan_artifact(path: Path) -> list[str]:
    if not path.is_file() or path.stat().st_size == 0:
        return [f"missing or empty artifact: {path}"]
    found: set[bytes] = set()
    overlap = max(map(len, FORBIDDEN_MARKERS)) - 1
    previous = b""
    with path.open("rb") as artifact:
        while chunk := artifact.read(64 * 1024):
            window = previous + chunk
            found.update(marker for marker in FORBIDDEN_MARKERS if marker in window)
            previous = window[-overlap:]
    return [
        f"{path} contains forbidden marker: {marker.decode('ascii')}"
        for marker in sorted(found)
    ]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--kms-features", required=True)
    parser.add_argument("--net-features", required=True)
    parser.add_argument("--kernel-features", required=True)
    parser.add_argument("--kms-artifact", required=True, type=Path)
    parser.add_argument("--net-artifact", required=True, type=Path)
    parser.add_argument("--kernel-artifact", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    errors = require_exact_posture(arguments)
    artifacts = {
        arguments.kms_artifact,
        arguments.net_artifact,
        arguments.kernel_artifact,
    }
    if len(artifacts) != 3:
        errors.append("KMS, net, and kernel artifacts must be distinct")
    for artifact in artifacts:
        errors.extend(scan_artifact(artifact))
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(
        "BLOCKED_BY_ADR_0006: production relay images require a superseding "
        "GO ADR, an implemented hardware provider, hardware qualification, "
        "and authenticated build provenance",
        file=sys.stderr,
    )
    return 3


if __name__ == "__main__":
    raise SystemExit(main())
