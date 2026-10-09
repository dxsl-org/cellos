#!/usr/bin/env bash
# Boot the x86_64 Tier-2 domain-entry test image in QEMU q35 and assert the
# phase-02 x86 witnesses:
#
#   * the admission posture reopened for `x86_64 && test-hooks`, with the closed
#     posture still denying and the publication path still refusing;
#   * a real Tier-2 cell admitted to a private root and run to completion
#     (`[domain] admitted cell … (CR3 isolation)`);
#   * the **live CR3** read from inside the domain's own kernel context, with its
#     root and PCID/tag bits;
#   * a deliberate fault from the second domain cell contained — exactly one
#     `[fault] Cell` line, the announced NULL store, the boot surviving it;
#   * teardown with frames released and nothing quarantined;
#   * shell recovery after the fault: the image's shell is interactive, so a
#     command typed *after* the containment line must be answered with a new
#     prompt.
#
# PCID is a machine property, not a lane property: TCG cannot emulate it (QEMU
# clears the feature), so the default run exercises the untagged-CR3 path and
# asserts `pcid_usable=false`; `X86_ACCEL=kvm X86_CPU_MODEL=host` runs the same
# image with tags live and asserts `pcid_usable=true` plus a non-zero tag in the
# DOMAIN-LIVE marker. Both are real observations of the kernel's own decision.
#
# Usage: bash scripts/x86/qemu-domain-test.sh [kernel-elf] [iso-out]
# Env: BOOT_WINDOW (default 120), X86_ACCEL, X86_CPU_MODEL, X86_EXPECT_PCID,
#      LOG_DIR (default: repository root)
#
# Requires the image from scripts/build-x86_64-domain-test-ci.sh.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

KERNEL="${1:-target/x86-domain-test/x86_64-unknown-none/release/cellos-kernel-domain-test}"
ISO="${2:-build/vicell-x86-domain-test.iso}"
ISO_ROOT="build/x86-domain-test-iso-root"
BOOT_WINDOW="${BOOT_WINDOW:-120}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT}"
X86_CPU_MODEL="${X86_CPU_MODEL:-qemu64,+pdpe1gb}"
X86_ACCEL="${X86_ACCEL:-}"
X86_EXPECT_PCID="${X86_EXPECT_PCID:-}"

# KVM runs the host CPU model by default, which is the only way PCID is real.
if [[ "$X86_ACCEL" == "kvm" && -z "${X86_CPU_MODEL_OVERRIDDEN:-}" ]]; then
    X86_CPU_MODEL="${X86_CPU_MODEL_HOST:-host}"
fi
if [[ -z "$X86_EXPECT_PCID" ]]; then
    if [[ "$X86_ACCEL" == "kvm" ]]; then X86_EXPECT_PCID=1; else X86_EXPECT_PCID=0; fi
fi

case "$X86_ACCEL" in
    ""|kvm|tcg) ;;
    *) echo "FAIL: X86_ACCEL must be empty, 'kvm' or 'tcg'" >&2; exit 1 ;;
esac
case "$X86_EXPECT_PCID" in
    0|1) ;;
    *) echo "FAIL: X86_EXPECT_PCID must be 0 or 1" >&2; exit 1 ;;
esac

if [[ ! -f "$KERNEL" ]]; then
    echo "FAIL: domain-test kernel not found: $KERNEL" >&2
    echo "  Build it with: bash scripts/build-x86_64-domain-test-ci.sh" >&2
    exit 1
fi
if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
    echo "FAIL: qemu-system-x86_64 not found" >&2; exit 1
fi

LABEL="x86-domain-test${X86_ACCEL:+-$X86_ACCEL}"
RAW_LOG="$LOG_DIR/qemu-$LABEL.raw.log"
LOG="$LOG_DIR/qemu-$LABEL.log"
FIFO="$(mktemp -u "${TMPDIR:-/tmp}/cellos-x86-domain-XXXXXX.fifo")"
mkfifo -m 600 "$FIFO"
# shellcheck disable=SC2064
trap "rm -f '$FIFO'" EXIT

