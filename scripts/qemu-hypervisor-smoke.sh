#!/usr/bin/env bash
# Boot the ViCell hypervisor kernel in QEMU ARM virt (EL2) and check that the
# Tier 3b VMM booted its Alpine Linux guest to a shell.
#
# The kernel must be built with EMBEDDED_OVERRIDE pointing to a kernel_fs.img
# that contains Alpine vmlinuz-virt + the guest initramfs (see
# scripts/make-hypervisor-fs.sh). The guest boots `rdinit=/bin/pi-guest-init`,
# which mounts /dev/vda and execs busybox sh; with HOME=/ its prompt is "~ #".
#
# Both modes require that prompt. They differ only in the CPU QEMU emulates:
#
#   HV_SMOKE_MODE=machinery (default) — QEMU-TCG with an emulated cortex-a72.
#     Runs on any host.
#
#   HV_SMOKE_MODE=boot — -enable-kvm -cpu host. Needs an ARM64 host with KVM.
#
# Usage:
#   bash scripts/qemu-hypervisor-smoke.sh [kernel-elf] [disk.img]
#   kernel-elf  default: target/aarch64-unknown-none-softfloat/release/cellos-kernel
#   disk.img    default: disk_hv_arm.img
#
# Environment:
#   HV_SMOKE_MODE  machinery (default) | boot
#   HV_SMOKE_CPU   CPU QEMU emulates in machinery mode (default: cortex-a72);
#                  a VHE-capable CPU (e.g. cortex-a76) runs the VHE host kernel
#   BOOT_WINDOW    seconds to wait for the prompt (default: 300). QEMU is
#                  stopped as soon as the prompt appears.
#   LOG_TAIL       lines of qemu-hv.log printed on any failure (default: 200)
#
# The full boot output is left in qemu-hv.log (normalised) and qemu-hv.raw.log
# (verbatim) in the working directory.
#
# Exit codes:
#   0  — the guest shell prompt was reached
#   1  — timeout, kernel panic, cell fault or hypervisor error

set -euo pipefail

KERNEL="${1:-target/aarch64-unknown-none-softfloat/release/cellos-kernel}"
DISK="${2:-disk_hv_arm.img}"
BOOT_WINDOW="${BOOT_WINDOW:-300}"
HV_SMOKE_MODE="${HV_SMOKE_MODE:-machinery}"
HV_SMOKE_CPU="${HV_SMOKE_CPU:-cortex-a72}"

if [[ "$HV_SMOKE_MODE" != "machinery" && "$HV_SMOKE_MODE" != "boot" ]]; then
    echo "FAIL: HV_SMOKE_MODE must be 'machinery' or 'boot' (got '$HV_SMOKE_MODE')" >&2
    exit 1
fi

if ! command -v qemu-system-aarch64 &>/dev/null; then
    echo "FAIL: qemu-system-aarch64 not found on PATH" >&2
    exit 1
fi

for f in "$KERNEL" "$DISK"; do
    if [[ ! -f "$f" ]]; then
        echo "FAIL: required file not found: $f" >&2
        exit 1
    fi
done

echo "[hv-smoke] mode=$HV_SMOKE_MODE kernel=$KERNEL disk=$DISK (window=${BOOT_WINDOW}s)"

if [[ "$HV_SMOKE_MODE" == "boot" ]]; then
    CPU_ARGS=(-enable-kvm -cpu host)
else
    CPU_ARGS=(-cpu "$HV_SMOKE_CPU")
fi

QEMU_ARGS=(
    -machine "virt,virtualization=on,gic-version=2"
    "${CPU_ARGS[@]}"
    -m 1G
    -nographic
    -kernel "$KERNEL"
    -drive "if=none,file=$DISK,format=raw,id=hd0"
    -device virtio-blk-device,drive=hd0
    -netdev user,id=net0
    -device virtio-net-device,netdev=net0
    -no-reboot
)

# The prompt busybox sh prints once /bin/pi-guest-init execs it.
SHELL_PROMPT='~ #'
# Signatures that end the run early as failures.
FATAL_RE='KERNEL PANIC|\[fault\] Cell|\[hv\] .*(fail|error)|\[hv\] guest exited'

qemu-system-aarch64 "${QEMU_ARGS[@]}" < /dev/null > qemu-hv.raw.log 2>&1 &
QEMU_PID=$!

deadline=$((SECONDS + BOOT_WINDOW))
while kill -0 "$QEMU_PID" 2>/dev/null && (( SECONDS < deadline )); do
    if grep -qaF "$SHELL_PROMPT" qemu-hv.raw.log || grep -qaiE "$FATAL_RE" qemu-hv.raw.log; then
        break
    fi
    sleep 2
done

kill -KILL "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

# Strip NULs and ANSI escapes.
tr -d '\000' < qemu-hv.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-hv.log

# Every failure path must show the boot output itself, not only the grepped
# signature lines: a fault line says which cell died but nothing about what the
# run was doing beforehand, and on CI this stdout is the only copy unless the
# workflow also uploads qemu-hv.log. LOG_TAIL raises the bound when a failure
# needs more history than the default.
LOG_TAIL="${LOG_TAIL:-200}"
dump_log() {
    echo "--- qemu-hv.log, last ${LOG_TAIL} of $(wc -l < qemu-hv.log) lines ---" >&2
    tail -n "$LOG_TAIL" qemu-hv.log >&2
}

if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-hv.log; then
    echo "FAIL: kernel panic / cell fault detected" >&2
    # `|| true`: head closing the pipe early SIGPIPEs grep, and pipefail would
    # turn that into exit 141 instead of the documented exit 1.
    grep -ai "fault\|PANIC" qemu-hv.log 2>&1 | head -20 >&2 || true
    dump_log
    exit 1
fi

if grep -qai "\[hv\] .*fail\|\[hv\] .*error\|\[hv\] guest exited" qemu-hv.log; then
    echo "FAIL: hypervisor error" >&2
    grep -ai "\[hv\]" qemu-hv.log | tail -20 >&2
    dump_log
    exit 1
fi

if grep -qaF "$SHELL_PROMPT" qemu-hv.log; then
    echo "PASS: Alpine guest shell prompt '$SHELL_PROMPT' reached after ${SECONDS}s"
    exit 0
fi

# Printed by the hypervisor cell immediately before it first enters the guest
# vCPU; says whether the guest never started or started and stalled.
if grep -qaF '[hv] vCPU ready' qemu-hv.log; then
    echo "FAIL: guest started but did not reach '$SHELL_PROMPT' within ${BOOT_WINDOW}s" >&2
else
    echo "FAIL: '[hv] vCPU ready' not seen within ${BOOT_WINDOW}s; the VMM never entered the guest" >&2
fi
dump_log
exit 1
