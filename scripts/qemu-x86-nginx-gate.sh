#!/usr/bin/env bash
# Tier 3 gate: nginx runs inside the Alpine guest on the x86_64 VMM.
#
# Builds the evidence image (guest initramfs with the nginx fixture + the
# `/virtio-e2e` selector that selects the E2E guest cmdline), boots it under the
# qualified QEMU-TCG 10.2.0, and asserts the guest's own markers: apk installs
# nginx from the pinned Alpine repository, nginx starts as master + worker
# (fork), and an HTTP fetch from inside the guest returns the expected body.
# This is the gate `.agents/TODO.md` names as "nginx chạy thật trong Linux VM".
#
# The guest carve is 128 MiB, so the prepared initramfs is pruned to the VirtIO
# modules it needs (scripts/prepare-x86-nginx-initramfs.sh).
#
# Evidence scope: QEMU-TCG only. Passing proves the guest workload, not physical
# x86 qualification and not nested-virtualization fidelity.
#
# Usage: bash scripts/qemu-x86-nginx-gate.sh [iso]
#   iso  default: build/vicell-x86-nginx.iso
#
# Environment:
#   QEMU_X86_BIN           emulator executable (default: qemu-system-x86_64)
#   QEMU_MEMORY            outer Cellos RAM (default 2G)
#   BOOT_WINDOW            seconds to wait (default 1200)
#   BUILD_EVIDENCE_IMAGE   1 (default) rebuilds initramfs/fs/kernel/ISO, 0 reuses
#   NGINX_GATE_INITRAMFS   prepared initramfs path
#   NGINX_GATE_WORK_DIR    scratch + logs (default build/x86-nginx-gate)
#
# The x86 cell build needs the littlefs/bindgen environment the CI job sets
# (CC_x86_64_unknown_none, CFLAGS_x86_64_unknown_none,
# BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none).
#
# Exit codes:
#   0 — gate PASSED; 1 — guest/build failure; 1 with BLOCKED_ENVIRONMENT — prerequisites

set -euo pipefail

ISO="${1:-build/vicell-x86-nginx.iso}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
QEMU_MEMORY="${QEMU_MEMORY:-2G}"
BOOT_WINDOW="${BOOT_WINDOW:-1200}"
BUILD_EVIDENCE_IMAGE="${BUILD_EVIDENCE_IMAGE:-1}"
INITRAMFS="${NGINX_GATE_INITRAMFS:-build/x86-nginx-initramfs.cpio.gz}"
WORK_DIR="${NGINX_GATE_WORK_DIR:-build/x86-nginx-gate}"
STAGE="$WORK_DIR/embedded-hv-x86"
ACTIVE_QEMU_PID=""

