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
FROZEN_DEV_MARKERS = [
    "AWS_DEV_SIGNED_TIME",
    "DEV_REFERENCE",
    "SLB9672",
    "SOFTWARE_HARNESS",
    "STM32H573I-DK",
    "TPM9672FW1523PCEBTOBO1",
    "aws-dev-signed-time",
    "cellos-dev-time-v1",
    "dev-reference",
    "dev-reference.toml",
    "development-stm32-authority",
    "root-stream",
    "slb9672-dev-anchor",
    "stm32h573i-dk-dev-authority",
    "vf2-root-stream",
]
DEV_MARKER_NAMES = frozenset(FROZEN_DEV_MARKERS)
DEV_MARKERS = tuple(marker.encode("ascii") for marker in FROZEN_DEV_MARKERS)


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
    for label, selected in (("KMS", kms), ("net", net), ("kernel", kernel)):
        if hits := sorted(selected & DEV_MARKER_NAMES):
            errors.append(f"{label} forbidden DEV marker: {','.join(hits)}")
    return errors


def scan_artifact(path: Path) -> list[str]:
    if not path.is_file() or path.stat().st_size == 0:
        return [f"missing or empty artifact: {path}"]
    all_markers = FORBIDDEN_MARKERS + DEV_MARKERS
    overlap = max(map(len, all_markers)) - 1
    found: set[bytes] = set()
    previous = b""
    with path.open("rb") as artifact:
        while chunk := artifact.read(64 * 1024):
            window = previous + chunk
            found.update(marker for marker in all_markers if marker in window)
            previous = window[-overlap:]
    errors = [
        f"{path} contains forbidden marker: {marker.decode('ascii')}"
        for marker in sorted(found & set(FORBIDDEN_MARKERS))
    ]
    errors.extend(
        f"{path} contains forbidden DEV marker: {marker.decode('ascii')}"
        for marker in sorted(found & set(DEV_MARKERS))
    )
    return errors


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
