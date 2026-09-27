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

Cases: switch, resume-root, sas-fastpath, migration, user-copy, user-copy-race,
admission, rollback, grant-revoke, grant-gate, grant-pair, asid-lease,
unmap-order, rt-wake

`grant-pair` drives the phase-03 step-5 Tier-2 grant pair: it background-spawns
the owner cell, reads the owner's `S22-RV64-GRANT-PAIR-HANDOFF` line, then spawns
one receiver generation per property (ReadWrite + GrantFree, ReadOnly write,
GrantUnregister, owner exit), classifying each deliberate receiver store fault
against the exact grant address the owner handed over. It is the only case that
writes to the guest console; the others boot unattended.

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
        switch|resume-root|sas-fastpath|migration|user-copy|user-copy-race|ipc-copy|ipc-copy-race|admission|admission-enabled|admission-publication|admission-ceiling|futex-key|rollback|grant-revoke|grant-gate|grant-pair|asid-lease|unmap-order|rt-wake) ;;
        *) echo "FAIL: unknown native-domain case: $case_id" >&2; exit 2 ;;
    esac
    [[ -z "${seen[$case_id]:-}" ]] || { echo "FAIL: duplicate native-domain case: $case_id" >&2; exit 2; }
    seen[$case_id]=1
    if [[ "$case_id" == "migration" && "$HARTS" != "2" ]]; then
        echo "FAIL: migration requires --harts 2" >&2
        exit 2
    fi
    if [[ "$case_id" == "rt-wake" && "$HARTS" != "2" ]]; then
        echo "FAIL: rt-wake requires --harts 2 (the RT hart must be online)" >&2
        exit 2
    fi
    if [[ "$case_id" == "user-copy-race" && "$HARTS" != "2" ]]; then
        echo "FAIL: user-copy-race requires --harts 2" >&2
        exit 2
    fi
    if [[ "$case_id" == "ipc-copy-race" && "$HARTS" != "2" ]]; then
        echo "FAIL: ipc-copy-race requires --harts 2" >&2
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
        resume-root) printf 'S22-RV64-RESUME-ROOT: PASS harts=%s' "$HARTS" ;;
        migration) printf 'S22-RV64-MIGRATION: PASS harts=2' ;;
        sas-fastpath) printf 'S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0 harts=%s' "$HARTS" ;;
        user-copy) printf 'S22-RV64-COPY: PASS harts=%s' "$HARTS" ;;
        user-copy-race) printf 'S22-RV64-COPY-RACE: PASS harts=2' ;;
        ipc-copy) printf 'S22-RV64-IPC-COPY: PASS harts=%s' "$HARTS" ;;
        ipc-copy-race) printf 'S22-RV64-IPC-COPY-RACE: PASS harts=2' ;;
        admission) printf 'S22-RV64-ADMISSION-DENY: PASS' ;;
        admission-enabled) printf 'S22-RV64-ADMISSION-ENABLED: PASS' ;;
        admission-publication) printf 'S22-RV64-ADMISSION-PUBLICATION-DENY: PASS' ;;
        admission-ceiling) printf 'S22-RV64-ADMISSION-CEILING: PASS' ;;
        futex-key) printf 'S22-RV64-FUTEX-KEY: PASS' ;;
        rollback) printf 'S22-RV64-ADMISSION-DRAIN: PASS' ;;
        grant-revoke) printf 'S22-RV64-GRANT-REVOKE: PASS' ;;
        grant-gate) printf 'S22-RV64-GRANT-GATE: PASS' ;;
        grant-pair) printf 'S22-RV64-GRANT-PAIR-OWNER: PASS' ;;
        asid-lease) printf 'S22-RV64-ASID-LEASE: PASS' ;;
        unmap-order) printf 'S22-RV64-UNMAP-ORDER: PASS' ;;
        rt-wake) printf 'S22-RV64-RT-WAKE: PASS harts=2' ;;
    esac
}
terminal_pattern_for() {
    case "$1" in
        switch) printf '(^|\\] )S22-RV64-SWITCH: PASS harts=%s$' "$HARTS" ;;
        resume-root) printf '(^|\\] )S22-RV64-RESUME-ROOT: PASS harts=%s$' "$HARTS" ;;
        migration) printf '(^|\\] )S22-RV64-MIGRATION: PASS harts=2$' ;;
        sas-fastpath) printf '(^|\\] )S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0 harts=%s$' "$HARTS" ;;
        user-copy) printf '(^|\\] )S22-RV64-COPY: PASS harts=%s$' "$HARTS" ;;
        user-copy-race) printf '(^|\\] )S22-RV64-COPY-RACE: PASS harts=2$' ;;
        ipc-copy) printf '(^|\\] )S22-RV64-IPC-COPY: PASS harts=%s$' "$HARTS" ;;
        ipc-copy-race) printf '(^|\\] )S22-RV64-IPC-COPY-RACE: PASS harts=2$' ;;
        admission) printf '(^|\\] )S22-RV64-ADMISSION-DENY: PASS$' ;;
        admission-enabled) printf '(^|\\] )S22-RV64-ADMISSION-ENABLED: PASS$' ;;
        admission-publication) printf '(^|\\] )S22-RV64-ADMISSION-PUBLICATION-DENY: PASS$' ;;
        admission-ceiling) printf '(^|\\] )S22-RV64-ADMISSION-CEILING: PASS$' ;;
        futex-key) printf '(^|\\] )S22-RV64-FUTEX-KEY: PASS$' ;;
        rollback) printf '(^|\\] )S22-RV64-ADMISSION-DRAIN: PASS$' ;;
        grant-revoke) printf '(^|\\] )S22-RV64-GRANT-REVOKE: PASS$' ;;
        grant-gate) printf '(^|\\] )S22-RV64-GRANT-GATE: PASS$' ;;
        # Guest-cell output is prefixed `USER: ` on the kernel console, so the
        # pair's terminal matches that prefix instead of the kernel log bracket.
        # The receiver halves end in a deliberate store fault, so the owner is
        # the only terminal; each receiver property is witnessed by its own
        # marker plus a fault line classified to the exact grant address below.
        grant-pair) printf '(^|USER: |\\] )S22-RV64-GRANT-PAIR-OWNER: PASS$' ;;
        asid-lease) printf '(^|\\] )S22-RV64-ASID-LEASE: PASS$' ;;
        unmap-order) printf '(^|\\] )S22-RV64-UNMAP-ORDER: PASS$' ;;
        rt-wake) printf '(^|\\] )S22-RV64-RT-WAKE: PASS harts=2$' ;;
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
# Fault addresses the interactive grant-pair case expects: one deliberate store
# per receiver property, each classified to the exact grant address the owner
# handed over. Populated by run_grant_pair_interactive.
GRANT_PAIR_FAULT_ADDRS=""

