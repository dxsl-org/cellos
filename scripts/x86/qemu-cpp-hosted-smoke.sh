#!/usr/bin/env bash
# Boot the x86_64 `cpp-hosted` witness image and run /bin/cpp-hosted-smoke from
# the shell (ADR-0034, runtime plan R4).
#
# Asserts, for each run:
#   * "CPP-HOSTED: 11/11 pass": containers, optional/variant, charconv, a throw
#     caught three frames up, dynamic_cast, a joined std::thread, std::future,
#     std::shared_mutex, thread_local, a contended thread-safe static, std::cout;
#   * no "CPP-HOSTED: ... FAIL|NOT-BUILT" marker;
#   * the uncaught exception that follows ends only the Cell, with a logged reason:
#     "libc++abi: terminating due to uncaught exception of type std::runtime_error:
#     cpp-hosted uncaught", then a fresh shell prompt;
#   * no "KERNEL PANIC", "panicked at" or "[fault] Cell" line (the abort path
#     exits through mlibc's panic, never a CPU fault);
#   * the run's Tier-2 domain is released (DOMAIN-FRAME-RELEASE: PASS, test-hooks kernel).
#
# Usage: bash scripts/x86/qemu-cpp-hosted-smoke.sh [kernel-elf] [iso-out]
# Env: BOOT_WINDOW (default 180), X86_ACCEL (kvm|tcg), X86_CPU_MODEL, LOG_DIR,
#      CPP_HOSTED_RUNS (default 1)
# Requires the image from scripts/build-x86_64-cpp-hosted-ci.sh.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

KERNEL="${1:-target/x86-cpp-hosted/x86_64-unknown-none/release/cellos-kernel-cpp-hosted}"
ISO="${2:-build/vicell-x86-cpp-hosted.iso}"
ISO_ROOT="build/x86-cpp-hosted-iso-root"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT}"
CPP_HOSTED_RUNS="${CPP_HOSTED_RUNS:-1}"
X86_ACCEL="${X86_ACCEL:-}"
X86_CPU_MODEL="${X86_CPU_MODEL:-}"
if [[ -z "$X86_CPU_MODEL" ]]; then
    if [[ "$X86_ACCEL" == "kvm" ]]; then X86_CPU_MODEL=host; else X86_CPU_MODEL="qemu64,+pdpe1gb"; fi
fi
case "$X86_ACCEL" in ""|kvm|tcg) ;; *) echo "FAIL: X86_ACCEL must be empty, kvm or tcg" >&2; exit 1 ;; esac

[[ -f "$KERNEL" ]] || { echo "FAIL: kernel not found: $KERNEL (run scripts/build-x86_64-cpp-hosted-ci.sh)" >&2; exit 1; }
command -v qemu-system-x86_64 >/dev/null 2>&1 || { echo "FAIL: qemu-system-x86_64 not found" >&2; exit 1; }

LABEL="x86-cpp-hosted${X86_ACCEL:+-$X86_ACCEL}"
RAW_LOG="$LOG_DIR/qemu-$LABEL.raw.log"
FIFO="$(mktemp -u "${TMPDIR:-/tmp}/cellos-cpp-hosted-XXXXXX.fifo")"
mkfifo -m 600 "$FIFO"
QEMU_PID=""
cleanup() {
    [[ -n "$QEMU_PID" ]] && kill "$QEMU_PID" 2>/dev/null || true
    rm -f "$FIFO"
}
trap cleanup EXIT

echo "[cpp-hosted] Building ISO from $KERNEL"
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" bash scripts/x86/make-iso-ci.sh "$ISO" >/dev/null

ACCEL_ARGS=()
[[ -n "$X86_ACCEL" ]] && ACCEL_ARGS=(-accel "$X86_ACCEL")
echo "[cpp-hosted] Booting $ISO (window ${BOOT_WINDOW}s, cpu=$X86_CPU_MODEL${X86_ACCEL:+, accel=$X86_ACCEL})"
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 "${ACCEL_ARGS[@]}" -cpu "$X86_CPU_MODEL" -smp 1 -m 256M \
    -nographic -cdrom "$ISO" -boot d -no-reboot \
    < "$FIFO" > "$RAW_LOG" 2>&1 &
QEMU_PID=$!
exec 9> "$FIFO"

clean_log() { tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g'; }
prompts() { clean_log | grep -ac "Cellos > " || true; }
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

PASS="CPP-HOSTED: 11/11 pass"
BAD="CPP-HOSTED: .*(FAIL|NOT-BUILT)"
TERMINATED="libc\+\+abi: terminating due to uncaught exception of type std::runtime_error: cpp-hosted uncaught"
FAULT="KERNEL PANIC|panicked at|\[fault\] Cell"

wait_for "Cellos > " 120 || fail "shell prompt not reached"
released_before="$(count "DOMAIN-FRAME-RELEASE: PASS")"
for ((run = 1; run <= CPP_HOSTED_RUNS; run++)); do
    before_prompts="$(prompts)"
    before_pass="$(count "$PASS")"
    before_term="$(count "$TERMINATED")"
    sleep 1
    printf 'cpp-hosted-smoke\n' >&9
    for ((i = 0; i < 180; i++)); do
        [[ "$(count "$TERMINATED")" -gt "$before_term" ]] && break
        [[ "$(count "$BAD")" -gt 0 ]] && fail "run $run printed a FAIL marker"
        [[ "$(count "$FAULT")" -gt 0 ]] && fail "run $run: kernel or Cell fault"
        kill -0 "$QEMU_PID" 2>/dev/null || fail "QEMU exited during run $run"
        sleep 0.5
    done
    [[ "$(count "$PASS")" -gt "$before_pass" ]] || fail "run $run did not print '$PASS'"
    [[ "$(count "$TERMINATED")" -gt "$before_term" ]] || fail "run $run: no libc++abi terminate line"
    # The next prompt proves the shell survived the Cell's abort.
    for ((i = 0; i < 60; i++)); do
        [[ "$(prompts)" -gt "$before_prompts" ]] && break
        sleep 0.5
    done
    [[ "$(prompts)" -gt "$before_prompts" ]] || fail "no shell prompt after run $run"
done
[[ "$(count "$BAD")" -eq 0 ]] || fail "a FAIL marker was printed"
[[ "$(count "$FAULT")" -eq 0 ]] || fail "kernel or Cell fault"
for ((i = 0; i < 20; i++)); do
    [[ "$(count "DOMAIN-FRAME-RELEASE: PASS")" -ge $((released_before + CPP_HOSTED_RUNS)) ]] && break
    sleep 0.5
done
released=$(($(count "DOMAIN-FRAME-RELEASE: PASS") - released_before))
[[ "$released" -eq "$CPP_HOSTED_RUNS" ]] \
    || fail "$released of $CPP_HOSTED_RUNS cpp-hosted domains released (DOMAIN-FRAME-RELEASE: PASS)"

clean_log | grep -aE "CPP-HOSTED: |libc\+\+abi: |mlibc PANIC|mlibc: " | sort | uniq -c | sed 's/^/[cpp-hosted] /'
echo "CPP-HOSTED-SMOKE: PASS (runs: $CPP_HOSTED_RUNS; log: $RAW_LOG)"
