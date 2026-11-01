#!/usr/bin/env bash
# Phase 04 entry-safety cost: A/B of the x86 bench suite on one machine.
#
# Usage:
#   scripts/x86/run-bench-p04-ab.sh [kvm|tcg] [repetitions]   (default: kvm 3)
#
# Candidate = this checkout. Baseline = the same commit in a detached worktree
# with scripts/x86/p04-entry-baseline.patch applied, which reverts the two
# Phase 04 changes that run on every entry of a default (no SMEP/SMAP/UMIP)
# kernel: the IDT-entry `ViCell_smap_active` test and the wider SYSCALL FMASK.
# The guarded IST stacks are not reverted: only NMI/#DF/#MC use them, and the
# suite never raises those, so they cost nothing on any measured path.
#
# Captures go to X86_BENCH_RESULTS_DIR (default perf-results-p04-ab), one
# compatibility group, then scripts/bench_ab.py prints per-observation medians.
# The result is report-only; on a machine with unknown governor/timer it is
# printed UNQUALIFIED and never becomes a budget.
#
# Exit status: 0 summary printed, 1 a capture was INVALID or incomparable,
# 2 usage/build errors.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

ACCEL="${1:-kvm}"
REPS="${2:-3}"
[[ "$ACCEL" == kvm || "$ACCEL" == tcg ]] || { echo "usage: $0 [kvm|tcg] [repetitions]" >&2; exit 2; }
[[ "$REPS" =~ ^[1-9][0-9]*$ ]] || { echo "repetitions must be a positive integer" >&2; exit 2; }

RESULTS_DIR="$(realpath -m "${X86_BENCH_RESULTS_DIR:-perf-results-p04-ab}")"
WORKTREE="$REPO_ROOT/build/p04-entry-baseline-wt"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$RESULTS_DIR"

if [[ -e "$WORKTREE" ]]; then
    git worktree remove --force "$WORKTREE"
fi
git worktree add --detach "$WORKTREE" HEAD >/dev/null
trap 'git -C "$REPO_ROOT" worktree remove --force "$WORKTREE" >/dev/null 2>&1 || true' EXIT
git -C "$WORKTREE" apply "$SCRIPT_DIR/p04-entry-baseline.patch"

capture() {
    local root="$1" label="$2" rep="$3" skip="$4"
    local id="$STAMP-p04-$label-$ACCEL-$rep"
    (cd "$root" && X86_BENCH_RESULTS_DIR="$RESULTS_DIR" X86_BENCH_SKIP_BUILD="$skip" \
        X86_BENCH_REPETITION="$rep" bash scripts/x86/run-bench-x86.sh "$ACCEL" "$id") >&2 || return 1
    echo "$id"
}

# Interleave baseline and candidate runs so slow host drift hits both sets.
BASELINE=()
CANDIDATE=()
for rep in $(seq 1 "$REPS"); do
    skip=1
    [[ "$rep" == 1 ]] && skip=0
    id="$(capture "$WORKTREE" baseline "$rep" "$skip")" || exit 1
    BASELINE+=("$id")
    id="$(capture "$REPO_ROOT" candidate "$rep" "$skip")" || exit 1
    CANDIDATE+=("$id")
done

python3 scripts/bench_ab.py "$RESULTS_DIR" --baseline "${BASELINE[@]}" --candidate "${CANDIDATE[@]}"
