#!/usr/bin/env bash
# Boot the ViCell x86_64 hypervisor kernel (Limine ISO) in QEMU q35 and check
# that the Tier 3 VMM brought up a minimal Alpine Linux guest (PVH boot).
#
# The kernel must be built with EMBEDDED_OVERRIDE=kernel/src/embedded-hv-x86
# (see scripts/make-hypervisor-fs-x86.sh) so the embedded filesystem carries
# /bin/hypervisor plus /vmlinux and /initrd.gz.
#
# Two assertion tiers (HV_SMOKE_MODE):
#
#   HV_SMOKE_MODE=machinery (default) — asserts the VMM constructed a runnable
#     vCPU and entered the guest (liveness marker), with no panic/cell-fault/
#     hv-init-error. If the guest additionally reaches its shell, that is an
#     unconditional PASS.
#
#   HV_SMOKE_MODE=boot — strict assertion: the Alpine guest must reach its
#     "/ #" prompt within BOOT_WINDOW.
#
# Both modes run under QEMU-TCG with `-cpu qemu64,+pdpe1gb,+svm`. Nested SVM
# through host KVM is Intel-host-incompatible (KVM exposes VT-x, not AMD-V), so
# unlike the ARM lane there is no KVM fast path here; TCG emulation of SVM makes
# the runs slow — give BOOT_WINDOW generous headroom.
#
# Usage:
#   bash scripts/qemu-hypervisor-smoke-x86.sh [iso]
#   iso  default: build/vicell-x86-hv.iso
#
# Environment:
#   HV_SMOKE_MODE  machinery (default) | boot
#   BOOT_WINDOW    seconds to wait (default: 900)
#   LOG_TAIL       lines of qemu-hv-x86.log printed on any failure (default 200)
#
# Exit codes:
#   0 — assertion for the selected mode passed
#   1 — timeout, kernel panic, hypervisor error, or missing liveness marker

set -euo pipefail

ISO="${1:-build/vicell-x86-hv.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-900}"
HV_SMOKE_MODE="${HV_SMOKE_MODE:-machinery}"

if [[ "$HV_SMOKE_MODE" != "machinery" && "$HV_SMOKE_MODE" != "boot" ]]; then
    echo "FAIL: HV_SMOKE_MODE must be 'machinery' or 'boot' (got '$HV_SMOKE_MODE')" >&2
    exit 1
fi

if ! command -v qemu-system-x86_64 &>/dev/null; then
    echo "FAIL: qemu-system-x86_64 not found on PATH" >&2
    exit 1
fi

if [[ ! -f "$ISO" ]]; then
    echo "FAIL: ISO not found: $ISO" >&2
    echo "  Build with: bash scripts/make-hypervisor-fs-x86.sh && \\" >&2
    echo "    RUSTFLAGS='-C relocation-model=pic -C code-model=kernel -C target-feature=-red-zone' \\" >&2
    echo "    EMBEDDED_OVERRIDE='kernel/src/embedded-hv-x86' cargo build --release -p cellos-kernel --target x86_64-unknown-none && \\" >&2
    echo "    bash scripts/x86/make-iso-ci.sh build/vicell-x86-hv.iso" >&2
    exit 1
fi

echo "[hv-smoke-x86] mode=$HV_SMOKE_MODE iso=$ISO (window=${BOOT_WINDOW}s)"

timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    -accel tcg \
    -cpu qemu64,+pdpe1gb,+svm \
    -m 1G \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    < /dev/null > qemu-hv-x86.raw.log 2>&1 || true

# Strip NULs and ANSI escapes.
tr -d '\000' < qemu-hv-x86.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-hv-x86.log

LOG_TAIL="${LOG_TAIL:-200}"
dump_log() {
    echo "--- qemu-hv-x86.log, last ${LOG_TAIL} of $(wc -l < qemu-hv-x86.log) lines ---" >&2
    tail -n "$LOG_TAIL" qemu-hv-x86.log >&2
}

# Real defects — fatal in both modes, checked before any mode-specific logic.
if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-hv-x86.log; then
    echo "FAIL: kernel panic / cell fault detected" >&2
    grep -ai "fault\|PANIC" qemu-hv-x86.log 2>&1 | head -20 >&2 || true
    dump_log
    exit 1
fi

# Hypervisor-cell failures are fatal in both modes.
if grep -qi "\[hv-x86\] .*fail\|\[hv-x86\] .*error\|\[hv-x86\] unhandled\|\[hv-x86\] unexpected\|\[hv-x86\] guest exited" qemu-hv-x86.log; then
    echo "FAIL: hypervisor cell error before/during guest boot" >&2
    grep -i "\[hv-x86\]" qemu-hv-x86.log | tail -20 >&2
    dump_log
    exit 1
fi

# Liveness marker: printed by the x86 hypervisor cell immediately before it
# enters the guest vCPU for the first time (cells/services/hypervisor/src/
# boot_x86.rs). Its absence means the VMM never brought the guest up at all.
LIVENESS_MARKER='[hv-x86] vCPU ready'
if ! grep -qF "$LIVENESS_MARKER" qemu-hv-x86.log; then
    echo "FAIL: liveness marker '$LIVENESS_MARKER' not seen — VMM never brought the guest up" >&2
    dump_log
    exit 1
fi

# Guest reached its busybox shell (cmdline rdinit=/bin/sh on console=ttyS0).
shell_reached() {
    grep -q "^/ #" qemu-hv-x86.log || grep -q $'/ #' qemu-hv-x86.log \
        || grep -qP "~ #|localhost:~#" qemu-hv-x86.log 2>/dev/null
}

if [[ "$HV_SMOKE_MODE" == "boot" ]]; then
    if shell_reached; then
        echo "PASS: Alpine guest '/ #' prompt reached — x86 hypervisor smoke test OK"
        exit 0
    fi
    echo "FAIL: Alpine '/ #' prompt not seen within ${BOOT_WINDOW}s" >&2
    dump_log
    exit 1
fi

# machinery mode: vCPU liveness alone proves the VMM machinery ran; a shell is
# an unconditional upgrade.
echo "PASS: machinery ran — x86 VMM entered the guest (liveness marker present)"
exit 0