echo "[x86-domain-test] Building ISO from $KERNEL"
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" bash scripts/x86/make-iso-ci.sh "$ISO" >/dev/null

ACCEL_ARGS=()
if [[ -n "$X86_ACCEL" ]]; then
    ACCEL_ARGS=(-accel "$X86_ACCEL")
fi

echo "[x86-domain-test] Booting ISO=$ISO (window=${BOOT_WINDOW}s, cpu=$X86_CPU_MODEL, pcid_expected=$X86_EXPECT_PCID)"

# The shell's stdin is a FIFO so a command can be typed *after* the containment
# line: the watcher below polls the raw log for the announced fault and only then
# sends `ps`, which makes the recovery witness an ordering proof rather than a
# race against the boot's timing.
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    < "$FIFO" > "$RAW_LOG" 2>&1 &
QEMU_PID=$!
exec 9> "$FIFO"

(
    typed=0
    for _ in $(seq 1 $((BOOT_WINDOW * 4))); do
        if grep -qa "deliberately writing to NULL" "$RAW_LOG" 2>/dev/null; then
            printf 'ps\r' >&9
            typed=1
            break
        fi
        sleep 0.25
    done
    exit $((1 - typed))
) &
WATCHER_PID=$!

QEMU_EXIT=0
wait "$QEMU_PID" || QEMU_EXIT=$?
kill "$WATCHER_PID" 2>/dev/null || true
wait "$WATCHER_PID" 2>/dev/null || true
exec 9>&-

# Strip NULs and ANSI escape sequences so patterns match cleanly.
tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"

if [[ "$QEMU_EXIT" -ne 0 && "$QEMU_EXIT" -ne 124 ]]; then
    echo "FAIL: QEMU exited $QEMU_EXIT (124 = boot window elapsed, the expected outcome)"
    tail -40 "$LOG" >&2
    exit 1
fi

if grep -qia "KERNEL PANIC\|panicked" "$LOG"; then
    echo "FAIL: kernel panic in $LOG" >&2
    tail -40 "$LOG" >&2
    exit 1
fi

# --- Fault containment -------------------------------------------------------
# The image is expected to contain exactly one fault: the announced NULL store
# from `tier2-exploit`. A second fault, a different address, or a fault that the
# launcher never announced all fail the lane.
FAULT_COUNT=$(grep -ca "\[fault\] Cell" "$LOG" || true)
if [[ "$FAULT_COUNT" -ne 1 ]]; then
    echo "FAIL: expected exactly 1 contained cell fault, saw $FAULT_COUNT" >&2
    grep -a "\[fault\] Cell" "$LOG" >&2 || true
    tail -40 "$LOG" >&2
    exit 1
fi
if ! grep -qa "\[fault\] Cell .* terminated: cause=.* addr=0x0" "$LOG"; then
    echo "FAIL: the single contained fault is not the announced NULL store (addr=0x0)" >&2
    grep -a "\[fault\] Cell" "$LOG" >&2
    exit 1
fi

# --- Shell recovery ---------------------------------------------------------
# The image has an interactive window, so recovery is witnessed rather than
# inferred: the watcher typed `ps` only after the containment line, and that
# command must have been answered with a second prompt.
FAULT_LINE=$(grep -na "\[fault\] Cell" "$LOG" | head -1 | cut -d: -f1)
PROMPT_LINES=$(grep -na "Cellos > " "$LOG" | cut -d: -f1 || true)
PROMPT_AFTER=$(printf '%s\n' "$PROMPT_LINES" | awk -v f="$FAULT_LINE" '$1 > f' | wc -l | tr -d ' ')
if [[ -z "$PROMPT_LINES" ]]; then
    echo "FAIL: 'Cellos >' prompt never appeared" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