# SWITCH logs one line per genuine Activate transition (domain_switch.rs
# root_switch), so multi-stage fixtures emit it several times per boot; its
# gate is at-least-one. Other markers are boot-terminal aggregates and stay
# exactly-one (min 0 = exact).
terminal_min_for() {
    case "$1" in
        switch) echo 1 ;;
        grant-pair) echo 1 ;;
        *)      echo 0 ;;
    esac
}

# Poll a growing raw QEMU log for an extended-regex pattern, bailing out if the
# emulator dies first. Used only by the interactive grant-pair case.
wait_for_log_pattern() {
    local file="$1" pattern="$2" secs="$3" pid="$4"
    local ticks=$(( secs * 4 ))
    local i=0
    while (( i < ticks )); do
        if grep -aqE -- "$pattern" "$file" 2>/dev/null; then
            return 0
        fi
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        sleep 0.25
        i=$(( i + 1 ))
    done
    return 1
}

# Interactive launcher for the phase-03 step-5 grant pair. Unlike every other
# case this one drives the guest console (same reviewed FIFO pattern as
# scripts/qemu-c-spawn.sh): it waits for the shell prompt, spawns the owner on
# its reviewed launch edge, drives one receiver generation per property over the
# public Grant* lifecycle, and classifies every receiver fault from the kernel's
# own fault line. Both cells therefore traverse the production SpawnFromPath
# edge as real Tier-2 private-root domains.
#
# Phase protocol. The owner allocates, proves its own mapping, publishes the
# handoff, then serves one receiver generation at a time. Each generation asks
# for the rights it needs over IPC and asks for the teardown its mode witnesses:
#
#   rw         -> ReadWrite mapping works, owner GrantFree revokes, the revoked id
#                 no longer resolves, and a store to the old address faults;
#   ro         -> ReadOnly mapping is readable and a store to it faults;
#   unregister -> owner GrantUnregister revokes and a store to the old address
#                 faults;
#   exit       -> owner exit revokes and a store to the old address faults.
#
# The phase address is what makes each fault attributable: every deliberate store
# lands on the exact grant base the owner handed over, so the classifier below
# cannot be satisfied by an unrelated cell fault.
run_grant_pair_interactive() {
    local fifo="$case_dir/stdin"
    local window="${GRANT_PAIR_WINDOW:-300}"
    local step="${GRANT_PAIR_STEP_TIMEOUT:-60}"
    local qemu_pid handoff id1 id2 id3 a1 a2 a3

    grant_pair_abort() {
        # Keep stderr: the FAIL line below is the only diagnosis a reader gets.
        exec 3>&- || true
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
        echo "FAIL: $1 for case=grant-pair; see $raw_log" >&2
        exit 1
    }

    # wait_marker <extended-regex> <description>
    wait_marker() {
        wait_for_log_pattern "$raw_log" "$1" "$step" "$qemu_pid" \
            || grant_pair_abort "missing $2 (pattern: $1)"
    }

    # fault_count <addr-in-hex> — deliberate store faults classified to one address.
    fault_count() {
        grep -acE "\[fault\] Cell [0-9]+ \(task [0-9]+ generation [0-9]+\) terminated: cause=0xf pc=0x[0-9a-f]+ addr=$1" "$raw_log" || true
    }

    # wait_fault <addr-in-hex> <at-least-count> <description>
    wait_fault() {
        local want="$2"
        local i
        for (( i = 0; i < step * 4; i++ )); do
            if [[ "$(fault_count "$1")" -ge "$want" ]]; then
                return 0
            fi
            kill -0 "$qemu_pid" 2>/dev/null || break
            sleep 0.25
        done
        grant_pair_abort "missing $3 (no classified store fault at $1)"
    }

    mkfifo "$fifo"
    qemu_status=0
    timeout "$window" "$QEMU" "${qemu_args[@]}" < "$fifo" > "$raw_log" 2>&1 &
    qemu_pid=$!
    # Open read+write so the fifo open never blocks (a write-only open waits for
    # a reader, and would hang forever if the emulator died before opening it).
    # The shell never reads fd 3, so the guest still receives every command.
    exec 3<> "$fifo"

    wait_for_log_pattern "$raw_log" 'Cellos >' 115 "$qemu_pid" \
        || grant_pair_abort "guest shell prompt never appeared"
    sleep 1

    # Owner: allocate, prove the owner mapping, publish the handoff, then serve.
    # It must outlive the shell's foreground child, so it is backgrounded: the
    # shell otherwise blocks in sys_wait for `/bin/tier2-smoke` and never reads
    # the receiver's command line.
    printf 'tier2-smoke &\n' >&3
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER-MAPPED: OK' 'owner self-mapping'
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER-REG-MAPPED: OK' 'owner registered mapping'
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY' 'non-private-root share denial'
    wait_marker 'S22-RV64-GRANT-PAIR-HANDOFF ' 'owner handoff line'

    handoff="$(grep -aoE 'S22-RV64-GRANT-PAIR-HANDOFF id1=[0-9]+ id2=[0-9]+ id3=[0-9]+' "$raw_log" | tail -n 1 || true)"
    id1="$(printf '%s' "$handoff" | sed -n 's/.*id1=\([0-9]\+\).*/\1/p')"
    id2="$(printf '%s' "$handoff" | sed -n 's/.*id2=\([0-9]\+\).*/\1/p')"
    id3="$(printf '%s' "$handoff" | sed -n 's/.*id3=\([0-9]\+\).*/\1/p')"
    if [[ -z "$id1" || -z "$id2" || -z "$id3" || "$id1" == 0 || "$id2" == 0 || "$id3" == 0 ]]; then
        grant_pair_abort "unparsable handoff line: '$handoff'"
    fi
    a1="$(printf '0x%x' "$id1")"
    a2="$(printf '0x%x' "$id2")"
    a3="$(printf '0x%x' "$id3")"
    # Phase 1 revokes id1; phases 2 and 3 both fault at id2; phase 4 at id3.
    GRANT_PAIR_FAULT_ADDRS="$a1 $a2 $a3"

    # Phase 1: ReadWrite works, GrantFree revokes, reused frames stay private,
    # the revoked address faults.
    printf 'tier2-exploit rw %s %s %s\n' "$id1" "$id2" "$id3" >&3
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY' 'unknown-id slice denial'
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER-SHARE-WO: DENY' 'write-only share denial'
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-RW: OK' 'ReadWrite receiver slice'
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER-FREE: OK' 'owner GrantFree'
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED' 'revoked id refused again'
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED' 'revoke fault announcement'
    wait_fault "$a1" 1 'store fault at the freed grant address'

    # Phase 2: a ReadOnly mapping's write must fault.
    printf 'tier2-exploit ro %s %s %s\n' "$id1" "$id2" "$id3" >&3
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RO: OK' 'ReadOnly receiver slice'
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED' 'read-only write announcement'
    wait_fault "$a2" 1 'store fault on the read-only grant page'

    # Phase 3: GrantUnregister revokes the persistent buffer.
    printf 'tier2-exploit unregister %s %s %s\n' "$id1" "$id2" "$id3" >&3
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER-UNREGISTER: OK' 'owner GrantUnregister'
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED' 'unregister fault announcement'
    wait_fault "$a2" 2 'second store fault at the unregistered grant address'

    # The owner publishes its terminal before phase 4's deliberate exit.
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER: PASS' 'owner terminal'

    # Phase 4: the owner's exit must revoke the receiver mapping.
    printf 'tier2-exploit exit %s %s %s\n' "$id1" "$id2" "$id3" >&3
    wait_marker 'S22-RV64-GRANT-PAIR-OWNER-EXIT: OK' 'owner exit'
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RW: OK' 'exit-phase receiver slice'
    wait_marker 'S22-RV64-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED' 'exit fault announcement'
    wait_fault "$a3" 1 'store fault at the address the owner exit revoked'

    exec 3>&-
    sleep 1
    kill "$qemu_pid" 2>/dev/null || true
    wait "$qemu_pid" 2>/dev/null || true
    # The guest is killed deliberately once every marker is observed, so its
    # exit status carries no signal for this case.
    qemu_status=0
    printf 'S22-RV64-GRANT-PAIR-HANDOFF-OBSERVED id1=%s id2=%s id3=%s faults=%s:%s:%s\n' \
        "$id1" "$id2" "$id3" \
        "$(fault_count "$a1")" "$(fault_count "$a2")" "$(fault_count "$a3")"
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
    if [[ "$case_id" == "grant-pair" ]]; then
        run_grant_pair_interactive
    else
        timeout "$BOOT_WINDOW" "$QEMU" "${qemu_args[@]}" < /dev/null > "$raw_log" 2>&1 || qemu_status=$?
    fi
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
        # Two injected canaries are expected, and each is identified by its
        # cause rather than by counters that record how far the concurrent
        # campaign had progressed:
        #   - Cell 254 / any task, cause=0xf, address in the campaign's scratch
        #     page — the user-copy fault fixture;
        #   - Cell 63 / task 6, cause=0xdead, null pc/addr — the SMP
        #     fault-retirement selftest's synthetic trap. Its `generation` is
        #     `NEXT_DOMAIN`, i.e. how many private AddressSpaces had been built
        #     when the record was published. The phase-07 captures read 99 there;
        #     this workstation reads 133 today **on the unmodified kernel too**
        #     (measured 2026-09-15: pre-change build, harts=2, same line), so the
        #     number is a timing artifact of the boot, not a contract. Pinning it
        #     back would fail a correct kernel.
        classified=0
        if [[ "$fault_line" =~ ^\[ERROR\]\ \[fault\]\ Cell\ 254\ \(task\ [0-9]+\ generation\ [0-9]+\)\ terminated:\ cause=0xf\ pc=0x[0-9a-f]+\ addr=0x[0-9a-f]+$ ]] \
            || [[ "$fault_line" =~ ^\[ERROR\]\ \[fault\]\ Cell\ 63\ \(task\ 6\ generation\ [0-9]+\)\ terminated:\ cause=0xdead\ pc=0x0\ addr=0x0$ ]]; then
            classified=1
        fi
        # The grant pair's receivers end in a deliberate store to the exact grant
        # address the owner handed over, so each of those faults is classified by
        # its address rather than by a cell id (the tid differs per phase).
        if [[ "$case_id" == "grant-pair" && -n "$GRANT_PAIR_FAULT_ADDRS" ]]; then
            for grant_addr in $GRANT_PAIR_FAULT_ADDRS; do
                if [[ "$fault_line" =~ ^\[ERROR\]\ \[fault\]\ Cell\ [0-9]+\ \(task\ [0-9]+\ generation\ [0-9]+\)\ terminated:\ cause=0xf\ pc=0x[0-9a-f]+\ addr=$grant_addr$ ]]; then
                    classified=1
                fi
            done
        fi
        if [[ "$classified" != 1 ]]; then
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

    # The phase-03 step-5 pair is asserted against the *positive* lifecycle
    # contract: both real Tier-2 domains must be admitted, the owner must prove
    # its own mapping and observe an allocatable/registrable backing, the
    # receiver must observe permission-accurate rights, and every revoke path
    # (GrantFree, GrantUnregister, owner exit) must be witnessed by a classified
    # store fault at the exact grant address. The denial assertions that remain
    # are the ones the phase still must refuse: a non-private-root peer, a
    # WriteOnly domain pair, and an unknown grant id.
    if [[ "$case_id" == "grant-pair" ]]; then
        grant_pair_required=(
            "[domain] admitted cell 'tier2-smoke'"
            "[domain] admitted cell 'tier2-exploit'"
            'S22-RV64-GRANT-PAIR-OWNER-ALLOC: OK id='
            'S22-RV64-GRANT-PAIR-OWNER-REGISTER: OK id='
            'S22-RV64-GRANT-PAIR-OWNER-MAPPED: OK'
            'S22-RV64-GRANT-PAIR-OWNER-REG-MAPPED: OK'
            'S22-RV64-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY'
            'S22-RV64-GRANT-PAIR-OWNER-SHARE-WO: DENY'
            'S22-RV64-GRANT-PAIR-OWNER-FREE: OK'
            'S22-RV64-GRANT-PAIR-OWNER-UNREGISTER: OK'
            'S22-RV64-GRANT-PAIR-OWNER-EXIT: OK'
            'S22-RV64-GRANT-PAIR-OWNER: PASS'
            'S22-RV64-GRANT-PAIR-RECEIVER-ALLOC: OK id='
            'S22-RV64-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY'
            'S22-RV64-GRANT-PAIR-RECEIVER-RW: OK'
            'S22-RV64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED'
            'S22-RV64-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED'
            'S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RO: OK'
            'S22-RV64-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED'
            'S22-RV64-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED'
            'S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RW: OK'
            'S22-RV64-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED'
        )
        for required_marker in "${grant_pair_required[@]}"; do
            if ! grep -Fq -- "$required_marker" "$normalized_log"; then
                echo "FAIL: grant-pair missing lifecycle marker: $required_marker; see $normalized_log" >&2
                exit 1
            fi
        done
        # Each phase's deliberate store must be attributable: one classified
        # store fault at the freed address, one at the read-only page, a second
        # at the unregistered page, and one at the address the owner exit
        # revoked. A receiver that silently stopped short of its store, or that
        # never faulted, cannot satisfy this.
        if [[ -z "$GRANT_PAIR_FAULT_ADDRS" ]]; then
            echo "FAIL: grant-pair observed no handoff line, so no fault address is known; see $normalized_log" >&2
            exit 1
        fi
        read -r fault_a1 fault_a2 fault_a3 <<< "$GRANT_PAIR_FAULT_ADDRS"
        observed_a1="$(grep -acE "\[fault\] Cell [0-9]+ \(task [0-9]+ generation [0-9]+\) terminated: cause=0xf pc=0x[0-9a-f]+ addr=$fault_a1" "$normalized_log" || true)"
        observed_a2="$(grep -acE "\[fault\] Cell [0-9]+ \(task [0-9]+ generation [0-9]+\) terminated: cause=0xf pc=0x[0-9a-f]+ addr=$fault_a2" "$normalized_log" || true)"
        observed_a3="$(grep -acE "\[fault\] Cell [0-9]+ \(task [0-9]+ generation [0-9]+\) terminated: cause=0xf pc=0x[0-9a-f]+ addr=$fault_a3" "$normalized_log" || true)"
        if [[ "$observed_a1" -lt 1 || "$observed_a2" -lt 2 || "$observed_a3" -lt 1 ]]; then
            echo "FAIL: grant-pair fault attribution ${fault_a1}=$observed_a1 ${fault_a2}=$observed_a2 ${fault_a3}=$observed_a3; see $normalized_log" >&2
            exit 1
        fi
    fi

    printf 'PASS: native-domain case=%s harts=%s terminal=%s\n' "$case_id" "$HARTS" "$marker"
done

printf 'S22-RV64-QEMU-SUITE: PASS HARTS=%s CASES=%s\n' "$HARTS" "$CASES"
