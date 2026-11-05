#!/usr/bin/env python3
"""Reject unsafe candidates and block unqualified production relay artifacts."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import sys

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_NET_ROOT = REPO_ROOT / "cells" / "services" / "net" / "roots" / "private.der"
# The "Cellos Dev CA" whose private key was committed as
# tools/hypha-mock-llm/ca-key.pem: anyone with git history can mint a server
# certificate it accepts. Same value as LEAKED_DEV_CA_SHA256 in
# cells/services/net/build.rs. The length narrows the artifact scan to DER
# SEQUENCE headers of exactly that size (30 82 01 57).
LEAKED_DEV_CA_SHA256 = "36335e7653f318be9a4024afa553dd81e828ae8a9745de839b57c1f9775925d1"
LEAKED_DEV_CA_LEN = 347

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
# production-relay-image already implies release-signing-key in kernel/Cargo.toml;
# requiring both by name keeps the zero-anchor posture from passing review.
KERNEL_REQUIRED = {"production-relay-image", "release-signing-key"}
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
    "CELLOS-KSSH-T1-DEV-ROOT-v1",
    "DEV_POSTURE ssh-dev-custody",
    "DEV_POSTURE ssh-dev-persist",
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
    "ssh-dev-custody",
    "ssh-dev-persist",
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
    if missing := KERNEL_REQUIRED - kernel:
        errors.append(f"kernel must select {','.join(sorted(missing))}")
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
    if embeds_leaked_dev_ca(path.read_bytes()):
        errors.append(f"{path} embeds the leaked Cellos Dev CA ({LEAKED_DEV_CA_SHA256})")
    return errors


def is_leaked_dev_ca(der: bytes) -> bool:
    return hashlib.sha256(der).hexdigest() == LEAKED_DEV_CA_SHA256


def embeds_leaked_dev_ca(data: bytes) -> bool:
    """Hash every DER SEQUENCE whose header announces the leaked certificate's size."""
    header = b"\x30\x82" + (LEAKED_DEV_CA_LEN - 4).to_bytes(2, "big")
    start = data.find(header)
    while start != -1:
        if is_leaked_dev_ca(data[start : start + LEAKED_DEV_CA_LEN]):
            return True
        start = data.find(header, start + 1)
    return False


def net_root_errors(net_root: Path, net_features: str) -> list[str]:
    """The private-CA selector embeds net_root verbatim (service-net roots.rs)."""
    if "tls-ca-private" not in feature_set(net_features):
        return []
    if not net_root.is_file():
        return [f"missing net trust root: {net_root}"]
    if is_leaked_dev_ca(net_root.read_bytes()):
        return [f"{net_root} is the leaked Cellos Dev CA ({LEAKED_DEV_CA_SHA256})"]
    return []


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--kms-features", required=True)
    parser.add_argument("--net-features", required=True)
    parser.add_argument("--kernel-features", required=True)
    parser.add_argument("--kms-artifact", required=True, type=Path)
    parser.add_argument("--net-artifact", required=True, type=Path)
    parser.add_argument("--kernel-artifact", required=True, type=Path)
    parser.add_argument("--net-root", type=Path, default=DEFAULT_NET_ROOT)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    errors = require_exact_posture(arguments)
    errors.extend(net_root_errors(arguments.net_root, arguments.net_features))
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
