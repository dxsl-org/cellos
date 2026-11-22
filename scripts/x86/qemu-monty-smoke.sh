#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Boot the x86_64 monty-smoke witness image and run /bin/monty-smoke from the shell.
#
# Asserts, for each run:
#   * every case marker and the final "[monty-smoke] PASS" (Monty running Python
#     in a Tier-2 UNTRUSTED rust-std Cell);
#   * no "[monty-smoke] ... FAIL" marker;
#   * no "KERNEL PANIC", "panicked at" or "[fault] Cell" line;
#   * the run's Tier-2 domain is released: DOMAIN-FRAME-RELEASE: PASS rises by
#     exactly one per run (the marker names no Cell, so it is counted).
#
# Usage: bash scripts/x86/qemu-monty-smoke.sh [kernel-elf] [iso-out]
# Env: BOOT_WINDOW (default 240), X86_ACCEL (kvm|tcg), X86_CPU_MODEL, LOG_DIR,
#      MONTY_SMOKE_RUNS (default 1)
# Requires the image from scripts/build-x86_64-monty-smoke-ci.sh.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

KERNEL="${1:-target/x86-monty-smoke/x86_64-unknown-none/release/cellos-kernel-monty-smoke}"
ISO="${2:-build/vicell-x86-monty-smoke.iso}"
ISO_ROOT="build/x86-monty-smoke-iso-root"
BOOT_WINDOW="${BOOT_WINDOW:-240}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT}"
RUNS="${MONTY_SMOKE_RUNS:-1}"
X86_ACCEL="${X86_ACCEL:-}"
X86_CPU_MODEL="${X86_CPU_MODEL:-}"
if [[ -z "$X86_CPU_MODEL" ]]; then
    if [[ "$X86_ACCEL" == "kvm" ]]; then X86_CPU_MODEL=host; else X86_CPU_MODEL="qemu64,+pdpe1gb,+smep,+smap,+umip"; fi
fi
case "$X86_ACCEL" in ""|kvm|tcg) ;; *) echo "FAIL: X86_ACCEL must be empty, kvm or tcg" >&2; exit 1 ;; esac

[[ -f "$KERNEL" ]] || { echo "FAIL: kernel not found: $KERNEL (run scripts/build-x86_64-monty-smoke-ci.sh)" >&2; exit 1; }
command -v qemu-system-x86_64 >/dev/null 2>&1 || { echo "FAIL: qemu-system-x86_64 not found" >&2; exit 1; }

LABEL="x86-monty-smoke${X86_ACCEL:+-$X86_ACCEL}"
RAW_LOG="$LOG_DIR/qemu-$LABEL.raw.log"
FIFO="$(mktemp -u "${TMPDIR:-/tmp}/cellos-monty-smoke-XXXXXX.fifo")"
mkfifo -m 600 "$FIFO"
QEMU_PID=""
cleanup() {
    [[ -n "$QEMU_PID" ]] && kill "$QEMU_PID" 2>/dev/null || true
    rm -f "$FIFO"
}
trap cleanup EXIT

echo "[monty-smoke] Building ISO from $KERNEL"
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" bash scripts/x86/make-iso-ci.sh "$ISO" >/dev/null

ACCEL_ARGS=()
[[ -n "$X86_ACCEL" ]] && ACCEL_ARGS=(-accel "$X86_ACCEL")
echo "[monty-smoke] Booting $ISO (window ${BOOT_WINDOW}s, cpu=$X86_CPU_MODEL${X86_ACCEL:+, accel=$X86_ACCEL})"
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 "${ACCEL_ARGS[@]}" -cpu "$X86_CPU_MODEL" -smp 1 -m 256M \
    -nographic -cdrom "$ISO" -boot d -no-reboot \
    < "$FIFO" > "$RAW_LOG" 2>&1 &
QEMU_PID=$!
exec 9> "$FIFO"

clean_log() { tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g'; }
count() { clean_log | grep -acE "$1" || true; }
# wait_for <pattern> <seconds>: 0 on match, 1 on timeout or QEMU exit.
wait_for() {
    local i
    for ((i = 0; i < $2 * 2; i++)); do
        clean_log | grep -aqE "$1" && return 0
        kill -0 "$QEMU_PID" 2>/dev/null || { clean_log | grep -aqE "$1"; return $?; }
        sleep 0.5
    done
    return 1
}
fail() {
    echo "FAIL: $1" >&2
    clean_log | tail -60 >&2
    exit 1
}

CASES=(fib nesting-at-limit nesting-over-limit random json feed-timeout)
PASS="\[monty-smoke\] PASS$"
BAD="\[monty-smoke\] .*FAIL"
FAULT="KERNEL PANIC|panicked at|\[fault\] Cell"

wait_for "Cellos > " 120 || fail "shell prompt not reached"
released_before="$(count "DOMAIN-FRAME-RELEASE: PASS")"
for ((run = 1; run <= RUNS; run++)); do
    before_pass="$(count "$PASS")"
    sleep 1
    printf 'monty-smoke\n' >&9
    for ((i = 0; i < 2 * BOOT_WINDOW; i++)); do
        [[ "$(count "$PASS")" -gt "$before_pass" ]] && break
        [[ "$(count "$BAD")" -gt 0 ]] && fail "run $run printed a FAIL marker"
        [[ "$(count "$FAULT")" -gt 0 ]] && fail "run $run: kernel or Cell fault"
        kill -0 "$QEMU_PID" 2>/dev/null || fail "QEMU exited during run $run"
        sleep 0.5
    done
    [[ "$(count "$PASS")" -gt "$before_pass" ]] || fail "run $run did not print '[monty-smoke] PASS'"
done
for case in "${CASES[@]}"; do
    [[ "$(count "\[monty-smoke\] $case PASS")" -eq "$RUNS" ]] || fail "case $case did not pass on every run"
done
[[ "$(count "$BAD")" -eq 0 ]] || fail "a FAIL marker was printed"
[[ "$(count "$FAULT")" -eq 0 ]] || fail "kernel or Cell fault"
for ((i = 0; i < 20; i++)); do
    [[ "$(count "DOMAIN-FRAME-RELEASE: PASS")" -ge $((released_before + RUNS)) ]] && break
    sleep 0.5
done
released=$(($(count "DOMAIN-FRAME-RELEASE: PASS") - released_before))
[[ "$released" -eq "$RUNS" ]] \
    || fail "$released of $RUNS monty-smoke domains released (DOMAIN-FRAME-RELEASE: PASS)"

clean_log | grep -aE "\[monty-smoke\] " | sort | uniq -c | sed 's/^/[monty-smoke] /'
echo "MONTY-SMOKE: PASS (runs: $RUNS; log: $RAW_LOG)"
