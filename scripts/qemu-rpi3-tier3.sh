#!/usr/bin/env bash
# QEMU raspi3b development gate; does not qualify physical Raspberry Pi hardware.
# Usage: bash scripts/qemu-rpi3-tier3.sh <board-rpi3-kernel8.img> [sd-image]
# QEMU's ELF direct boot uses a different exception-level handoff; pass the
# raw Raspberry Pi kernel8.img produced by make-hypervisor-fs-rpi3.sh.
# RPI3_GATE=host|machinery|boot (default: boot).  The boot gate requires the
# Alpine guest shell, not merely a constructed vCPU or a working host shell.
set -euo pipefail

kernel="${1:?usage: $0 <board-rpi3-kernel8.img> [sd-image]}"
disk="${2:-}"
gate="${RPI3_GATE:-boot}"
window="${BOOT_WINDOW:-180}"
qemu="${QEMU_ARM64_BIN:-qemu-system-aarch64}"
work_dir="${RPI3_GATE_WORK_DIR:-build/rpi3-tier3-gate}"

case "$gate" in
    host|machinery|boot) ;;
    *) echo "FAIL: RPI3_GATE must be host, machinery or boot" >&2; exit 2 ;;
esac
[[ -f "$kernel" ]] || { echo "FAIL: missing kernel: $kernel" >&2; exit 2; }
if [[ -n "$disk" ]]; then
    [[ -f "$disk" ]] || { echo "FAIL: missing SD image: $disk" >&2; exit 2; }
fi
command -v "$qemu" >/dev/null || { echo "FAIL: missing QEMU: $qemu" >&2; exit 2; }
mkdir -p "$work_dir"
log="$work_dir/raspi3b.log"
args=(-machine raspi3b -cpu cortex-a53 -m 1G -display none
      -serial null -serial stdio -kernel "$kernel" -no-reboot)
if [[ -n "$disk" ]]; then
    args+=(-drive "if=sd,file=$disk,format=raw")
fi

status=0
timeout "$window" "$qemu" "${args[@]}" </dev/null >"$log" 2>&1 || status=$?
if [[ "$status" -ne 0 && "$status" -ne 124 ]]; then
    echo "FAIL: QEMU exited $status; log: $log" >&2
    exit 1
fi
if grep -Eiq 'KERNEL PANIC|\[fault\] Cell|\[hv\].*(failed|error)|run_vcpu kernel error' "$log"; then
    echo "FAIL: kernel/VM failure; log: $log" >&2
    exit 1
fi
if ! grep -Fq 'Kernel initialization complete. Entering idle loop.' "$log"; then
    echo "FAIL: Cellos host did not initialize; log: $log" >&2
    exit 1
fi
if [[ "$gate" == host ]] && ! grep -Fq 'Cellos shell ready' "$log"; then
    echo "FAIL: host shell not reached; log: $log" >&2
    exit 1
fi
if [[ "$gate" != host ]] && ! grep -Fq '[pi-monitor] HVC/MMIO/VI/PREEMPT smoke PASS; HypervisorCap open' "$log"; then
    echo "FAIL: EL2 monitor smoke did not pass; log: $log" >&2
    exit 1
fi
if [[ "$gate" != host ]] && ! grep -Fq '[hv] vCPU ready — entering run loop' "$log"; then
    echo "FAIL: guest vCPU not entered; log: $log" >&2
    exit 1
fi
if [[ "$gate" == boot ]] && ! grep -Eq '(^|[[:space:]])(/ #|localhost:~#|~ #)' "$log"; then
    echo "FAIL: Alpine guest shell not reached; log: $log" >&2
    exit 1
fi
echo "PASS: raspi3b $gate gate; log: $log"