if [[ "$PROMPT_AFTER" -lt 2 ]]; then
    echo "FAIL: shell did not recover after the contained fault (prompts after the fault: $PROMPT_AFTER)" >&2
    tail -40 "$LOG" >&2
    exit 1
fi

# --- PCID decision ----------------------------------------------------------
DECISION=$(grep -a "x86_64 paging: PCID" "$LOG" | head -1 || true)
if [[ -z "$DECISION" ]]; then
    echo "FAIL: kernel never emitted its PCID decision" >&2
    exit 1
fi
if [[ "$X86_EXPECT_PCID" == "1" ]]; then
    if [[ "$DECISION" != *"PCID enabled"* || "$DECISION" != *"CR4.PCIDE=1"* ]]; then
        echo "FAIL: expected PCID enabled on this CPU, kernel decided: $DECISION" >&2
        exit 1
    fi
else
    if [[ "$DECISION" != *"PCID disabled"* ]]; then
        echo "FAIL: expected PCID disabled on this CPU, kernel decided: $DECISION" >&2
        exit 1
    fi
fi

LIVE=$(grep -a "S22-X86-DOMAIN-LIVE: PASS" "$LOG" | head -1 || true)
if [[ -z "$LIVE" ]]; then
    echo "FAIL: S22-X86-DOMAIN-LIVE: PASS absent" >&2
    grep -a "DOMAIN-LIVE" "$LOG" >&2 || true
    exit 1
fi
if [[ "$X86_EXPECT_PCID" == "1" ]]; then
    if [[ "$LIVE" != *"pcid_usable=true"* ]]; then
        echo "FAIL: live CR3 read did not report pcid_usable=true: $LIVE" >&2
        exit 1
    fi
    LIVE_PCID=$(printf '%s\n' "$LIVE" | sed -n 's/.* pcid=\([0-9]*\) .*/\1/p')
    if [[ -z "$LIVE_PCID" || "$LIVE_PCID" == "0" ]]; then
        echo "FAIL: PCID is usable but the live domain CR3 carries tag 0: $LIVE" >&2
        exit 1
    fi
else
    if [[ "$LIVE" != *"pcid_usable=false"* ]]; then
        echo "FAIL: live CR3 read did not report pcid_usable=false on TCG: $LIVE" >&2
        exit 1
    fi
fi

