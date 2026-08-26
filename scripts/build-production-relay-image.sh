#!/usr/bin/env bash
# Validate candidate inputs, then stop: Phase 1 cannot build a production image.

set -euo pipefail

TARGET="${1:?usage: build-production-relay-image.sh <target>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PRODUCT="${CELLOS_PRODUCTION_ROOT_PRODUCT:-}"
FIRMWARE_SHA256="${CELLOS_PRODUCTION_ROOT_FIRMWARE_SHA256:-}"
PROVIDER_SOURCE="${CELLOS_PRODUCTION_ROOT_PROVIDER_SOURCE:-}"
CA_FEATURE="${CELLOS_RELAY_CA_FEATURE:-tls-ca-private}"

[[ -n "$PRODUCT" ]] || {
    echo "FAIL: Phase 6 must set CELLOS_PRODUCTION_ROOT_PRODUCT" >&2
    exit 2
}
[[ "$FIRMWARE_SHA256" =~ ^[0-9a-fA-F]{64}$ ]] || {
    echo "FAIL: Phase 6 must set a 64-hex CELLOS_PRODUCTION_ROOT_FIRMWARE_SHA256" >&2
    exit 2
}
[[ -n "$PROVIDER_SOURCE" && -f "$PROVIDER_SOURCE" ]] || {
    echo "FAIL: selected product provider source is missing" >&2
    exit 2
}
case "$PROVIDER_SOURCE" in
    cells/services/kms/src/storage/*.rs) ;;
    *)
        echo "FAIL: provider source must be owned by KMS storage" >&2
        exit 2
        ;;
esac
case "$CA_FEATURE" in
    tls-ca-private|tls-ca-amazon|tls-ca-letsencrypt|tls-ca-rsa) ;;
    *)
        echo "FAIL: unsupported production relay CA feature" >&2
        exit 2
        ;;
esac

echo "BLOCKED_BY_ADR_0006: production relay images require a superseding GO ADR, an implemented hardware provider, hardware qualification, and authenticated build provenance" >&2
exit 3
