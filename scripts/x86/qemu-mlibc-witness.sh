#!/usr/bin/env bash
# Boot the x86_64 mlibc witness image and run both witnesses from the shell.
#
# Asserts, in order:
#   * /bin/mlibc-smoke     — "MLIBC-SMOKE: 3/3 pass" (malloc, printf, clock)
#   * /bin/mlibc-posix-x86 — "MLIBC-POSIX: 7/7 pass" (threads, once, TLS/errno
#     per thread, timed condvar, clock_nanosleep, errno after open,
#     dl_iterate_phdr + build-id)
# and that neither printed a FAIL marker, and that every posix run's Tier-2
# domain is released (one DOMAIN-FRAME-RELEASE: PASS per run; test-hooks kernel).
# Both run through mlibc's start-up (`__cellos_libc_start` on the kernel's
# initial stack with auxv; runtime plan slice RE), so a pass is also the
# witness for that path.
#
# Usage: bash scripts/x86/qemu-mlibc-witness.sh [kernel-elf] [iso-out]
# Env: BOOT_WINDOW (default 180), X86_ACCEL (kvm|tcg), X86_CPU_MODEL, LOG_DIR
# Requires the image from scripts/build-x86_64-mlibc-witness-ci.sh.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

KERNEL="${1:-target/x86-mlibc-witness/x86_64-unknown-none/release/cellos-kernel-mlibc-witness}"
ISO="${2:-build/vicell-x86-mlibc-witness.iso}"
ISO_ROOT="build/x86-mlibc-witness-iso-root"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT}"
X86_ACCEL="${X86_ACCEL:-}"
X86_CPU_MODEL="${X86_CPU_MODEL:-}"
if [[ -z "$X86_CPU_MODEL" ]]; then
    if [[ "$X86_ACCEL" == "kvm" ]]; then X86_CPU_MODEL=host; else X86_CPU_MODEL="qemu64,+pdpe1gb"; fi
fi
case "$X86_ACCEL" in ""|kvm|tcg) ;; *) echo "FAIL: X86_ACCEL must be empty, kvm or tcg" >&2; exit 1 ;; esac

[[ -f "$KERNEL" ]] || { echo "FAIL: kernel not found: $KERNEL (run scripts/build-x86_64-mlibc-witness-ci.sh)" >&2; exit 1; }
command -v qemu-system-x86_64 >/dev/null 2>&1 || { echo "FAIL: qemu-system-x86_64 not found" >&2; exit 1; }

# QEMU's serial log is redirected into LOG_DIR from a background job; a missing
# directory fails only that redirection and surfaces as "shell prompt not reached".
mkdir -p -- "$LOG_DIR"
LABEL="x86-mlibc-witness${X86_ACCEL:+-$X86_ACCEL}"
RAW_LOG="$LOG_DIR/qemu-$LABEL.raw.log"
FIFO="$(mktemp -u "${TMPDIR:-/tmp}/cellos-mlibc-witness-XXXXXX.fifo")"
mkfifo -m 600 "$FIFO"
QEMU_PID=""
cleanup() {
    [[ -n "$QEMU_PID" ]] && kill "$QEMU_PID" 2>/dev/null || true
    rm -f "$FIFO"
}
trap cleanup EXIT

echo "[mlibc-witness] Building ISO from $KERNEL"
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" bash scripts/x86/make-iso-ci.sh "$ISO" >/dev/null

ACCEL_ARGS=()
[[ -n "$X86_ACCEL" ]] && ACCEL_ARGS=(-accel "$X86_ACCEL")
echo "[mlibc-witness] Booting $ISO (window ${BOOT_WINDOW}s, cpu=$X86_CPU_MODEL${X86_ACCEL:+, accel=$X86_ACCEL})"
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 "${ACCEL_ARGS[@]}" -cpu "$X86_CPU_MODEL" -smp 1 -m 256M \
    -nographic -cdrom "$ISO" -boot d -no-reboot \
    < "$FIFO" > "$RAW_LOG" 2>&1 &
QEMU_PID=$!
exec 9> "$FIFO"

clean_log() { tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g'; }
prompts() { clean_log | grep -ac "Cellos > " || true; }
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
count() { clean_log | grep -acE "$1" || true; }
# run_witness <command> <pass-pattern> <fail-pattern>: type the command once and
# require one more pass line, no fail line, and a fresh prompt afterwards.
run_witness() {
    local command="$1" pass="$2" bad="$3" before_prompts before_pass i
    before_prompts="$(prompts)"
    before_pass="$(count "$pass")"
    sleep 1
    printf '%s\n' "$command" >&9
    for ((i = 0; i < 180; i++)); do
        [[ "$(count "$pass")" -gt "$before_pass" ]] && break
        [[ "$(count "$bad")" -gt 0 ]] && fail "$command printed a FAIL marker"
        kill -0 "$QEMU_PID" 2>/dev/null || fail "QEMU exited during $command"
        sleep 0.5
    done
    [[ "$(count "$pass")" -gt "$before_pass" ]] || fail "$command did not print '$pass'"
    [[ "$(count "$bad")" -eq 0 ]] || fail "$command printed a FAIL marker"
    # The next prompt proves the shell survived the witness's exit.
    for ((i = 0; i < 60; i++)); do
        [[ "$(prompts)" -gt "$before_prompts" ]] && return 0
        sleep 0.5
    done
    fail "no shell prompt after $command"
}

# MLIBC_POSIX_RUNS repeats the posix witness in the same boot (phase R3's
# 100-run criterion); raise BOOT_WINDOW with it.
MLIBC_POSIX_RUNS="${MLIBC_POSIX_RUNS:-1}"
wait_for "Cellos > " 120 || fail "shell prompt not reached"
run_witness mlibc-smoke "MLIBC-SMOKE: 3/3 pass" "MLIBC-SMOKE: .*FAIL"
# Domain release: each posix run spawns and retires 8 threads in its own Tier-2
# domain, and the test-hooks kernel logs one DOMAIN-FRAME-RELEASE per dropped
# domain. A retired Cell whose root stays referenced never logs it (slice TL: a
# switch plan alive across a dying thread's last switch kept the domain, 14
# frames and ~620 KiB of kernel heap per run).
released_before="$(count "DOMAIN-FRAME-RELEASE: PASS")"
for ((run = 1; run <= MLIBC_POSIX_RUNS; run++)); do
    run_witness mlibc-posix-x86 "MLIBC-POSIX: 7/7 pass" "MLIBC-POSIX: .*(FAIL|NOT-BUILT)"
done
for ((i = 0; i < 20; i++)); do
    [[ "$(count "DOMAIN-FRAME-RELEASE: PASS")" -ge $((released_before + MLIBC_POSIX_RUNS)) ]] && break
    sleep 0.5
done
released=$(($(count "DOMAIN-FRAME-RELEASE: PASS") - released_before))
[[ "$released" -eq "$MLIBC_POSIX_RUNS" ]] \
    || fail "$released of $MLIBC_POSIX_RUNS posix domains released (DOMAIN-FRAME-RELEASE: PASS)"
echo "[mlibc-witness] domains released: $released of $MLIBC_POSIX_RUNS posix runs"

clean_log | grep -aE "MLIBC-SMOKE|MLIBC-POSIX: [0-9]+/7" | sort | uniq -c | sed 's/^/[mlibc-witness] /'
echo "MLIBC-WITNESS: PASS (posix runs: $MLIBC_POSIX_RUNS; log: $RAW_LOG)"
