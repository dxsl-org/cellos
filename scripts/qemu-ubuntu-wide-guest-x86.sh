#!/usr/bin/env bash
# Two-boot Ubuntu 24.04 Tier 3 persistence runner. Success is based only on
# exact machine markers emitted after explicit systemd/apt/file assertions and
# a final host-side ext4 read, never on a shell prompt or generic boot text.

set -euo pipefail

readonly REQUIRED_QEMU_VERSION="QEMU emulator version 10.2.0"
readonly EXPECTED_PROFILE="ubuntu-wide-guest-v1"
readonly EXPECTED_ROOTFS_SHA256="16429c49387eaf783a88ce1896940dfdb10b51cbec38304b2b652e26993276b7"
readonly EXPECTED_SNAPSHOT="20240821T000000Z"
readonly MARKER_PACKAGE="sl"
readonly MARKER_VERSION="5.02-1"
readonly MARKER_PATH="/var/lib/cellos/ubuntu-apt-marker-v1"
readonly MARKER_CONTENT="sl=5.02-1"
readonly READY_MARKER="CELLOS_UBUNTU_MULTI_USER_READY_V1"
readonly COMMIT_MARKER="CELLOS_UBUNTU_APT_COMMIT_V1"
readonly SECOND_BOOT_MARKER="CELLOS_UBUNTU_SECOND_BOOT_MARKER_OK_V1"
readonly GUEST_PROMPT="CELLOS_UBUNTU_ROOT# "


# shellcheck source=scripts/lib-qemu-ubuntu-runner.sh
source "$(dirname "$0")/lib-qemu-ubuntu-runner.sh"
usage() {
    cat <<'EOF'
Usage: bash scripts/qemu-ubuntu-wide-guest-x86.sh [options]

Options:
  --iso FILE          Ubuntu-profile Cellos ISO (default: build/vicell-x86-ubuntu.iso)
  --artifacts DIR     Pinned guest artifacts (default: build/ubuntu-wide-guest-x86)
  --work-dir DIR      Runner state/logs (default: build/x86-ubuntu-wide-run)
  --boot-window SEC   Per-boot deadline (default: 1200)
  -h, --help          Show this help

Environment:
  QEMU_X86_BIN        Qualified emulator (default: qemu-system-x86_64)
  QEMU_MEMORY         Outer Cellos RAM, integer M/G and >= 1G (default: 2G)

The ISO must be built with:
  HV_GUEST_PROFILE=ubuntu \
    bash scripts/make-hypervisor-fs-x86.sh --skip-fetch
EOF
}

ISO="build/vicell-x86-ubuntu.iso"
ARTIFACT_DIR="build/ubuntu-wide-guest-x86"
WORK_DIR="build/x86-ubuntu-wide-run"
BOOT_WINDOW=1200
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --iso|--artifacts|--work-dir|--boot-window)
            [[ "$#" -ge 2 && -n "$2" ]] || { echo "ERROR: $1 requires a value" >&2; exit 2; }
            case "$1" in
                --iso) ISO="$2" ;;
                --artifacts) ARTIFACT_DIR="$2" ;;
                --work-dir) WORK_DIR="$2" ;;
                --boot-window) BOOT_WINDOW="$2" ;;
            esac
            shift 2
            ;;
        --help|-h) usage; exit 0 ;;
        *) echo "ERROR: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done
[[ "$BOOT_WINDOW" =~ ^[1-9][0-9]*$ ]] \
    || { echo "ERROR: --boot-window must be a positive integer" >&2; exit 2; }

QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
QEMU_MEMORY="${QEMU_MEMORY:-2G}"
case "$QEMU_MEMORY" in
    *G) [[ "${QEMU_MEMORY%G}" =~ ^[1-9][0-9]*$ ]] || { echo "ERROR: invalid QEMU_MEMORY" >&2; exit 2; }
        MEMORY_MIB=$(( ${QEMU_MEMORY%G} * 1024 )) ;;
    *M) [[ "${QEMU_MEMORY%M}" =~ ^[1-9][0-9]*$ ]] || { echo "ERROR: invalid QEMU_MEMORY" >&2; exit 2; }
        MEMORY_MIB="${QEMU_MEMORY%M}" ;;
    *) echo "ERROR: QEMU_MEMORY must be an integer followed by M or G" >&2; exit 2 ;;
esac
(( MEMORY_MIB >= 1024 )) \
    || { echo "ERROR: Ubuntu wide-guest requires outer QEMU memory >= 1 GiB" >&2; exit 1; }

for tool in "$QEMU_X86_BIN" sha256sum truncate sfdisk mkfs.fat mcopy sync \
    debugfs grep sed tail wc realpath mkfifo cmp; do
    if [[ "$tool" == "$QEMU_X86_BIN" ]]; then
        command -v "$tool" >/dev/null 2>&1 || [[ -x "$tool" ]] \
            || { echo "BLOCKED_ENVIRONMENT: QEMU executable not found: $tool" >&2; exit 1; }
    else
        command -v "$tool" >/dev/null 2>&1 \
            || { echo "BLOCKED_ENVIRONMENT: required tool not found: $tool" >&2; exit 1; }
    fi
done
qemu_version="$("$QEMU_X86_BIN" --version | sed -n '1p')"
[[ "$qemu_version" == "$REQUIRED_QEMU_VERSION" ]] \
    || { echo "BLOCKED_ENVIRONMENT: require QEMU-TCG 10.2.0, found: $qemu_version" >&2; exit 1; }

