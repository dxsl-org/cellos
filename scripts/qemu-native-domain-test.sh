#!/usr/bin/env bash
# Run RV64 native-domain test hooks in a fresh, isolated QEMU guest. This is a
# test-only assertion runner: it never routes native-domains into a production
# image or makes a qualification/ledger claim.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

HARTS=""
CASES=""
BOOT_WINDOW="${BOOT_WINDOW:-55}"
QEMU="${VICELL_QEMU:-qemu-system-riscv64}"
LOG_ROOT="${NATIVE_DOMAIN_QEMU_LOG_DIR:-$ROOT/.logs/native-domain-qemu}"
KERNEL="target/riscv64gc-unknown-none-elf/release/cellos-kernel-native-domain-test"

usage() {
    cat <<'USAGE'
Usage: scripts/qemu-native-domain-test.sh --harts {1|2} --case <csv>

Cases: switch, sas-fastpath, migration

Each requested case gets a separate fresh QEMU log directory. `migration`
requires two harts; it asserts the domain-switch terminal from the cross-hart
fixture rather than relabeling a one-hart result as migration evidence.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --harts)
            [[ $# -ge 2 ]] || { echo "FAIL: --harts requires 1 or 2" >&2; exit 2; }
            HARTS="$2"
            shift 2
            ;;
        --case)
            [[ $# -ge 2 ]] || { echo "FAIL: --case requires a CSV value" >&2; exit 2; }
            CASES="$2"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "FAIL: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

[[ "$HARTS" == "1" || "$HARTS" == "2" ]] || { echo "FAIL: --harts must be 1 or 2" >&2; exit 2; }
[[ -n "$CASES" ]] || { echo "FAIL: --case is required" >&2; exit 2; }
command -v "$QEMU" >/dev/null 2>&1 || { echo "FAIL: $QEMU not found on PATH" >&2; exit 1; }

IFS=',' read -r -a REQUESTED_CASES <<< "$CASES"
[[ ${#REQUESTED_CASES[@]} -gt 0 ]] || { echo "FAIL: no cases requested" >&2; exit 2; }
declare -A seen=()
for case_id in "${REQUESTED_CASES[@]}"; do
    [[ -n "$case_id" ]] || { echo "FAIL: empty case in --case" >&2; exit 2; }
    case "$case_id" in
        switch|sas-fastpath|migration) ;;
        *) echo "FAIL: unknown native-domain case: $case_id" >&2; exit 2 ;;
    esac
    [[ -z "${seen[$case_id]:-}" ]] || { echo "FAIL: duplicate native-domain case: $case_id" >&2; exit 2; }
    seen[$case_id]=1
    if [[ "$case_id" == "migration" && "$HARTS" != "2" ]]; then
        echo "FAIL: migration requires --harts 2" >&2
        exit 2
    fi
done

# The artifact is rebuilt for every invocation so a prior feature-off kernel or
# an earlier domain run cannot satisfy this runner's markers.
bash scripts/build-native-domain-test-ci.sh
[[ -f "$KERNEL" ]] || { echo "FAIL: fresh native-domain test kernel missing: $KERNEL" >&2; exit 1; }

mkdir -p "$LOG_ROOT"
QEMU_VERSION="$($QEMU --version | sed -n '1p')"
ELF_DIGEST="$(sha256sum "$KERNEL" | awk '{print $1}')"

marker_for() {
    case "$1" in
        switch) printf 'S22-RV64-SWITCH: PASS harts=%s' "$HARTS" ;;
        migration) printf 'S22-RV64-MIGRATION: PASS harts=2' ;;
        sas-fastpath) printf 'S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0 harts=%s' "$HARTS" ;;
    esac
}
terminal_pattern_for() {
    case "$1" in
        switch) printf '(^|\\] )S22-RV64-SWITCH: PASS harts=%s$' "$HARTS" ;;
        migration) printf '(^|\\] )S22-RV64-MIGRATION: PASS harts=2$' ;;
        sas-fastpath) printf '(^|\\] )S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0 harts=%s$' "$HARTS" ;;
    esac
}
assert_runtime_hart_count() {
    if [[ "$HARTS" == "2" ]]; then
        grep -Fq '[smp] hart 1 online, parked' "$normalized_log" || {
            echo "FAIL: requested two-hart QEMU run did not bring hart 1 online; see $normalized_log" >&2
            exit 1
        }
    elif grep -Fq '[smp] hart 1 online, parked' "$normalized_log"; then
        echo "FAIL: one-hart QEMU run unexpectedly brought hart 1 online; see $normalized_log" >&2
        exit 1
    fi
}
# SWITCH logs one line per genuine Activate transition (domain_switch.rs
# root_switch), so multi-stage fixtures emit it several times per boot; its
# gate is at-least-one. Other markers are boot-terminal aggregates and stay
# exactly-one (min 0 = exact).
terminal_min_for() {
    case "$1" in
        switch) echo 1 ;;
        *)      echo 0 ;;
    esac
}
 
 for case_id in "${REQUESTED_CASES[@]}"; do
    marker="$(marker_for "$case_id")"
    terminal_pattern="$(terminal_pattern_for "$case_id")"

    case_dir="$(mktemp -d "$LOG_ROOT/h${HARTS}-${case_id}-XXXXXX")"
    raw_log="$case_dir/qemu.raw.log"
    normalized_log="$case_dir/qemu.log"
    metadata="$case_dir/run.env"
    qemu_args=(
        -machine virt
        -m 256M
        -nographic
        -bios default
        -smp "$HARTS"
        -kernel "$KERNEL"
    )

    {
        printf 'environment=qemu\narchitecture=riscv64\nhart_count=%s\nhost_vmm=QEMU TCG\n' "$HARTS"
        printf 'feature_tuple=native-domains,test-hooks\nfirmware=default\nqemu_version=%s\nelf_sha256=%s\n' "$QEMU_VERSION" "$ELF_DIGEST"
        printf 'command='
        printf '%q ' "$QEMU" "${qemu_args[@]}"
        printf '\ncase=%s\nexpected_marker=%s\n' "$case_id" "$marker"
    } > "$metadata"

    echo "[qemu-native-domain-test] case=$case_id harts=$HARTS log_dir=$case_dir"
    qemu_status=0
    timeout "$BOOT_WINDOW" "$QEMU" "${qemu_args[@]}" < /dev/null > "$raw_log" 2>&1 || qemu_status=$?
    tr -d '\000\r' < "$raw_log" | sed 's/\x1b\[[0-9;]*m//g' > "$normalized_log"

    # A timeout is the normal post-self-test completion path. Any other QEMU
    # process error is distinct from a guest assertion and fails immediately.
    if [[ "$qemu_status" -ne 0 && "$qemu_status" -ne 124 ]]; then
        echo "FAIL: QEMU exited $qemu_status for case=$case_id; see $raw_log" >&2
        exit 1
    fi
    if grep -Eqi 'KERNEL PANIC|S22-RV64-[A-Z0-9-]+: FAIL' "$normalized_log"; then
        echo "FAIL: native-domain failure terminal for case=$case_id; see $normalized_log" >&2
        exit 1
    fi
    while IFS= read -r fault_line; do
        [[ -z "$fault_line" ]] && continue
        if [[ ! "$fault_line" =~ ^\[ERROR\]\ \[fault\]\ Cell\ 254\ \(task\ [0-9]+\ generation\ [0-9]+\)\ terminated:\ cause=0xf\ pc=0x[0-9a-f]+\ addr=0x[0-9a-f]+$ ]]; then
            echo "FAIL: unclassified cell fault for case=$case_id: $fault_line; see $normalized_log" >&2
            exit 1
        fi
    done < <(grep -F '[fault] Cell' "$normalized_log" || true)
    assert_runtime_hart_count
    terminal_count="$(grep -Ec "$terminal_pattern" "$normalized_log" || true)"
    terminal_min="$(terminal_min_for "$case_id")"
    if [[ "$terminal_min" -gt 0 ]]; then
        if [[ "$terminal_count" -lt "$terminal_min" ]]; then
            echo "FAIL: expected at least $terminal_min terminal for case=$case_id: $marker; found $terminal_count; see $normalized_log" >&2
            exit 1
        fi
    elif [[ "$terminal_count" != "1" ]]; then
        echo "FAIL: expected exactly one terminal for case=$case_id: $marker; found $terminal_count; see $normalized_log" >&2
        exit 1
    fi

    printf 'PASS: native-domain case=%s harts=%s terminal=%s\n' "$case_id" "$HARTS" "$marker"
done

printf 'S22-RV64-QEMU-SUITE: PASS HARTS=%s CASES=%s\n' "$HARTS" "$CASES"
