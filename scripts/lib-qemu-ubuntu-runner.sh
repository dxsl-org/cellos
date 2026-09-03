#!/usr/bin/env bash
# Helper functions for the x86 Ubuntu 24.04 wide-guest QEMU runner.
# Sourced by scripts/qemu-ubuntu-wide-guest-x86.sh.

set -euo pipefail

fatal_pattern='KERNEL PANIC|\[fault\] Cell|\[hv-x86\].*(fail|error|unexpected|unsupported|unknown vmexit|unhandled|triple-fault|unavailable|absent|volatile disk)'
failure_marker_pattern='^USER: \[guest-uart\] CELLOS_UBUNTU_(ROOT_MOUNT|APT|SECOND_BOOT)_FAIL_V1'

prepare_runner() {
    local iso="$1" disk="$2" part_start="$3" guest_disk="$4"
    truncate -s 0 "$disk"
    truncate -s 5G "$disk"
    printf 'label: dos\nunit: sectors\n\nstart=2048,type=c\n' | sfdisk "$disk" >/dev/null
    mkfs.fat -F 32 --offset="$part_start" -n CELLOSUBUNTU "$disk" >/dev/null
    mcopy -o -i "$disk@@$((part_start * 512))" "$guest_disk" ::/guest_disk.img
    sync -d "$disk"

    QEMU_ISO="$(realpath "$iso")"
    QEMU_DISK="$(realpath "$disk")"
    if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
        command -v wslpath >/dev/null 2>&1 \
            || { echo "BLOCKED_ENVIRONMENT: Windows QEMU requires wslpath" >&2; exit 1; }
        QEMU_ISO="$(wslpath -w "$QEMU_ISO")"
        QEMU_DISK="$(wslpath -w "$QEMU_DISK")"
    fi
}

cleanup_qemu() {
    if [[ -n "${QEMU_PID:-}" ]] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
    QEMU_PID=""
    { exec 3>&-; } 2>/dev/null || true
    rm -f "${INPUT_FIFO:-}"
}

fail_boot() {
    local phase="$1" reason="$2" log="$3"
    echo "FAIL: $phase: $reason" >&2
    echo "--- last 200 lines of $log ---" >&2
    tr -d '\000\r' < "$log" | tail -n 200 >&2 || true
    exit 1
}

start_boot() {
    local phase="$1" log="$2"
    : > "$log"
    rm -f "$INPUT_FIFO"
    mkfifo "$INPUT_FIFO"
    exec 3<> "$INPUT_FIFO"
    "$QEMU_X86_BIN" -machine q35 -device intel-iommu,intremap=on \
        -accel tcg -cpu qemu64,+pdpe1gb,+svm -m "$QEMU_MEMORY" -nographic \
        -cdrom "$QEMU_ISO" -boot d -no-reboot \
        -drive "file=$QEMU_DISK,if=none,id=nvme0,format=raw" \
        -device nvme,drive=nvme0,serial=CELLOSUBUNTU \
        -netdev user,id=net0,net=10.0.2.0/24 \
        -device e1000,netdev=net0,mac=52:54:00:12:34:58 \
        <&3 > "$log" 2>&1 &
    QEMU_PID=$!
    echo "[ubuntu-wide-runner] $phase started (pid=$QEMU_PID, window=${BOOT_WINDOW}s)"
}

wait_exact_line() {
    local phase="$1" marker="$2" log="$3" deadline=$((SECONDS + BOOT_WINDOW))
    while (( SECONDS < deadline )); do
        grep -axFq "USER: [guest-uart] $marker" "$log" 2>/dev/null && return 0
        grep -aEqi "$fatal_pattern" "$log" 2>/dev/null \
            && fail_boot "$phase" "fatal guest or hypervisor marker" "$log"
        grep -aEqi "$failure_marker_pattern" "$log" 2>/dev/null \
            && fail_boot "$phase" "explicit guest failure marker" "$log"
        grep -axEq 'USER: \[hv-x86\] (guest shutdown|guest power-off port write|guest exited)' "$log" 2>/dev/null \
            && fail_boot "$phase" "guest stopped before '$marker'" "$log"
        kill -0 "$QEMU_PID" 2>/dev/null \
            || fail_boot "$phase" "outer QEMU exited before '$marker'" "$log"
        sleep 1
    done
    fail_boot "$phase" "deadline expired before '$marker'" "$log"
}

assert_boot_contract() {
    local phase="$1" log="$2"
    grep -axFq 'USER: [hv-x86] guest profile: ubuntu-wide-guest (512 MiB, /dev/vda)' "$log" \
        || fail_boot "$phase" "Ubuntu wide-guest boot profile marker missing" "$log"
    grep -axFq 'USER: [hv-x86] persistent disk: /mnt/sd/guest_disk.img' "$log" \
        || fail_boot "$phase" "persistent production backend marker missing" "$log"
}

wait_prompt() {
    local phase="$1" log="$2" deadline=$((SECONDS + BOOT_WINDOW))
    while (( SECONDS < deadline )); do
        grep -aFq "$GUEST_PROMPT" "$log" 2>/dev/null && return 0
        grep -aEqi "$fatal_pattern" "$log" 2>/dev/null \
            && fail_boot "$phase" "fatal condition before serial login" "$log"
        grep -aEqi "$failure_marker_pattern" "$log" 2>/dev/null \
            && fail_boot "$phase" "explicit guest failure marker" "$log"
        grep -axEq 'USER: \[hv-x86\] (guest shutdown|guest power-off port write|guest exited)' "$log" 2>/dev/null \
            && fail_boot "$phase" "guest stopped before serial login" "$log"
        kill -0 "$QEMU_PID" 2>/dev/null \
            || fail_boot "$phase" "outer QEMU exited before serial login" "$log"
        sleep 1
    done
    fail_boot "$phase" "serial autologin prompt not reached" "$log"
}

wait_shutdown() {
    local phase="$1" log="$2" deadline=$((SECONDS + BOOT_WINDOW))
    while (( SECONDS < deadline )); do
        if grep -axEq 'USER: \[hv-x86\] (guest shutdown|guest power-off port write)' "$log" 2>/dev/null; then
            return 0
        fi
        grep -aEqi "$fatal_pattern" "$log" 2>/dev/null \
            && fail_boot "$phase" "fatal condition while awaiting clean guest shutdown" "$log"
        grep -aEqi "$failure_marker_pattern" "$log" 2>/dev/null \
            && fail_boot "$phase" "explicit guest failure marker" "$log"
        kill -0 "$QEMU_PID" 2>/dev/null \
            || fail_boot "$phase" "outer QEMU exited without clean guest shutdown" "$log"
        sleep 1
    done
    fail_boot "$phase" "clean guest shutdown not observed" "$log"
}

finish_boot() {
    local log="$1"
    wait_shutdown "clean boot transition" "$log"
    cleanup_qemu
    sync -d "$OUTER_DISK"
}