[[ -f "$ISO" ]] || { echo "BLOCKED_ENVIRONMENT: ISO not found: $ISO" >&2; exit 1; }
for required in vmlinux initrd.gz guest_disk.img provenance.txt artifact-sha256sums; do
    [[ -f "$ARTIFACT_DIR/$required" ]] \
        || { echo "BLOCKED_ENVIRONMENT: Ubuntu artifact missing: $ARTIFACT_DIR/$required" >&2; exit 1; }
done

require_provenance() {
    local expected="$1"
    [[ "$(grep -Fxc -- "$expected" "$ARTIFACT_DIR/provenance.txt" || true)" == 1 ]] \
        || { echo "FAIL: Ubuntu image provenance mismatch: expected '$expected'" >&2; exit 1; }
}
require_provenance "profile=$EXPECTED_PROFILE"
require_provenance "ubuntu_release=24.04"
require_provenance "ubuntu_rootfs_sha256=$EXPECTED_ROOTFS_SHA256"
require_provenance "ubuntu_snapshot=$EXPECTED_SNAPSHOT"
require_provenance "marker_package=$MARKER_PACKAGE"
require_provenance "marker_version=$MARKER_VERSION"
require_provenance "marker_path=$MARKER_PATH"
[[ "$(wc -l < "$ARTIFACT_DIR/artifact-sha256sums")" == 3 ]] \
    || { echo "FAIL: artifact-sha256sums must contain exactly three records" >&2; exit 1; }
for artifact in vmlinux initrd.gz guest_disk.img; do
    grep -Eq "^[0-9a-f]{64}  ${artifact}$" "$ARTIFACT_DIR/artifact-sha256sums" \
        || { echo "FAIL: missing strict digest record for $artifact" >&2; exit 1; }
done
(
    cd "$ARTIFACT_DIR"
    sha256sum --check --strict artifact-sha256sums
) || { echo "FAIL: Ubuntu artifact digest mismatch" >&2; exit 1; }

mkdir -p "$WORK_DIR"
OUTER_DISK="$WORK_DIR/outer-nvme.img"
INPUT_FIFO="$WORK_DIR/serial.in"
FIRST_RAW="$WORK_DIR/first-boot.raw.log"
SECOND_RAW="$WORK_DIR/second-boot.raw.log"
PART_START=2048
QEMU_PID=""
trap cleanup_qemu EXIT INT TERM
prepare_runner "$ISO" "$OUTER_DISK" "$PART_START" "$ARTIFACT_DIR/guest_disk.img"

start_boot "first boot" "$FIRST_RAW"
wait_exact_line "first boot" "$READY_MARKER" "$FIRST_RAW"
assert_boot_contract "first boot" "$FIRST_RAW"
wait_prompt "first boot" "$FIRST_RAW"
FIRST_COMMAND="if systemctl is-active --quiet multi-user.target && apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ${MARKER_PACKAGE}=${MARKER_VERSION} && [ \"\$(dpkg-query -W -f='\${Version}' ${MARKER_PACKAGE} 2>/dev/null)\" = '${MARKER_VERSION}' ] && install -d -m 0755 \"\$(dirname '${MARKER_PATH}')\" && printf '${MARKER_CONTENT}\\n' > '${MARKER_PATH}' && sync; then printf '${COMMIT_MARKER}\\n' > /dev/ttyS0; systemctl poweroff; else rc=\$?; printf 'CELLOS_UBUNTU_APT_FAIL_V1 rc=%s\\n' \"\$rc\" > /dev/ttyS0; fi"
printf '%s\n' "$FIRST_COMMAND" >&3
wait_exact_line "first boot" "$COMMIT_MARKER" "$FIRST_RAW"
finish_boot "$FIRST_RAW"

start_boot "second boot" "$SECOND_RAW"
wait_exact_line "second boot" "$READY_MARKER" "$SECOND_RAW"
assert_boot_contract "second boot" "$SECOND_RAW"
wait_prompt "second boot" "$SECOND_RAW"
SECOND_COMMAND="if systemctl is-active --quiet multi-user.target && [ \"\$(dpkg-query -W -f='\${Version}' ${MARKER_PACKAGE} 2>/dev/null)\" = '${MARKER_VERSION}' ] && [ \"\$(cat '${MARKER_PATH}' 2>/dev/null)\" = '${MARKER_CONTENT}' ]; then printf '${SECOND_BOOT_MARKER}\\n' > /dev/ttyS0; sync; systemctl poweroff; else rc=\$?; printf 'CELLOS_UBUNTU_SECOND_BOOT_FAIL_V1 rc=%s\\n' \"\$rc\" > /dev/ttyS0; fi"
printf '%s\n' "$SECOND_COMMAND" >&3
wait_exact_line "second boot" "$SECOND_BOOT_MARKER" "$SECOND_RAW"
finish_boot "$SECOND_RAW"

RECOVERED="$WORK_DIR/recovered-guest-disk.img"
rm -f "$RECOVERED"
mcopy -i "$OUTER_DISK@@$((PART_START * 512))" ::/guest_disk.img "$RECOVERED"
printf '%s\n' "$MARKER_CONTENT" > "$WORK_DIR/expected-marker"
debugfs -R "cat $MARKER_PATH" "$RECOVERED" 2>/dev/null > "$WORK_DIR/recovered-marker"
cmp -s "$WORK_DIR/expected-marker" "$WORK_DIR/recovered-marker" \
    || { echo "FAIL: host-side ext4 read did not recover the persisted marker" >&2; exit 1; }

echo "PASS: Ubuntu 24.04 reached multi-user.target, apt installed ${MARKER_PACKAGE}=${MARKER_VERSION}, and the exact marker survived a clean second boot."
