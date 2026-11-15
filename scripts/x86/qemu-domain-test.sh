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
#   * a deliberate fault from the second domain cell contained — the announced
#     NULL store at `addr=0x0`, exactly once, the boot surviving it;
#   * teardown with frames released and nothing quarantined;
#   * shell recovery after the fault: the image's shell is interactive, so a
#     `ps` typed *after* the containment line must be echoed and then answered
#     with a new prompt;
#   * the Phase-15 domain grant suite: the kernel lifecycle and capability
#     fixtures (`S22-X86-GRANT-REVOKE`, `-GATE`), the deferred revoke
#     (`-DEFERRED`), the six negatives with their fault-injection controls
#     (`-NEG-<case>: PASS`, `-CONTROL-<case>: CAUGHT (<reason>)` with the
#     case's exact reason; a control that reports `FAIL` or `MISSED` fails the
#     lane), and the two-cell public pair (`S22-X86-GRANT-PAIR-*`) with its five
#     deliberate receiver faults, each classified to the grant address the owner
#     handed over and to its page-fault kind;
#   * at 2+ CPUs, the Phase-16 silent-CPU quarantine fixture
#     (`S22-X86-QUARANTINE-SILENT-CPU`): one teardown under a CPU held with
#     interrupts masked completes after one probe bound with every frame
#     quarantined, the reaper frees nothing while the CPU stays silent
#     (`CONTROL`), and frees everything once it answers (`PASS`).
#   * when the boot line says `smap=on` (the default model has SMAP), the
#     SMAP window (`[selftest] SMAP-WINDOW: PASS`): a Sas cell's `Log` copies
#     in through the user-copy window and a raw kernel read of the same user
#     byte outside it faults; and `GrantSlice`'s `size_out`
#     (`[selftest] SMAP-GRANT-SLICE: PASS`): a Sas thread's slice writes the
#     grant size to its user stack through the window.
#
# PCID is a machine property, not a lane property: TCG cannot emulate it (QEMU
# clears the feature), so the default run exercises the untagged-CR3 path and
# asserts `pcid_usable=false`; `X86_ACCEL=kvm X86_CPU_MODEL=host` runs the same
# image with tags live and asserts `pcid_usable=true` plus a non-zero tag in the
# DOMAIN-LIVE marker. Both are real observations of the kernel's own decision.
#
# Usage: bash scripts/x86/qemu-domain-test.sh [kernel-elf] [iso-out]
# Env: BOOT_WINDOW (default 120), X86_ACCEL (""|tcg|kvm), X86_CPUS (1..64),
#      X86_CPU_MODEL, X86_EXPECT_PCID, LOG_DIR (default: repository root).
#      The grant suite's evidence matrix is X86_CPUS ∈ {1,2,4} × X86_ACCEL ∈
#      {tcg,kvm}; CI runs TCG at one CPU.
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
X86_CPU_MODEL="${X86_CPU_MODEL:-qemu64,+pdpe1gb,+smep,+smap,+umip}"
X86_ACCEL="${X86_ACCEL:-}"
X86_EXPECT_PCID="${X86_EXPECT_PCID:-}"
X86_CPUS="${X86_CPUS:-1}"

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
# 1..64 (the x86 MAX_HARTS). Two or more CPUs require the `online=N` topology
# and the SMP fixtures below (plan 261103-2218 phase 07b).
if ! [[ "$X86_CPUS" =~ ^[1-9][0-9]*$ ]] || (( X86_CPUS > 64 )); then
    echo "FAIL: X86_CPUS must be a number from 1 to 64" >&2; exit 1
fi

if [[ ! -f "$KERNEL" ]]; then
    echo "FAIL: domain-test kernel not found: $KERNEL" >&2
    echo "  Build it with: bash scripts/build-x86_64-domain-test-ci.sh" >&2
    exit 1
fi
if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
    echo "FAIL: qemu-system-x86_64 not found" >&2; exit 1
fi

LABEL="x86-domain-test${X86_ACCEL:+-$X86_ACCEL}-cpu$X86_CPUS"
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
# line: the watcher below polls the raw log for the contained NULL store and only
# then sends `ps`, which makes the recovery witness an ordering proof rather than
# a race against the boot's timing.
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -smp "$X86_CPUS" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    < "$FIFO" > "$RAW_LOG" 2>&1 &
QEMU_PID=$!
exec 9> "$FIFO"

