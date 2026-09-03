#!/usr/bin/env bash
# Boot the ViCell x86_64 hypervisor kernel (Limine ISO) in QEMU q35 and check
# that the Tier 3 VMM brought up a minimal Alpine Linux guest (PVH boot).
#
# The kernel must be built with EMBEDDED_OVERRIDE=kernel/src/embedded-hv-x86
# (see scripts/make-hypervisor-fs-x86.sh) so the embedded filesystem carries
# /bin/hypervisor plus /vmlinux and /initrd.gz.
#
# Three assertion tiers (HV_SMOKE_MODE):
#
#   HV_SMOKE_MODE=machinery (default) — asserts the VMM constructed a runnable
#     vCPU and entered the guest (liveness marker), with no panic/cell-fault/
#     hv-init-error. If the guest additionally reaches its shell, that is an
#     unconditional PASS.
#
#   HV_SMOKE_MODE=boot — strict assertion: the Alpine guest must reach its
#     "/ #" prompt within BOOT_WINDOW.
#
#   HV_SMOKE_MODE=host-shell — regression gate for the full init profile:
#     asserts that probes for absent optional `/bin` entries terminate and the
#     packaged x86 shell reaches its `Cellos >` prompt. Guest SVM failure after
#     the prompt is outside this mode's contract.
#
# All modes run under QEMU-TCG with `-cpu qemu64,+pdpe1gb,+svm`. Nested SVM
# through host KVM is Intel-host-incompatible (KVM exposes VT-x, not AMD-V), so
# unlike the ARM lane there is no KVM fast path here; TCG emulation of SVM makes
# the runs slow — give BOOT_WINDOW generous headroom.
# QEMU-TCG 10.2.0 is qualified for this lane at 1G and 2G. Ubuntu 24.04's
# 8.2.2 build deterministically triple-faults on the same ISO; select another
# runtime with QEMU_X86_BIN.
#
# Usage:
#   bash scripts/qemu-hypervisor-smoke-x86.sh [iso]
#   iso  default: build/vicell-x86-hv.iso
#
# Environment:
#   HV_SMOKE_MODE  machinery (default) | boot | host-shell
#   BOOT_WINDOW    seconds to wait (default: 900; host-shell: 60)
#   QEMU_X86_BIN   emulator executable (default: qemu-system-x86_64)
#   QEMU_MEMORY    outer Cellos VM memory (default: 1G)
#   LOG_TAIL       lines of qemu-hv-x86.log printed on any failure (default 200)
#
# Exit codes:
#   0 — assertion for the selected mode passed
#   1 — timeout, kernel panic, hypervisor error, or missing liveness marker

set -euo pipefail

ISO="${1:-build/vicell-x86-hv.iso}"
HV_SMOKE_MODE="${HV_SMOKE_MODE:-machinery}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
QEMU_MEMORY="${QEMU_MEMORY:-1G}"
if [[ "$HV_SMOKE_MODE" == "host-shell" ]]; then
    BOOT_WINDOW="${BOOT_WINDOW:-60}"
else
    BOOT_WINDOW="${BOOT_WINDOW:-900}"
fi

if [[ "$HV_SMOKE_MODE" != "machinery" && "$HV_SMOKE_MODE" != "boot" && "$HV_SMOKE_MODE" != "host-shell" ]]; then
    echo "FAIL: HV_SMOKE_MODE must be 'machinery', 'boot', or 'host-shell' (got '$HV_SMOKE_MODE')" >&2
    exit 1
fi

if ! command -v "$QEMU_X86_BIN" &>/dev/null; then
    echo "BLOCKED_ENVIRONMENT: QEMU executable not found: $QEMU_X86_BIN" >&2
    exit 1
fi

QEMU_VERSION_LINE="$("$QEMU_X86_BIN" --version 2>&1 | sed -n '1p')"
if [[ "$QEMU_VERSION_LINE" != "QEMU emulator version 10.2.0" ]]; then
    echo "BLOCKED_ENVIRONMENT: requires exact 'QEMU emulator version 10.2.0' (got '$QEMU_VERSION_LINE' from $QEMU_X86_BIN)" >&2
    exit 1
fi

if [[ ! -f "$ISO" ]]; then
    echo "FAIL: ISO not found: $ISO" >&2
    echo "  Build with: HV_VOLATILE_DISK=1 bash scripts/make-hypervisor-fs-x86.sh && \\" >&2
    echo "    RUSTFLAGS='-C relocation-model=static -C code-model=kernel -C no-redzone=yes -Z cf-protection=full' \\" >&2
    echo "    EMBEDDED_OVERRIDE='kernel/src/embedded-hv-x86' cargo build --release -p cellos-kernel --target x86_64-unknown-none && \\" >&2
    echo "    bash scripts/x86/make-iso-ci.sh build/vicell-x86-hv.iso" >&2
    exit 1
fi

QEMU_ISO="$ISO"
if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
    if ! command -v wslpath &>/dev/null; then
        echo "FAIL: Windows QEMU requires wslpath to translate the ISO path" >&2
        exit 1
    fi
    QEMU_ISO="$(wslpath -w "$(realpath "$ISO")")"
fi
QEMU_VERSION="$("$QEMU_X86_BIN" --version | sed -n '1p')"

echo "[hv-smoke-x86] mode=$HV_SMOKE_MODE iso=$ISO memory=$QEMU_MEMORY (window=${BOOT_WINDOW}s)"
echo "[hv-smoke-x86] $QEMU_VERSION"

# Kill QEMU at the exact deadline so post-window output cannot satisfy a gate.
timeout --signal=KILL "$BOOT_WINDOW" "$QEMU_X86_BIN" \
    -machine q35 \
    -accel tcg \
    -cpu qemu64,+pdpe1gb,+svm \
    -m "$QEMU_MEMORY" \
    -nographic \
    -cdrom "$QEMU_ISO" \
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

if [[ "$HV_SMOKE_MODE" == "host-shell" ]]; then
    if grep -qF "Cellos > " qemu-hv-x86.log; then
        echo "PASS: x86 host shell reached the 'Cellos >' prompt"
        exit 0
    fi
    echo "FAIL: x86 host shell prompt not seen within ${BOOT_WINDOW}s" >&2
    dump_log
    exit 1
fi

if grep -qi "\[hv-x86\] guest triple-fault" qemu-hv-x86.log; then
    echo "FAIL: guest triple-fault under $QEMU_VERSION" >&2
    echo "  QEMU-TCG 8.2.2 is a known-incompatible SVM runtime; use QEMU_X86_BIN with the qualified 10.2.0 build." >&2
    dump_log
    exit 1
fi

# Hypervisor-cell failures are fatal in both guest modes.
if grep -qi "\[hv-x86\] .*fail\|\[hv-x86\] .*error\|\[hv-x86\] unhandled guest MMIO\|\[hv-x86\] unexpected\|\[hv-x86\] guest exited" qemu-hv-x86.log; then
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