# --- Required markers -------------------------------------------------------
REQUIRED_MARKERS=(
    "Tier 2 admission: ENABLED (development profile):::Tier 2 admission: ENABLED (development profile)"
    "ADMISSION-ENABLED:::S22-X86-ADMISSION-ENABLED: PASS"
    "ADMISSION-DENY:::S22-X86-ADMISSION-DENY: PASS"
    "ADMISSION-DRAIN:::S22-X86-ADMISSION-DRAIN: PASS"
    "ADMISSION-PUBLICATION-DENY:::S22-X86-ADMISSION-PUBLICATION-DENY: PASS"
    "ADMISSION-CEILING:::S22-X86-ADMISSION-CEILING: PASS"
    "SAS fastpath:::S22-X86-SAS-FASTPATH: PASS"
    "private-root plan:::S22-X86-PLAN: PASS"
    "same-domain resume:::S22-X86-RESUME-ROOT: PASS"
    "pin-dying window:::S22-X86-PIN-DYING: PASS"
    "tier2-smoke launch:::Init: tier2-smoke admitted."
    "tier2-exploit launch:::Init: tier2-exploit admitted."
    "cpp-smoke launch:::Init: cpp-smoke admitted."
    "cpp-smoke static ctor:::[cpp-smoke] static-ctor marker=0xC0FFEE11"
    "cpp-smoke virtual dispatch:::[cpp-smoke] virtual-dispatch area=37"
    "cpp-smoke virtual delete:::[cpp-smoke] virtual-delete area=36"
    "cpp-smoke templates:::[cpp-smoke] templates total=64"
    "cpp-smoke heap churn:::[cpp-smoke] heap churn checksum="
    "cpp-smoke vfs roundtrip:::[cpp-smoke] vfs client roundtrip bytes="
    "cpp-smoke c-abi read:::[cpp-smoke] c-abi read magic=ELF"
    "cpp-smoke complete:::CPP-SMOKE: PASS"
    "cross-tier provider launch:::Init: tier2-rpc-provider admitted."
    "cross-tier provider registration:::Init: tier2-rpc-provider registered."
    "cross-tier driver launch:::Init: tier2-rpc-driver launched."
    "cross-tier named binding:::[tier2-rpc] PROVIDER-BINDING tid="
    "cross-tier binding agreement:::[tier2-rpc] PROVIDER-NAME-MATCHES-RAW=true"
    "cross-tier Tier-1 to Tier-2:::[tier2-rpc] TIER1-TO-TIER2=OK"
    "cross-tier Tier-2 to Tier-1:::[tier2-rpc] TIER2-TO-TIER1=OK root_is_dir="
    "cross-tier oversize refusal:::[tier2-rpc] OVERSIZE=REFUSED"
    "cross-tier invalid buffer refusal:::[tier2-rpc] INVALID-BUFFER=REFUSED"
    "cross-tier unauthorized method:::[tier2-rpc] UNAUTHORIZED-METHOD=REFUSED"
    "cross-tier provider gone:::[tier2-rpc] PROVIDER-GONE=none"
    "cross-tier stale descriptor:::[tier2-rpc] STALE-BINDING=REFUSED"
    "cross-tier stale cleared:::[tier2-rpc] STALE-BINDING-CLEARED=true"
    "cross-tier driver done:::[tier2-rpc] DRIVER-DONE"
    "domain admission:::Tier 2 Paged Domain (CR3 isolation)"
    "domain cell ran:::S22-X86-DOMAIN-LIVE: PASS"
    "domain cell completed:::[tier2-smoke] PASS: All Tier 2 runtime invariants verified successfully!"
    "announced fault:::[tier2-exploit] deliberately writing to NULL (0x0) — expect Page Fault termination"
    "frame release:::[selftest] DOMAIN-FRAME-RELEASE: PASS"
    "domain teardown:::S22-X86-DOMAIN-TEARDOWN: PASS releases="
    "interactive shell:::Cellos > "
)

bash scripts/assert-boot-markers.sh "$LOG" "$LABEL" "${REQUIRED_MARKERS[@]}"

# --- Cross-tier exchange negatives -------------------------------------------
# The fixture's own failure marker must be absent, and no leg may report that a
# refusal was accepted: those are exactly the outcomes the exchange exists to rule
# out, so their absence is asserted rather than assumed. `grep -Fqa` because the
# markers contain characters the log's own ANSI stripping must not be re-interpreted.
if grep -Fqa "[tier2-rpc] FAIL" "$LOG"; then
    echo "FAIL: the cross-tier fixture reported a failure:" >&2
    grep -a "\[tier2-rpc\]" "$LOG" >&2
    exit 1
fi
for rejected in "OVERSIZE=ACCEPTED" "INVALID-BUFFER=ACCEPTED" "UNAUTHORIZED-METHOD=ACCEPTED" "STALE-BINDING=SERVED" "STALE-BINDING=ERROR"; do
    if grep -Fqa "$rejected" "$LOG"; then
        echo "FAIL: the cross-tier exchange reported $rejected" >&2
        grep -a "\[tier2-rpc\]" "$LOG" >&2
        exit 1
    fi
done

echo "PCID decision: $DECISION"
echo "Domain live:   $LIVE"
grep -a "S22-X86-DOMAIN-TEARDOWN\|\[selftest\] DOMAIN-FRAME-RELEASE\|\[domain\] admitted cell" "$LOG" || true
echo "PASS: x86_64 Tier-2 domain-entry lane ($LABEL) — every marker present"