(
    # Type only once the NULL store is contained AND the serial session has
    # printed a prompt: the session is owned by the input service, and bytes
    # sent before it exists are dropped, not queued. The prompt may come before
    # or after the fault (it races the fixture cells), so it is looked for
    # anywhere. A `ps` the session dropped is never echoed, so it is typed again
    # until the echo follows the fault (at most three times).
    # `set -e` and `pipefail` hold in this subshell too: a probe that finds
    # nothing must not end the watcher.
    contained() { grep -naEm1 '\[fault\] Cell [0-9]+ .* terminated: .* addr=0x0' "$RAW_LOG" 2>/dev/null | cut -d: -f1 || true; }
    echoed_after() { tail -n +"$1" "$RAW_LOG" | tr -d '\r' | grep -qaE '^USER: ps[[:space:]]*$'; }
    typed=0
    for _ in $(seq 1 $((BOOT_WINDOW * 4))); do
        fault_at="$(contained)"
        if [[ -n "$fault_at" ]] && grep -qa "Cellos > " "$RAW_LOG"; then
            if (( typed > 0 )) && echoed_after "$fault_at"; then
                exit 0
            fi
            if (( typed < 3 )); then
                printf 'ps\r' >&9
                typed=$((typed + 1))
                for _ in $(seq 1 40); do
                    echoed_after "$fault_at" && exit 0
                    sleep 0.25
                done
                continue
            fi
        fi
        sleep 0.25
    done
    exit 1
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
# Every `[fault] Cell` line in the boot must be one of the deliberate, announced,
# address-classified faults, and nothing else:
#
#   * `tier2-exploit`'s NULL store — exactly one, at `addr=0x0`;
#   * the grant pair's five receiver generations — each ends in the store its
#     phase exists to witness, at the exact grant address the owner handed over
#     (`S22-X86-GRANT-PAIR-HANDOFF`): the freed grant (id1), the ReadOnly page
#     and then the same page after `GrantUnregister` (id2, twice), the address
#     the owner's exit revoked (id3), and the downgraded grant (id4).
#
# The total is therefore fixed at six; any other fault — a second NULL store,
# another cell, another address — fails the lane. The address attributes each
# fault to its phase, and the page-fault error code's P bit (bit 0, which the
# kernel reports as `cause`) says what the phase must have found: a revoked
# address (id1 after `GrantFree`, id2 after `GrantUnregister`, id3 after the
# owner's exit) is not present (P=0), while the ReadOnly page (id2's first
# fault) and the downgraded page (id4) are present without the write right
# (P=1). The unregister generation holds id2 ReadWrite, so a PTE that survived
# `GrantUnregister` would let its store land rather than fault.
FAULT_PATTERN='\[fault\] Cell [0-9]+ \(task [0-9]+ generation [0-9]+\) terminated: cause=0x[0-9a-f]+ pc=0x[0-9a-f]+ addr='
FAULT_LINES="$(grep -a "\[fault\] Cell" "$LOG" || true)"
FAULT_COUNT="$(printf '%s\n' "$FAULT_LINES" | grep -c . || true)"
faults_at() { grep -acE "${FAULT_PATTERN}$1\$" "$LOG" || true; }
# Byte offset of the Nth fault at one exact address (1-based); empty when absent.
fault_offset_at() { grep -aboE "${FAULT_PATTERN}$1\$" "$LOG" | sed -n "$2p" | cut -d: -f1 || true; }
# Reported cause (the x86 page-fault error code) of the Nth fault at one exact
# address (1-based); empty when absent.
fault_cause_at() { grep -aoE "${FAULT_PATTERN}$1\$" "$LOG" | sed -n "$2p" | sed -n 's/.* cause=\(0x[0-9a-f]*\) .*/\1/p' || true; }
offset_of() { grep -aboF -- "$1" "$LOG" | head -1 | cut -d: -f1 || true; }

if [[ "$(faults_at 0x0)" != "1" ]]; then
    echo "FAIL: expected exactly one contained NULL-store fault (addr=0x0), found $(faults_at 0x0):" >&2
    printf '%s\n' "$FAULT_LINES" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
HANDOFF_LINE="$(grep -aoE 'S22-X86-GRANT-PAIR-HANDOFF id1=[0-9]+ id2=[0-9]+ id3=[0-9]+ id4=[0-9]+' "$LOG" | tail -1 || true)"
if [[ -z "$HANDOFF_LINE" ]]; then
    echo "FAIL: the grant pair published no handoff line, so no receiver fault can be classified" >&2
    printf '%s\n' "$FAULT_LINES" >&2
    exit 1
fi
handoff_id() { printf '%s' "$HANDOFF_LINE" | sed -n "s/.*$1=\([0-9]\+\).*/\1/p"; }
A1="$(printf '0x%x' "$(handoff_id id1)")"
A2="$(printf '0x%x' "$(handoff_id id2)")"
A3="$(printf '0x%x' "$(handoff_id id3)")"
A4="$(printf '0x%x' "$(handoff_id id4)")"
if [[ "$(faults_at "$A1")" != "1" || "$(faults_at "$A2")" != "2" \
    || "$(faults_at "$A3")" != "1" || "$(faults_at "$A4")" != "1" ]]; then
    echo "FAIL: the pair's deliberate fault counts are wrong (want id1:$A1=1 id2:$A2=2 id3:$A3=1 id4:$A4=1," >&2
    echo "      found id1=$(faults_at "$A1") id2=$(faults_at "$A2") id3=$(faults_at "$A3") id4=$(faults_at "$A4")):" >&2
    printf '%s\n' "$FAULT_LINES" >&2
    exit 1
fi
if [[ "$FAULT_COUNT" != "6" ]]; then
    echo "FAIL: expected exactly 6 accounted cell faults (the NULL store plus the pair's five), found $FAULT_COUNT:" >&2
    printf '%s\n' "$FAULT_LINES" >&2
    exit 1
fi
# Each fault must follow the announcement that names it, so no fault can be
# attributed to a phase whose store had not been announced yet; byte offsets
# make the ordering a property of the boot, not of the grep.
assert_announced_before() {
    local announcement="$1" address="$2" nth="$3" description="$4"
    local announced_at faulted_at
    announced_at="$(offset_of "$announcement")"
    faulted_at="$(fault_offset_at "$address" "$nth")"
    if [[ -z "$announced_at" || -z "$faulted_at" || "$announced_at" -ge "$faulted_at" ]]; then
        echo "FAIL: fault '$description' at $address is not preceded by its announcement (announced=$announced_at faulted=$faulted_at)" >&2
        exit 1
    fi
}
assert_announced_before "[tier2-exploit] deliberately writing to NULL (0x0)" 0x0 1 "NULL store"
assert_announced_before "S22-X86-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED" "$A1" 1 "GrantFree revoke"
assert_announced_before "S22-X86-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED" "$A2" 1 "ReadOnly write"
assert_announced_before "S22-X86-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED" "$A2" 2 "GrantUnregister"
assert_announced_before "S22-X86-GRANT-PAIR-RECEIVER-DOWNGRADE-WRITE: FAULT-EXPECTED" "$A4" 1 "same-recipient downgrade"
assert_announced_before "S22-X86-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED" "$A3" 1 "owner exit"
# present <0|1>: the P bit the Nth fault at an address must carry.
assert_fault_kind() {
    local address="$1" nth="$2" present="$3" description="$4"
    local cause
    cause="$(fault_cause_at "$address" "$nth")"
    if [[ -z "$cause" ]] || (( (cause & 1) != present )); then
        echo "FAIL: fault '$description' at $address has cause=${cause:-none}; want P=$present ($( (( present )) && echo protection || echo not-present))" >&2
        printf '%s\n' "$FAULT_LINES" >&2
        exit 1
    fi
}
assert_fault_kind "$A1" 1 0 "GrantFree revoke"
assert_fault_kind "$A2" 1 1 "ReadOnly write"
assert_fault_kind "$A2" 2 0 "GrantUnregister"
assert_fault_kind "$A4" 1 1 "same-recipient downgrade"
assert_fault_kind "$A3" 1 0 "owner exit"

# --- Shell recovery ---------------------------------------------------------
# The image has an interactive window, so recovery is witnessed rather than
# inferred: the watcher typed `ps` only after the NULL store was contained, the
# shell must have echoed it after that line, and a prompt must follow the echo
# (the command was answered). A prompt alone proves nothing: the session can
# print its first prompt before or after the fault.
FAULT_LINE=$(grep -naE "${FAULT_PATTERN}0x0\$" "$LOG" | head -1 | cut -d: -f1 || true)
if ! grep -qa "Cellos > " "$LOG"; then
    echo "FAIL: 'Cellos >' prompt never appeared" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
PS_ECHO_LINE=$(grep -naE '^USER: ps[[:space:]]*$' "$LOG" | cut -d: -f1 | awk -v f="$FAULT_LINE" '$1 > f { print; exit }' || true)
if [[ -z "$PS_ECHO_LINE" ]]; then
    echo "FAIL: the shell never echoed the 'ps' typed after the contained fault (line $FAULT_LINE)" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
PROMPT_AFTER=$(grep -na "Cellos > " "$LOG" | cut -d: -f1 | awk -v e="$PS_ECHO_LINE" '$1 > e' | wc -l | tr -d ' ')
if [[ "$PROMPT_AFTER" -lt 1 ]]; then
    echo "FAIL: shell did not recover after the contained fault (no prompt after the echoed 'ps' at line $PS_ECHO_LINE)" >&2
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
    "MMIO revoke preserves supervisor binding:::MMIO-REVOKE-USERBIT: PASS"
    "vfs-lifetime and grant quarantine:::vfs-lifetime self-test PASS (exact lease, quarantine, owner watch)"
    "tier2-smoke launch:::fixture-launcher: tier2-smoke admitted."
    "tier2-exploit launch:::fixture-launcher: tier2-exploit admitted."
    "cpp-smoke launch:::fixture-launcher: cpp-smoke admitted."
    "cpp-smoke static ctor:::[cpp-smoke] static-ctor marker=0xC0FFEE11"
    "cpp-smoke virtual dispatch:::[cpp-smoke] virtual-dispatch area=37"
    "cpp-smoke virtual delete:::[cpp-smoke] virtual-delete area=36"
    "cpp-smoke templates:::[cpp-smoke] templates total=64"
    "cpp-smoke heap churn:::[cpp-smoke] heap churn checksum="
    "cpp-smoke vfs roundtrip:::[cpp-smoke] vfs client roundtrip bytes="
    "cpp-smoke c-abi read:::[cpp-smoke] c-abi read magic=ELF"
    "cpp-smoke complete:::CPP-SMOKE: PASS"
    "cross-tier provider launch:::fixture-launcher: tier2-rpc-provider admitted."
    "cross-tier provider registration:::fixture-launcher: tier2-rpc-provider registered."
    "cross-tier driver launch:::fixture-launcher: tier2-rpc-driver launched."
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
    "interrupt allocations charge the kernel:::[selftest] IRQ-ALLOC-CHARGE: PASS"
    "user stacks map from the stack arena:::[selftest] STACK-ARENA: PASS"
    "user stack off identity:::stack-probe self-test PASS (two guards, overflow target unmapped, watermark, user stack off identity)"
    "domain teardown:::S22-X86-DOMAIN-TEARDOWN: PASS releases="
    "grant lifecycle fixture:::S22-X86-GRANT-REVOKE: PASS"
    "grant capability gate:::S22-X86-GRANT-GATE: PASS"
    "grant deferred revoke:::S22-X86-GRANT-DEFERRED: PASS"
    "grant unbound owner:::S22-X86-GRANT-UNBOUND-OWNER: PASS"
    "negative revoke while mapped:::S22-X86-GRANT-NEG-REVOKE-MAPPED: PASS"
    "negative receiver death:::S22-X86-GRANT-NEG-RECEIVER-DEATH: PASS"
    "negative stale grant id:::S22-X86-GRANT-NEG-STALE-ID: PASS"
    "negative cross-domain forgery:::S22-X86-GRANT-NEG-FORGERY: PASS"
    "negative fault during copy:::S22-X86-GRANT-NEG-COPY-FAULT: PASS"
    "negative downgrade during copy:::S22-X86-GRANT-NEG-COPY-DOWNGRADE: PASS"
    # Each control must fail for the one reason its disabled check exists for.
    "control revoke while mapped:::S22-X86-GRANT-CONTROL-REVOKE-MAPPED: CAUGHT (receiver-mapping-survived-revoke)"
    "control receiver death:::S22-X86-GRANT-CONTROL-RECEIVER-DEATH: CAUGHT (dead-receiver-mapping-survived)"
    "control stale grant id:::S22-X86-GRANT-CONTROL-STALE-ID: CAUGHT (owner-resolved-revoking-record)"
    "control cross-domain forgery:::S22-X86-GRANT-CONTROL-FORGERY: CAUGHT (third-domain-resolved-grant)"
    "control fault during copy:::S22-X86-GRANT-CONTROL-COPY-FAULT: CAUGHT (commit-accepted-revoked-page)"
    "control downgrade during copy:::S22-X86-GRANT-CONTROL-COPY-DOWNGRADE: CAUGHT (commit-accepted-downgraded-page)"
    "grant negatives:::S22-X86-GRANT-NEG: PASS"
    # The two-cell public pair, launched from the boot order by the fixture
    # launcher (`tier2-grant-pair`) on the two reviewed paths whose real services
    # this image does not package (`/bin/silo`, `/bin/net-broker`).
    "pair owner launch:::fixture-launcher: tier2-grant-pair owner admitted."
    "pair complete:::fixture-launcher: tier2-grant-pair complete."
    "pair owner admission:::[domain] admitted cell 'silo' to Tier 2 Paged Domain (CR3 isolation)"
    "pair receiver admission:::[domain] admitted cell 'net-broker' to Tier 2 Paged Domain (CR3 isolation)"
    "pair owner begin:::S22-X86-GRANT-PAIR-OWNER-BEGIN: public Grant* owner path"
    "pair owner alloc:::S22-X86-GRANT-PAIR-OWNER-ALLOC: OK id="
    "pair owner register:::S22-X86-GRANT-PAIR-OWNER-REGISTER: OK id="
    "pair owner mapped:::S22-X86-GRANT-PAIR-OWNER-MAPPED: OK"
    "pair owner registered mapping:::S22-X86-GRANT-PAIR-OWNER-REG-MAPPED: OK"
    "pair foreign share denied:::S22-X86-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY"
    "pair handoff:::S22-X86-GRANT-PAIR-HANDOFF id1="
    "pair receiver rw:::S22-X86-GRANT-PAIR-RECEIVER-BEGIN: mode=rw"
    "pair receiver ro:::S22-X86-GRANT-PAIR-RECEIVER-BEGIN: mode=ro"
    "pair receiver downgrade:::S22-X86-GRANT-PAIR-RECEIVER-BEGIN: mode=downgrade"
    "pair receiver unregister:::S22-X86-GRANT-PAIR-RECEIVER-BEGIN: mode=unregister"
    "pair receiver exit:::S22-X86-GRANT-PAIR-RECEIVER-BEGIN: mode=exit"
    "pair receiver alloc:::S22-X86-GRANT-PAIR-RECEIVER-ALLOC: OK id="
    "pair unknown id denied:::S22-X86-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY"
    "pair write-only share denied:::S22-X86-GRANT-PAIR-OWNER-SHARE-WO: DENY"
    "pair rw slice:::S22-X86-GRANT-PAIR-RECEIVER-SLICE-RW: OK"
    "pair rw access:::S22-X86-GRANT-PAIR-RECEIVER-RW: OK"
    "pair owner free:::S22-X86-GRANT-PAIR-OWNER-FREE: OK"
    "pair revoked id refused:::S22-X86-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED"
    "pair ro slice:::S22-X86-GRANT-PAIR-RECEIVER-SLICE-RO: OK (read "
    "pair downgrade rw:::S22-X86-GRANT-PAIR-RECEIVER-DOWNGRADE-RW: OK"
    "pair downgrade reshare:::S22-X86-GRANT-PAIR-OWNER-DOWNGRADE-RESHARE: OK"
    "pair downgrade read:::S22-X86-GRANT-PAIR-RECEIVER-DOWNGRADE-READ: OK (read 0xa5)"
    "pair unregister rw store:::S22-X86-GRANT-PAIR-RECEIVER-UNREGISTER-RW: OK"
    "pair owner unregister:::S22-X86-GRANT-PAIR-OWNER-UNREGISTER: OK"
    "pair owner exit:::S22-X86-GRANT-PAIR-OWNER-EXIT: OK"
    "pair terminal:::S22-X86-GRANT-PAIR-OWNER: PASS"
    "grant gate-held measurement:::[grant] gate-held max publish="
    "interactive shell:::Cellos > "
)
if (( X86_CPUS >= 2 )); then
    REQUIRED_MARKERS+=(
        "qualified $X86_CPUS-CPU topology:::[x86-smp] topology online=$X86_CPUS processor_ids="
        "broadcast invalidation acknowledged by every AP:::[x86-smp] transport IPI-invalidate broadcast targets=$((X86_CPUS - 1)) "
        "raw-context ownership and retirement:::S22-X86-CONTEXT-HANDOFF: PASS"
        "actual remote translation invalidation:::S22-X86-REMOTE-SHOOTDOWN: PASS harts=$X86_CPUS "
        "invalidation answered under a masked uncontended lock:::S22-X86-MASKED-SHOOTDOWN: PASS harts=$X86_CPUS "
        "reaper frees nothing while a CPU is silent:::S22-X86-QUARANTINE-SILENT-CPU: CONTROL silent=1 "
        "teardown quarantines under a silent CPU, frees after:::S22-X86-QUARANTINE-SILENT-CPU: PASS harts=$X86_CPUS "
        "silent-CPU window opened:::[frame-run] silent-cpu window OPEN"
        "silent-CPU window closed:::[frame-run] silent-cpu window CLOSED"
    )
