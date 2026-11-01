#!/usr/bin/env bash
# Collect one immutable x86_64 benchmark capture (profile x86_64-q35-2c-256m-v1).
#
# Usage:
#   scripts/x86/run-bench-x86.sh [tcg|kvm] [capture-id]
#
# Builds the production x86 lifecycle image (scripts/build-x86_64-c2c-lifecycle-ci.sh,
# isolated target dirs; it contains /bin/bench and /bin/bench-probe), boots it in
# QEMU q35 with exactly one explicit accelerator, types `bench` at the shell prompt
# and lets scripts/bench_results.py retain the raw serial log and the capture.
#
# Accelerator honesty: KVM usability is decided by the real launch. A requested
# KVM run whose QEMU fails to start, or that the collector does not observe
# running on KVM, is recorded as an INVALID `kvm_unavailable` capture. This
# script never retries under TCG and never relabels a capture.
#
# TCG captures are functional/report-only. Budgets come only from KVM captures
# compared against a matched baseline (same profile, accelerator and machine
# fingerprint) by scripts/compare-bench-results.sh.
#
# Environment:
#   X86_BENCH_RESULTS_DIR  capture directory (default: perf-results-x86)
#   X86_BENCH_SKIP_BUILD   1 = reuse the existing image instead of rebuilding
#   X86_KERNEL_FEATURES    extra kernel features (comma/space separated, e.g.
#                          bench-counters); passed to the builder and recorded in
#                          the capture, so instrumented captures form their own
#                          series. With X86_BENCH_SKIP_BUILD=1 it must describe the
#                          reused image; the collector rejects counter values that
#                          contradict it.
#   X86_BENCH_TIMEOUT      collector timeout in seconds (default: tcg 900, kvm 300)
#   X86_BENCH_REPETITION   capture repetition number (default: 1)
#   QEMU_X86               QEMU binary (default: qemu-system-x86_64)
#
# Exit status: 0 for a VALID capture, 1 for an INVALID one (including
# kvm_unavailable), 2 for usage/build errors.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

PROFILE_ID="x86_64-q35-2c-256m-v1"
ACCEL="${1:-tcg}"
case "$ACCEL" in
    tcg) CPU_MODEL="qemu64,+pdpe1gb"; DEFAULT_TIMEOUT=900 ;;
    kvm) CPU_MODEL="host"; DEFAULT_TIMEOUT=300 ;;
    *)
        echo "usage: $0 [tcg|kvm] [capture-id]" >&2
        exit 2
        ;;
esac
CAPTURE_ID="${2:-$(date -u +%Y%m%dT%H%M%SZ)-$PROFILE_ID-$ACCEL}"
RESULTS_DIR="${X86_BENCH_RESULTS_DIR:-perf-results-x86}"
TIMEOUT="${X86_BENCH_TIMEOUT:-$DEFAULT_TIMEOUT}"
REPETITION="${X86_BENCH_REPETITION:-1}"
QEMU_BIN="${QEMU_X86:-qemu-system-x86_64}"
export X86_KERNEL_FEATURES="${X86_KERNEL_FEATURES:-}"
FEATURE_ARGS=(--feature release-default)
for feature in ${X86_KERNEL_FEATURES//,/ }; do
    FEATURE_ARGS+=(--feature "$feature")
done

REL="target/x86-c2c-lifecycle/x86_64-unknown-none/release"
KERNEL="$REL/cellos-kernel"
RAMDISK="target/x86-c2c-lifecycle-embedded/kernel_fs.img"
ISO="build/vicell-x86-c2c-lifecycle.iso"

if [[ "${X86_BENCH_SKIP_BUILD:-0}" != 1 ]]; then
    echo "==> Building the x86 lifecycle image (bench, bench-probe, bench-peer-t2; kernel features: ${X86_KERNEL_FEATURES:-none})"
    C2C_WORKLOAD=lifecycle bash scripts/build-x86_64-c2c-lifecycle-ci.sh
fi
for artifact in "$KERNEL" "$REL/bench" "$REL/bench-probe" "$REL/bench-peer-t2" "$RAMDISK" "$ISO"; do
    if [[ ! -s "$artifact" ]]; then
        echo "FAIL: missing build artifact $artifact (unset X86_BENCH_SKIP_BUILD to build it)" >&2
        exit 2
    fi
done

TOOLCHAIN="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
REPOSITORY="${GITHUB_REPOSITORY:-$(git remote get-url origin 2>/dev/null || echo local)}"
SOURCE_REF="${GITHUB_REF:-$(git symbolic-ref -q HEAD || echo detached)}"
COMMIT="${GITHUB_SHA:-$(git rev-parse HEAD)}"
if [[ -z "${GITHUB_SHA:-}" && -n "$(git status --porcelain --untracked-files=no)" ]]; then
    COMMIT="$COMMIT+dirty"
fi

echo "==> Collecting $CAPTURE_ID ($PROFILE_ID, accelerator=$ACCEL) into $RESULTS_DIR"
python3 scripts/bench_results.py collect \
    --results-dir "$RESULTS_DIR" \
    --capture-id "$CAPTURE_ID" \
    --profile "$PROFILE_ID" \
    --repetition "$REPETITION" \
    --repository "$REPOSITORY" \
    --source-ref "$SOURCE_REF" \
    --commit "$COMMIT" \
    --toolchain "$TOOLCHAIN" \
    "${FEATURE_ARGS[@]}" \
    --source-input Cargo.lock=Cargo.lock \
    --source-input rust-toolchain.toml=rust-toolchain.toml \
    --artifact kernel="$KERNEL" \
    --artifact bench="$REL/bench" \
    --artifact probe="$REL/bench-probe" \
    --artifact peer="$REL/bench-peer-t2" \
    --artifact ramdisk="$RAMDISK" \
    --artifact iso="$ISO" \
    --stdin-line bench \
    --stdin-after "Cellos >" \
    --timeout-seconds "$TIMEOUT" \
    -- \
    "$QEMU_BIN" \
        -machine q35 \
        -accel "$ACCEL" \
        -cpu "$CPU_MODEL" \
        -smp 2 \
        -m 256M \
        -nographic \
        -no-reboot \
        -boot d \
        -cdrom "$ISO"

CAPTURE="$RESULTS_DIR/perf-$CAPTURE_ID.json"
VALIDITY="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["validity"]["verdict"])' "$CAPTURE")"
echo "X86_BENCH_CAPTURE=$CAPTURE"
echo "X86_BENCH_CAPTURE_ID=$CAPTURE_ID"
echo "X86_BENCH_VALIDITY=$VALIDITY"
[[ "$VALIDITY" == VALID ]]