cleanup() {
    if [[ -n "$ACTIVE_QEMU_PID" ]] && kill -0 "$ACTIVE_QEMU_PID" 2>/dev/null; then
        kill -KILL "$ACTIVE_QEMU_PID" 2>/dev/null || true
        wait "$ACTIVE_QEMU_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

for tool in mcopy realpath; do
    command -v "$tool" >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: required tool not found: $tool" >&2; exit 1; }
done
if ! command -v "$QEMU_X86_BIN" >/dev/null 2>&1 && [[ ! -x "$QEMU_X86_BIN" ]]; then
    echo "BLOCKED_ENVIRONMENT: QEMU executable not found: $QEMU_X86_BIN" >&2
    exit 1
fi

qemu_version="$("$QEMU_X86_BIN" --version 2>&1 | sed -n '1p')"
if [[ "$qemu_version" != "QEMU emulator version 10.2.0" ]]; then
    echo "BLOCKED_ENVIRONMENT: requires exact 'QEMU emulator version 10.2.0' (got '$qemu_version')" >&2
    exit 1
fi

mkdir -p "$WORK_DIR"
if [[ "$BUILD_EVIDENCE_IMAGE" == 1 ]]; then
    NGINX_GATE_INITRAMFS="$INITRAMFS" bash scripts/prepare-x86-nginx-initramfs.sh
    rm -rf "$STAGE"
    mkdir -p "$STAGE"
    HV_VOLATILE_DISK=1 HV_INIT_MIN=1 INITRD_OVERRIDE="$INITRAMFS" \
        HV_EMBEDDED_DIR="$STAGE" \
        bash scripts/make-hypervisor-fs-x86.sh --skip-fetch
    # The build wrote kernel_fs.img/init straight into $STAGE (HV_EMBEDDED_DIR),
    # so the tracked kernel/src/embedded-hv-x86 is never rewritten by this lane.
    printf 'rdinit=/bin/virtio-e2e-init\n' > "$WORK_DIR/selector"
    mcopy -o -i "$STAGE/kernel_fs.img" "$WORK_DIR/selector" ::/virtio-e2e
    RUSTFLAGS="-C relocation-model=static -C code-model=kernel -C no-redzone=yes -Z cf-protection=full" \
        EMBEDDED_OVERRIDE="$STAGE" \
        cargo build --release -p cellos-kernel --target x86_64-unknown-none
    # X86_ISO_ROOT keeps the lane out of the tracked build/x86-iso-root staging
    # directory (same reason as HV_EMBEDDED_DIR above).
    X86_ISO_ROOT="$WORK_DIR/iso-root" bash scripts/x86/make-iso-ci.sh "$ISO"
elif [[ "$BUILD_EVIDENCE_IMAGE" != 0 ]]; then
    echo "FAIL: BUILD_EVIDENCE_IMAGE must be 0 or 1" >&2
    exit 1
fi
[[ -f "$ISO" ]] || { echo "BLOCKED_ENVIRONMENT: evidence ISO not found: $ISO" >&2; exit 1; }

QEMU_ISO="$(realpath "$ISO")"
if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
    command -v wslpath >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: Windows QEMU requires wslpath" >&2; exit 1; }
    QEMU_ISO="$(wslpath -w "$QEMU_ISO")"
fi

raw_log="$WORK_DIR/qemu-nginx-gate.raw.log"
log="$WORK_DIR/qemu-nginx-gate.log"
fatal_pattern='KERNEL PANIC|\[fault\] Cell|\[hv-x86\].*(fail|error|unexpected|unsupported|unknown vmexit|unhandled|guest (exited|shutdown)|triple-fault)|NGINX_IN_VM_FAIL|Out of memory: Killed process|Init: hypervisor exited|corrupt(ion|ed)?'
success_marker='NGINX_IN_VM_ALL_PASS'

echo "[nginx-gate] $qemu_version iso=$ISO memory=$QEMU_MEMORY window=${BOOT_WINDOW}s"
"$QEMU_X86_BIN" \
    -machine q35 \
    -device intel-iommu,intremap=on \
    -accel tcg \
    -cpu qemu64,+pdpe1gb,+svm \
    -m "$QEMU_MEMORY" \
    -nographic \
    -cdrom "$QEMU_ISO" \
    -boot d \
    -no-reboot \
    -netdev user,id=net0,net=10.0.2.0/24 \
    -device e1000,netdev=net0,mac=52:54:00:12:34:56 \
    < /dev/null > "$raw_log" 2>&1 &
ACTIVE_QEMU_PID=$!

deadline=$((SECONDS + BOOT_WINDOW))
while kill -0 "$ACTIVE_QEMU_PID" 2>/dev/null && (( SECONDS < deadline )); do
    if grep -qF "$success_marker" "$raw_log" 2>/dev/null; then
        break
    fi
    if grep -Eqi "$fatal_pattern" "$raw_log" 2>/dev/null; then
        sleep 5  # let the guest print the surrounding context
        break
    fi
    sleep 2
done
cleanup
ACTIVE_QEMU_PID=""

tr -d '\000\r' < "$raw_log" | sed -e 's/\x1b\[[0-9;]*m//g' > "$log"

dump_log() {
    echo "--- $log, last 120 of $(wc -l < "$log") lines ---" >&2
    tail -n 120 "$log" >&2
}

if grep -Eqi "$fatal_pattern" "$log"; then
    echo "FAIL: fatal condition in the gate run" >&2
    grep -aiE "$fatal_pattern" "$log" | head -10 >&2 || true
    dump_log
    exit 1
fi

required_markers=(
    '[hv-x86] vCPU ready'
    'NGINX_IN_VM_GUEST_INIT_START'
    'NGINX_IN_VM_CRNG_READY'
    'nginx version: nginx/'
    'nginx: configuration file /etc/nginx/nginx.conf test is successful'
    'NGINX_IN_VM_FORK_MASTER_WORKER_PASS'
    'NGINX_IN_VM_HTTP_SERVE_PASS'
    'served body: cellos-tier3-nginx-ok'
    "$success_marker"
)
for marker in "${required_markers[@]}"; do
    grep -qF "$marker" "$log" \
        || { echo "FAIL: missing marker: $marker" >&2; dump_log; exit 1; }
done
if grep -qF 'NGINX_IN_VM_APK_REPO_INSTALL_PASS' "$log"; then
    install_path=repo-https
elif grep -qF 'NGINX_IN_VM_APK_REPO_HTTP_INSTALL_PASS' "$log"; then
    install_path=repo-http
else
    echo "FAIL: nginx was not installed from the Alpine repository" >&2
    dump_log
    exit 1
fi
if ! grep -qF 'NGINX_IN_VM_GATEWAY_PING_PASS' "$log" \
    || ! grep -qF 'NGINX_IN_VM_DNS_PASS' "$log"; then
    echo "FAIL: guest network prerequisites did not pass" >&2
    dump_log
    exit 1
fi

echo "PASS: nginx $install_path install, fork, and in-guest HTTP serve — log: $log"
echo "Scope: QEMU-TCG 10.2.0 emulator evidence only; not physical x86 qualification."