fi

# SMAP is on whenever the CPU has it (the default TCG model and KVM `-cpu
# host` on a host with SMAP). The kernel's boot line says so, and then a Sas
# cell's `Log` must copy in through the user-access window while a raw
# supervisor read of the same user byte faults (`SMAP-WINDOW`), and a Sas
# thread's `GrantSlice` must write `size_out` to its user stack through the
# window (`SMAP-GRANT-SLICE`; a raw store there is a kernel #PF).
if grep -aqE '\[x86\] protection: .*smap=on' "$LOG"; then
    REQUIRED_MARKERS+=(
        "SMAP window:::[selftest] SMAP-WINDOW: PASS"
        "GrantSlice size_out under SMAP:::[selftest] SMAP-GRANT-SLICE: PASS"
    )
fi

# The guest-memory budget fixture (census §8.6(b)) runs only with SVM/VMX root
# operation, which this image enters when the CPU exposes it (e.g. KVM `-cpu
# host` on a host with nested virtualization; the TCG default model has none).
if grep -aqF "x86 virt: root operation active" "$LOG"; then
    REQUIRED_MARKERS+=(
        "guest-memory budget:::[selftest] VM-GUEST-BUDGET: PASS"
    )
fi

# The tag-pool owner-limit fixture (census §8.5) needs a VM tag, which only SVM
# has (an ASID per vCPU; VMX uses no VPID and the fixture logs SKIP there).
if grep -aqF "x86 virt: root operation active (SVM)" "$LOG"; then
    REQUIRED_MARKERS+=(
        "tag-pool owner limit:::[selftest] TAG-POOL-OWNER-LIMIT: PASS"
    )
