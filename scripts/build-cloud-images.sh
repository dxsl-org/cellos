#!/usr/bin/env bash
# Build Cellos guest VM images for QEMU/KVM, VMware, and Kubernetes (KubeVirt).
#
# Outputs in build/:
#   - cellos-x86_64.iso   (Dual BIOS+UEFI bootable ISO / CD-ROM)
#   - cellos-x86_64.qcow2 (QEMU/KVM disk image with VirtIO support)
#   - cellos-x86_64.vmdk  (VMware ESXi / Workstation virtual disk)
#   - deploy/kubevirt/    (KubeVirt containerdisk Dockerfile and VM manifest)
#
# Usage:
#   bash scripts/build-cloud-images.sh [--containerdisk [tag]]
#
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

BUILD_CONTAINER=0
CONTAINER_TAG="cellos-containerdisk:latest"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --containerdisk)
            BUILD_CONTAINER=1
            if [[ $# -gt 1 && ! "$2" =~ ^-- ]]; then
                CONTAINER_TAG="$2"
                shift
            fi
            shift
            ;;
        -h|--help)
            echo "Usage: bash scripts/build-cloud-images.sh [--containerdisk [tag]]"
            exit 0
            ;;
        *)
            echo "ERROR: unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

# ── 1. Check prerequisites ───────────────────────────────────────────────────
for tool in cargo xorriso qemu-img gcc; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "FAIL: required host tool '$tool' not found." >&2
        exit 1
    fi
done

LIMINE_CLI="limine/limine-8.7.0/bin/limine"
if [[ ! -x "$LIMINE_CLI" ]]; then
    echo "[build-cloud] Compiling Limine host tool..."
    gcc -O2 -o "$LIMINE_CLI" "limine/limine-8.7.0/bin/limine.c"
fi

mkdir -p build deploy/kubevirt

# ── 2. Build kernel ──────────────────────────────────────────────────────────
echo "[build-cloud] 1/4 Compiling Cellos x86_64 kernel (release)..."
cargo build --release -p cellos-kernel --target x86_64-unknown-none --features policy-required

# ── 3. Build hybrid ISO ──────────────────────────────────────────────────────
echo "[build-cloud] 2/4 Generating dual BIOS+UEFI image..."
bash scripts/x86/make-iso-ci.sh build/cellos-x86_64.iso

echo "[build-cloud] 3/4 Injecting Limine BIOS MBR/GPT stages for disk boot..."
"$LIMINE_CLI" bios-install build/cellos-x86_64.iso

# ── 4. Convert to QCOW2 and VMDK ─────────────────────────────────────────────
echo "[build-cloud] 4/4 Converting to QEMU QCOW2 and VMware VMDK..."
qemu-img convert -f raw -O qcow2 build/cellos-x86_64.iso build/cellos-x86_64.qcow2
qemu-img convert -f raw -O vmdk -o subformat=monolithicSparse build/cellos-x86_64.iso build/cellos-x86_64.vmdk

# ── 5. Optional KubeVirt Containerdisk ───────────────────────────────────────
if (( BUILD_CONTAINER )); then
    CONTAINER_TOOL=""
    if command -v docker >/dev/null 2>&1; then
        CONTAINER_TOOL="docker"
    elif command -v podman >/dev/null 2>&1; then
        CONTAINER_TOOL="podman"
    fi

    if [[ -n "$CONTAINER_TOOL" ]]; then
        echo "[build-cloud] Building KubeVirt containerdisk: $CONTAINER_TAG"
        "$CONTAINER_TOOL" build -t "$CONTAINER_TAG" -f deploy/kubevirt/Dockerfile .
        echo "[build-cloud] Container image built: $CONTAINER_TAG"
    else
        echo "WARN: neither docker nor podman found; skipped container build." >&2
    fi
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo
echo "=========================================================================="
echo "  Cellos Guest VM Images Ready in build/:"
echo "=========================================================================="
ls -lh build/cellos-x86_64.iso build/cellos-x86_64.qcow2 build/cellos-x86_64.vmdk
echo
echo "Target 1 — QEMU / KVM:"
echo "  qemu-system-x86_64 -enable-kvm -machine q35 -m 512M \\"
echo "    -drive file=build/cellos-x86_64.qcow2,format=qcow2,if=virtio \\"
echo "    -netdev user,id=net0 -device virtio-net-pci,netdev=net0 -serial stdio"
echo
echo "Target 2 — VMware Workstation / ESXi:"
echo "  Attach build/cellos-x86_64.vmdk as SATA/AHCI disk, or build/cellos-x86_64.iso as CD-ROM."
echo "  Configure: GuestOS = other-64, Firmware = BIOS, NIC = e1000, Serial0 = Pipe/File."
echo
echo "Target 3 — Kubernetes (KubeVirt):"
echo "  1. Build containerdisk:  bash scripts/build-cloud-images.sh --containerdisk <tag>"
echo "  2. Deploy VM on K8s:     kubectl apply -f deploy/kubevirt/cellos-vm.yaml"
echo "=========================================================================="
