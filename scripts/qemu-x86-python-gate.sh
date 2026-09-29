#!/usr/bin/env bash
# Prove a real CPython data-processing workload inside the Alpine Tier 3 guest.
# This is QEMU-TCG evidence, not physical x86 qualification.
set -euo pipefail

ISO="${1:-build/vicell-x86-python.iso}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
QEMU_MEMORY="${QEMU_MEMORY:-2G}"
BOOT_WINDOW="${BOOT_WINDOW:-1200}"
BUILD_EVIDENCE_IMAGE="${BUILD_EVIDENCE_IMAGE:-1}"
WORK_DIR="${PYTHON_GATE_WORK_DIR:-build/x86-python-gate}"
INITRAMFS="$WORK_DIR/python-initramfs.cpio.gz"
STAGE="$WORK_DIR/embedded-hv-x86"
QEMU_PID=""

cleanup() {
    if [[ -n "$QEMU_PID" ]]; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
        QEMU_PID=""
    fi
}
trap cleanup EXIT INT TERM

for tool in mcopy realpath sha256sum; do
    command -v "$tool" >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: required tool not found: $tool" >&2; exit 1; }
done
if ! command -v "$QEMU_X86_BIN" >/dev/null 2>&1 && [[ ! -x "$QEMU_X86_BIN" ]]; then
    echo "BLOCKED_ENVIRONMENT: QEMU executable not found: $QEMU_X86_BIN" >&2
    exit 1
fi
[[ "$("$QEMU_X86_BIN" --version | sed -n '1p')" == 'QEMU emulator version 10.2.0' ]] \
    || { echo 'BLOCKED_ENVIRONMENT: require QEMU-TCG 10.2.0' >&2; exit 1; }
[[ "$BOOT_WINDOW" =~ ^[1-9][0-9]*$ ]] \
    || { echo 'FAIL: BOOT_WINDOW must be positive seconds' >&2; exit 1; }
mkdir -p "$WORK_DIR"
if [[ "$BUILD_EVIDENCE_IMAGE" == 1 ]]; then
    APP_GATE_INITRAMFS="$INITRAMFS" \
        APP_GATE_FIXTURE=tests/guests/x86-python/guest-init.sh \
        bash scripts/prepare-x86-app-initramfs.sh
    rm -rf "$STAGE"
    mkdir -p "$STAGE"
    HV_GUEST_PROFILE=alpine-wide HV_VOLATILE_DISK=1 HV_INIT_MIN=1 INITRD_OVERRIDE="$INITRAMFS" \
        HV_EMBEDDED_DIR="$STAGE" \
        bash scripts/make-hypervisor-fs-x86.sh --skip-fetch
    printf 'rdinit=/bin/virtio-e2e-init\n' > "$WORK_DIR/selector"
    mcopy -o -i "$STAGE/kernel_fs.img" "$WORK_DIR/selector" ::/virtio-e2e
    RUSTFLAGS='-C relocation-model=static -C code-model=kernel -C no-redzone=yes -Z cf-protection=full' \
        EMBEDDED_OVERRIDE="$STAGE" \
        cargo build --release -p cellos-kernel --target x86_64-unknown-none
    X86_ISO_ROOT="$WORK_DIR/iso-root" bash scripts/x86/make-iso-ci.sh "$ISO"
elif [[ "$BUILD_EVIDENCE_IMAGE" != 0 ]]; then
    echo 'FAIL: BUILD_EVIDENCE_IMAGE must be 0 or 1' >&2
    exit 1
fi
[[ -f "$ISO" ]] || { echo "BLOCKED_ENVIRONMENT: ISO not found: $ISO" >&2; exit 1; }
QEMU_ISO="$(realpath "$ISO")"
if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
    command -v wslpath >/dev/null 2>&1 \
        || { echo 'BLOCKED_ENVIRONMENT: Windows QEMU requires wslpath' >&2; exit 1; }
    QEMU_ISO="$(wslpath -w "$QEMU_ISO")"
fi

NET_TRACE_ARGS=()
if [[ -n "${QEMU_NET_CAPTURE:-}" ]]; then
    net_capture="$(realpath -m "$QEMU_NET_CAPTURE")"
    if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
        net_capture="$(wslpath -w "$net_capture")"
    fi
    NET_TRACE_ARGS=(-object "filter-dump,id=python-gate-net,netdev=net0,file=$net_capture")
fi

RAW="$WORK_DIR/qemu-python-gate.raw.log"
LOG="$WORK_DIR/qemu-python-gate.log"
SUCCESS=PYTHON_IN_VM_WORKLOAD_PASS
FATAL='KERNEL PANIC|\[fault\] Cell|\[hv-x86\].*(fail|error|unexpected|unsupported|unknown vmexit|unhandled|guest (exited|shutdown)|triple-fault)|PYTHON_IN_VM_FAIL:|Out of memory: Killed process|Init: hypervisor exited|corrupt(ion|ed)?'
"$QEMU_X86_BIN" -machine q35 -device intel-iommu,intremap=on \
    -accel tcg -cpu qemu64,+pdpe1gb,+svm -m "$QEMU_MEMORY" -nographic \
    -cdrom "$QEMU_ISO" -boot d -no-reboot \
    -netdev user,id=net0,net=10.0.2.0/24 \
    -device e1000,netdev=net0,mac=52:54:00:12:34:56 \
    "${NET_TRACE_ARGS[@]}" \
    < /dev/null > "$RAW" 2>&1 &
QEMU_PID=$!
deadline=$((SECONDS + BOOT_WINDOW))
while (( SECONDS < deadline )); do
    grep -qF "$SUCCESS" "$RAW" 2>/dev/null && break
    grep -Eqi "$FATAL" "$RAW" 2>/dev/null && break
    kill -0 "$QEMU_PID" 2>/dev/null || break
    sleep 2
done
cleanup
tr -d '\000\r' < "$RAW" | sed -e 's/\x1b\[[0-9;]*m//g' > "$LOG"
if grep -Eqi "$FATAL" "$LOG"; then
    echo 'FAIL: fatal condition in the Python gate' >&2
    tail -n 100 "$LOG" >&2
    exit 1
fi
for marker in '[hv-x86] vCPU ready' PYTHON_IN_VM_GUEST_INIT_START \
    PYTHON_IN_VM_CRNG_READY PYTHON_IN_VM_DNS_PASS \
    PYTHON_IN_VM_APK_PASS "$SUCCESS"; do
    grep -qF "$marker" "$LOG" \
        || { echo "FAIL: missing guest marker: $marker" >&2; tail -n 100 "$LOG" >&2; exit 1; }
done
grep -qE 'Python 3\.[0-9]+' "$LOG" \
    || { echo 'FAIL: CPython version not observed in guest' >&2; exit 1; }
echo "PASS: CPython package installed over HTTPS; JSON-to-CSV and child consumer ran inside Tier 3 guest — $LOG"
echo 'Scope: QEMU-TCG 10.2.0 emulator evidence only; not physical x86 qualification.'