fi

# --- Shootdown fail-stop -----------------------------------------------------
# Every unconfirmed tag or shared-mapping invalidation is logged (`[tlb] tag …
# invalidation unconfirmed`, `[frame-run] shared invalidation unconfirmed`,
# `[tlb] shared invalidation unconfirmed`); each is a lane failure, never a
# retry.
#
# Two exceptions, each bracketed by the kernel itself, and each excusing only
# the lines that name themselves, so a real silent hart inside a window still
# fails the lane, as does any line outside one. The brackets must balance.
# - The grant deferred-revoke fixture withholds every tag acknowledgement
#   between `[grant-neg] withhold-ack window OPEN` and `… CLOSED`; its forced
#   failures read `[tlb] tag N invalidation unconfirmed: withheld (test)`.
# - The silent-CPU quarantine fixture (2+ CPUs) holds the RT CPU with
#   interrupts masked between `[frame-run] silent-cpu window OPEN` and
#   `… CLOSED`; its one teardown reads `[frame-run] shared invalidation
#   unconfirmed: hart N silent through the probe; M pages quarantined: held
#   (fixture)`.
for bracket in "\[grant-neg\] withhold-ack window" "\[frame-run\] silent-cpu window"; do
    OPENED=$(grep -ac "$bracket OPEN" "$LOG" || true)
    CLOSED=$(grep -ac "$bracket CLOSED" "$LOG" || true)
    if [[ "$OPENED" -ne "$CLOSED" ]]; then
        echo "FAIL: '$bracket' windows do not balance (OPEN=$OPENED CLOSED=$CLOSED)" >&2
        exit 1
    fi
