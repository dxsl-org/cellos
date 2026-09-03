#!/usr/bin/env bash
# Helper functions for scripts/make-hypervisor-fs-x86.sh
# Sourced by scripts/make-hypervisor-fs-x86.sh.

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: bash scripts/make-hypervisor-fs-x86.sh [--skip-fetch]

Environment:
  HV_GUEST_PROFILE       alpine (default) or ubuntu
  UBUNTU_ARTIFACT_DIR    Ubuntu profile artifacts (default build/ubuntu-wide-guest-x86)
  HV_VOLATILE_DISK       0 (default) or 1; Ubuntu requires 0
  HV_INIT_MIN            0 (default) or 1
  HV_HOSTILE_BACKEND_RECOVERY  0 (default) or 1

For Ubuntu, build the root-owned artifacts first with
  sudo bash scripts/build-ubuntu-wide-guest-x86.sh
then use HV_GUEST_PROFILE=ubuntu with --skip-fetch for the Cargo/image build.
EOF
}

parse_and_validate_args() {
    SKIP_FETCH=0
    case "${1:-}" in
        "") ;;
        --skip-fetch) SKIP_FETCH=1 ;;
        --help|-h) usage; exit 0 ;;
        *) echo "ERROR: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
    [[ "$#" -le 1 ]] || { echo "ERROR: too many arguments" >&2; usage >&2; exit 2; }

    TARGET="x86_64-unknown-none"
    BIN_DIR="target/$TARGET/release"
    ALPINE_CACHE=".alpine-cache-x86"
    GUEST_PROFILE="${HV_GUEST_PROFILE:-alpine}"
    UBUNTU_ARTIFACT_DIR="${UBUNTU_ARTIFACT_DIR:-build/ubuntu-wide-guest-x86}"
    VMLINUX_SOURCE="$ALPINE_CACHE/vmlinux"
    INITRD_SOURCE="${INITRD_OVERRIDE:-$ALPINE_CACHE/initramfs-virt}"
    EMBEDDED_HV="kernel/src/embedded-hv-x86"

    HV_INIT_MIN_VALUE="${HV_INIT_MIN:-0}"
    HV_HOSTILE_BACKEND_RECOVERY_VALUE="${HV_HOSTILE_BACKEND_RECOVERY:-0}"
    HV_VOLATILE_DISK_VALUE="${HV_VOLATILE_DISK:-0}"

    case "$GUEST_PROFILE" in
        alpine) ;;
        ubuntu)
            if [[ -n "${INITRD_OVERRIDE:-}" ]]; then
                echo "ERROR: INITRD_OVERRIDE is not permitted for the pinned Ubuntu profile" >&2
                exit 1
            fi
            if [[ "$HV_VOLATILE_DISK_VALUE" == "1" ]]; then
                echo "ERROR: the Ubuntu profile requires persistent /mnt/sd/guest_disk.img" >&2
                exit 1
            fi
            if [[ "$HV_HOSTILE_BACKEND_RECOVERY_VALUE" == "1" ]]; then
                echo "ERROR: hostile backend recovery uses the Alpine evidence initramfs" >&2
                exit 1
            fi
            VMLINUX_SOURCE="$UBUNTU_ARTIFACT_DIR/vmlinux"
            INITRD_SOURCE="$UBUNTU_ARTIFACT_DIR/initrd.gz"
            ;;
        *)
            echo "ERROR: HV_GUEST_PROFILE must be 'alpine' or 'ubuntu' (got '$GUEST_PROFILE')" >&2
            exit 1
            ;;
    esac

    for val_spec in "HV_INIT_MIN:$HV_INIT_MIN_VALUE" \
                     "HV_HOSTILE_BACKEND_RECOVERY:$HV_HOSTILE_BACKEND_RECOVERY_VALUE" \
                     "HV_VOLATILE_DISK:$HV_VOLATILE_DISK_VALUE"; do
        var="${val_spec%%:*}"
        val="${val_spec#*:}"
        case "$val" in
            0|1) ;;
            *) echo "ERROR: $var must be 0 or 1" >&2; exit 1 ;;
        esac
    done

    if [[ "$HV_HOSTILE_BACKEND_RECOVERY_VALUE" == "1" && "$HV_VOLATILE_DISK_VALUE" == "1" ]]; then
        echo "ERROR: hostile backend recovery requires persistent disk mode" >&2
        exit 1
    fi
}

prepare_guest_artifacts() {
    if [[ "$SKIP_FETCH" == "0" ]]; then
        if [[ "$GUEST_PROFILE" == "ubuntu" ]]; then
            bash scripts/build-ubuntu-wide-guest-x86.sh --output-dir "$UBUNTU_ARTIFACT_DIR"
        else
            bash scripts/fetch-alpine-x86.sh "$ALPINE_CACHE"
        fi
    fi
    if [[ ! -f "$VMLINUX_SOURCE" || ! -f "$INITRD_SOURCE" ]]; then
        echo "ERROR: $GUEST_PROFILE guest artifacts missing: $VMLINUX_SOURCE or $INITRD_SOURCE" >&2
        exit 1
    fi
    if [[ "$GUEST_PROFILE" == "ubuntu" ]]; then
        for required in guest_disk.img provenance.txt artifact-sha256sums; do
            [[ -f "$UBUNTU_ARTIFACT_DIR/$required" ]] \
                || { echo "ERROR: Ubuntu artifact missing: $UBUNTU_ARTIFACT_DIR/$required" >&2; exit 1; }
        done
        (
            cd "$UBUNTU_ARTIFACT_DIR"
            sha256sum --check --strict artifact-sha256sums
        ) || { echo "ERROR: Ubuntu artifact digest mismatch" >&2; exit 1; }
    fi
}
