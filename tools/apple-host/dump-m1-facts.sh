#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Dump hardware facts from physical Mac mini M1 into evidence directory (Phase 01, AS-1).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVIDENCE_DIR="$ROOT/.agents/261104-0711-apple-silicon-group/evidence"
mkdir -p "$EVIDENCE_DIR"

echo "==> Capturing Mac mini M1 hardware facts to $EVIDENCE_DIR..."

if command -v lspci >/dev/null 2>&1; then
    lspci -nn > "$EVIDENCE_DIR/pci-ids.txt" || true
    echo "    Captured PCI IDs: $EVIDENCE_DIR/pci-ids.txt"
fi

if [[ -d "/sys/firmware/devicetree/base" ]]; then
    dtc -I fs -O dtb /sys/firmware/devicetree/base -o "$EVIDENCE_DIR/devicetree.dtb" 2>/dev/null || true
    echo "    Captured Device Tree: $EVIDENCE_DIR/devicetree.dtb"
fi

if [[ -f "/proc/cpuinfo" ]]; then
    cp /proc/cpuinfo "$EVIDENCE_DIR/cpuinfo.txt"
    echo "    Captured CPU info: $EVIDENCE_DIR/cpuinfo.txt"
fi

# Generate SHA-256 manifest
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$EVIDENCE_DIR" && sha256sum * > evidence-sha256.txt 2>/dev/null || true)
    echo "    Generated SHA-256 manifest: $EVIDENCE_DIR/evidence-sha256.txt"
fi

echo "==> Fact capture complete."