done
UNCONFIRMED=$(awk '
    /\[grant-neg\] withhold-ack window OPEN/ { withheld = 1; next }
    /\[grant-neg\] withhold-ack window CLOSED/ { withheld = 0; next }
    /\[frame-run\] silent-cpu window OPEN/ { held = 1; next }
    /\[frame-run\] silent-cpu window CLOSED/ { held = 0; next }
    withheld && /\[tlb\] tag [0-9]+ invalidation unconfirmed: withheld \(test\)$/ { next }
    held && /\[frame-run\] shared invalidation unconfirmed: hart [0-9]+ silent through the probe; [0-9]+ pages quarantined: held \(fixture\)$/ { next }
    /invalidation unconfirmed/ { print }
' "$LOG")
if [[ -n "$UNCONFIRMED" ]]; then
    echo "FAIL: an invalidation went unconfirmed:" >&2
    printf '%s\n' "$UNCONFIRMED" >&2
    exit 1
fi

bash scripts/assert-boot-markers.sh "$LOG" "$LABEL" "${REQUIRED_MARKERS[@]}"

# --- Grant fixture negatives -------------------------------------------------
# No x86 fixture may report FAIL, and no control may report MISSED: a control
# whose case still passed with its check disabled proves nothing about that
# check, and one that failed for another reason (`FAIL`) did not exercise it.
# An unregistered pair grant that still resolves (`FRAME-REUSE: REUSED`) fails
# too. The marker prefix is architecture-honest: an x86 boot emitting an
# RV64- or AArch64-tagged grant marker fails outright.
NEGATIVE_FAILURES='S22-X86-[A-Z0-9-]+: FAIL|S22-X86-GRANT-CONTROL-[A-Z-]+: MISSED|S22-X86-GRANT-PAIR-RECEIVER-FRAME-REUSE: REUSED'
if grep -aqE "$NEGATIVE_FAILURES" "$LOG"; then
    echo "FAIL: an x86 fixture reported a failure, a control missed or misattributed its violation, or a revoked pair id resolved:" >&2
    grep -aE "$NEGATIVE_FAILURES" "$LOG" >&2
    exit 1
fi
if grep -aqE 'S22-(RV64|AARCH64)-GRANT' "$LOG"; then
    echo "FAIL: an x86 boot emitted another architecture's grant marker:" >&2
    grep -aE 'S22-(RV64|AARCH64)-GRANT' "$LOG" | head -5 >&2
    exit 1
fi

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
grep -a "S22-X86-GRANT-CONTROL-\|\[grant\] gate-held max" "$LOG" || true
echo "Pair faults: NULL=1 id1($A1)=$(faults_at "$A1") id2($A2)=$(faults_at "$A2") id3($A3)=$(faults_at "$A3") id4($A4)=$(faults_at "$A4")"
echo "Pair causes: id1=$(fault_cause_at "$A1" 1) id2=$(fault_cause_at "$A2" 1),$(fault_cause_at "$A2" 2) id3=$(fault_cause_at "$A3" 1) id4=$(fault_cause_at "$A4" 1)"
echo "PASS: x86_64 Tier-2 domain-entry lane ($LABEL) — every marker present"
