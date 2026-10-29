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
#   bash scripts/build-cloud-images.sh [--ocel-profile embedded|desktop] [--javascript] [--qemu-dev-https] [--containerdisk [tag]]
#
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# Cloud images stay out of the promoted release (Phase 10 D10-9 A, S10-2b): reopening the default
# kernel path for the qualified kernel must not let this script build qcow2/vmdk/KubeVirt images, and
# step 1 rewrites the canonical ramdisk in place. Distribution formats need their own decision.
echo "ROLLOUT BLOCKED: cloud images are outside the Phase 10 release (D10-9 A); see .agents/261020-1750-cellbox-cli/reports/rollout-containment.md" >&2
exit 1

BUILD_CONTAINER=0
CONTAINER_TAG="cellos-containerdisk:latest"
KERNEL_FEATURES="policy-required"
QEMU_DEV_HTTPS=0
OCEL_PROFILE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --javascript)
            # Explicit opt-in; the default fleet image keeps Tier 2 closed.
            KERNEL_FEATURES="browser-engine-profile"
            shift
            ;;
        --qemu-dev-https)
            # Requires an embedded NET built with the matching opt-in feature.
            QEMU_DEV_HTTPS=1
            shift
            ;;
        --ocel-profile)
            if [[ $# -lt 2 || ( "$2" != "embedded" && "$2" != "desktop" ) ]]; then
                echo "ERROR: --ocel-profile requires embedded or desktop" >&2
                exit 1
            fi
            OCEL_PROFILE="$2"
            shift 2
            ;;
        --containerdisk)
            BUILD_CONTAINER=1
            if [[ $# -gt 1 && ! "$2" =~ ^-- ]]; then
                CONTAINER_TAG="$2"
                shift
            fi
            shift
            ;;
        -h|--help)
            echo "Usage: bash scripts/build-cloud-images.sh [--ocel-profile embedded|desktop] [--javascript] [--qemu-dev-https] [--containerdisk [tag]]"
            exit 0
            ;;
        *)
            echo "ERROR: unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if (( QEMU_DEV_HTTPS )); then
    KERNEL_FEATURES="${KERNEL_FEATURES},qemu-dev-https"
    echo "[build-cloud] QEMU DEV HTTPS: prepared NET must use qemu-dev-https; host time companion required."
fi

if [[ "$OCEL_PROFILE" == "desktop" ]]; then
    KERNEL_FEATURES="${KERNEL_FEATURES},large-cell-va"
fi

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

if [[ -n "$OCEL_PROFILE" ]]; then
    OCEL_FEATURE_ARGS=()
    if [[ "$OCEL_PROFILE" == "desktop" ]]; then
        OCEL_FEATURE_ARGS=(--features desktop)
    fi
    echo "[build-cloud] Building and packaging Ocel profile: $OCEL_PROFILE"
    RUSTFLAGS="-C relocation-model=pic" cargo build --release -p ocel \
        --no-default-features "${OCEL_FEATURE_ARGS[@]}" \
        --target x86_64-unknown-none -Z build-std=core,alloc
    python3 scripts/cellos-sign --sign target/x86_64-unknown-none/release/ocel
    python3 scripts/package-ocel-profile.py kernel/src/embedded-x86_64/kernel_fs.img \
        target/x86_64-unknown-none/release/ocel
fi

# ── 2. Build kernel ──────────────────────────────────────────────────────────
echo "[build-cloud] 1/4 Compiling Cellos x86_64 kernel (release)..."
cargo build --release -p cellos-kernel --target x86_64-unknown-none --features "$KERNEL_FEATURES"

# ── 3. Build hybrid ISO ──────────────────────────────────────────────────────
echo "[build-cloud] 2/4 Generating dual BIOS+UEFI image..."
bash scripts/x86/make-iso-ci.sh build/cellos-x86_64.iso

echo "[build-cloud] 3/4 Injecting Limine BIOS MBR/GPT stages for disk boot..."
"$LIMINE_CLI" bios-install build/cellos-x86_64.iso

# ── 4. Convert to QCOW2 and VMDK ─────────────────────────────────────────────
echo "[build-cloud] 4/4 Converting to QEMU QCOW2 and VMware VMDK..."
qemu-img convert -f raw -O qcow2 build/cellos-x86_64.iso build/cellos-x86_64.qcow2
qemu-img convert -f raw -O vmdk -o subformat=monolithicSparse build/cellos-x86_64.iso build/cellos-x86_64.vmdk
if [[ -n "$OCEL_PROFILE" ]]; then
    cp build/cellos-x86_64.qcow2 "build/cellos-x86_64-${OCEL_PROFILE}.qcow2"
fi

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
if [[ -n "$OCEL_PROFILE" ]]; then
    echo "  Packaged Ocel memory profile: $OCEL_PROFILE"
    echo "  Profile-specific disk: build/cellos-x86_64-${OCEL_PROFILE}.qcow2"
fi
echo
echo "Target 1 — QEMU / KVM:"
if (( QEMU_DEV_HTTPS )); then
    echo "  Start trusted-host time first: python3 scripts/qemu-dev-time.py"
    echo "  QEMU DEV only: retain host state outside guest snapshots; whole-host rollback excluded."
fi
echo "  qemu-system-x86_64 -enable-kvm -cpu host -machine q35,vmport=on -m 1024M \\"
echo "    -drive file=build/cellos-x86_64.qcow2,format=qcow2,if=virtio \\"
echo "    -netdev user,id=net0 -device e1000,netdev=net0 -serial stdio"
echo
echo "Target 2 — VMware Workstation / ESXi:"
echo "  Attach build/cellos-x86_64.vmdk as SATA/AHCI disk, or build/cellos-x86_64.iso as CD-ROM."
echo "  Configure: GuestOS = other-64, Firmware = BIOS, NIC = e1000, Serial0 = Pipe/File."
echo
echo "Target 3 — Kubernetes (KubeVirt):"
echo "  1. Build containerdisk:  bash scripts/build-cloud-images.sh --containerdisk <tag>"
echo "  2. Deploy VM on K8s:     kubectl apply -f deploy/kubevirt/cellos-vm.yaml"
echo "=========================================================================="
