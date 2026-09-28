#!/usr/bin/env bash
# Boot the Cellos AArch64 test-hooks kernel in QEMU with semihosting enabled.
#
# Dedicated test-hooks runner; does not alter production boot runner defaults.
#
# Usage: scripts/qemu-aarch64-test-hooks.sh [kernel-elf] [disk.img]

set -euo pipefail

KERNEL="${1:-target/aarch64-unknown-none-softfloat/release/cellos-kernel-test-hooks}"
DISK="${2:-disk_arm_virt.img}"
BOOT_WINDOW="${BOOT_WINDOW:-35}"
# CPUs to boot with. The kernel-side SMP path is exercised explicitly with
# `QEMU_SMP=2`, which turns on the markers below; the domain fixtures stay on one
# CPU by default because their fault *pattern* is a single-hart expectation.
#
# Measured at `QEMU_SMP=2` (2026-09-29): every kernel-side marker holds — hart 1
# online, the cross-hart IPI answered, a task dispatched to hart 1, no panic, no
# deferred-record integrity error, vfs-test 96/0 — but the grant pair's fault
# counts come out `id1=1 id2=2 id3=0 id4=1` where it requires `id3=1`: the
# receiver's access at 0x426fe000 *succeeded* where the single-hart boot faults
# it. A receiver that ran ahead of an unconfirmed remote invalidation is the
# likely shape, i.e. this is the retirement/root-switch argument the plan lists
# as the blocker for domains on more than one hart — not a fixture regression.
QEMU_SMP="${QEMU_SMP:-1}"
DEVELOPMENT_SILO="${CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO:-0}"
if [[ "$DEVELOPMENT_SILO" != "0" && "$DEVELOPMENT_SILO" != "1" ]]; then
    echo "FAIL: CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO must be exactly 0 or 1" >&2
    exit 1
fi


if ! command -v qemu-system-aarch64 &>/dev/null; then
    echo "FAIL: qemu-system-aarch64 not found on PATH" >&2
    exit 1
fi

if [[ ! -f "$KERNEL" ]]; then
    echo "FAIL: test-hooks kernel ELF not found: $KERNEL" >&2
    echo "  Build with: bash scripts/build-aarch64-test-hooks-ci.sh" >&2
    exit 1
fi

# Create a temporary test disk if not existing
if [[ ! -f "$DISK" ]]; then
    if [[ -f "scripts/format-disk-arm.sh" ]]; then
        bash scripts/format-disk-arm.sh "$DISK"
    fi
fi

MACHINE="virt"
if [[ "$DEVELOPMENT_SILO" == "1" ]]; then
    MACHINE="virt,virtualization=on"
fi

echo "[qemu-aarch64-test-hooks] Booting kernel=$KERNEL (window=${BOOT_WINDOW}s, semihosting enabled, machine=$MACHINE)"

QEMU_ARGS=(
    -machine "$MACHINE"
    -cpu cortex-a57
    -m 256M
    -smp "$QEMU_SMP"
    -nographic
    -kernel "$KERNEL"
    -no-reboot
    -semihosting
)

if [[ -f "$DISK" ]]; then
    QEMU_ARGS+=(
        -drive "if=none,file=$DISK,format=raw,id=hd0"
        -device virtio-blk-device,drive=hd0
    )
fi

RAW_LOG="qemu-aarch64-test-hooks.raw.log"
LOG="qemu-aarch64-test-hooks.log"

QEMU_EXIT_CODE=0
timeout "$BOOT_WINDOW" qemu-system-aarch64 "${QEMU_ARGS[@]}" \
    < /dev/null > "$RAW_LOG" 2>&1 || QEMU_EXIT_CODE=$?

# Strip NULs and ANSI escape sequences
tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"

if [[ $QEMU_EXIT_CODE -ne 0 ]]; then
    echo "FAIL: QEMU exited with code $QEMU_EXIT_CODE (expected 0 via semihosting)" >&2
    tail -40 "$LOG"
    exit 1
fi

if grep -qia "KERNEL PANIC\|panicked" "$LOG"; then
    echo "FAIL: kernel panic detected during aarch64 test-hooks boot" >&2
    grep -ai "PANIC\|panic" "$LOG" | head -20
    exit 1
fi

# ── SMP ──────────────────────────────────────────────────────────────────────
# A second hart has to be *started*, has to answer the kernel's cross-hart IPI,
# and has to be handed a task — a hart that is online but deaf, or online but
# never scheduling, looks exactly like a healthy single-hart boot. Only asserted
# when the machine was actually given more than one CPU.
if [[ "$QEMU_SMP" -gt 1 ]]; then
    for marker in "[smp] hart 1 online, parked" "[selftest] SMP-IPI: PASS hart=1" "[sched] hart 1 dispatched a task"; do
        if ! grep -qaF "$marker" "$LOG"; then
            echo "FAIL: missing SMP marker with -smp $QEMU_SMP: $marker" >&2
            grep -a "\[smp\]\|\[sched\]" "$LOG" | head -20
            exit 1
        fi
    done
fi

# Fault containment is an asserted outcome, not a forbidden one. Every cell fault
# in the boot must be deliberate, announced, and classified: `tier2-exploit`'s
# NULL store (exactly one, at `addr=0x0`) and the phase-03 grant pair's five
# receiver-generation store faults, each at the exact grant address the owner
# handed over. A fault line that is not one of those — a second NULL store,
# another cell, another address, an unannounced fault — still fails the lane. The
# EL2 development-Silo machine witnesses no Tier-2 entry at all, so there it stays
# a blanket prohibition.
#
# The cells are launched from the boot order (init), not typed at the shell: this
# image's shell sleeps ~2 s before its first prompt and its own test root exits
# the VM at ~4.5 s, so a prompt-driven sequence cannot run at all. Containment is
# therefore read off the boot instead: each deliberate fault must be announced by
# its launcher first, and the boot's terminal marker must come *after* them — the
# kernel, init and the remaining test root all survived a domain cell's fault.
if [[ "$DEVELOPMENT_SILO" == "1" ]]; then
    if grep -qaiE '\[fault\] Cell' "$LOG"; then
        echo "FAIL: cell fault detected during the AArch64 development-Silo boot" >&2
        grep -ai "\[fault\] Cell" "$LOG" | head -20
        exit 1
    fi
else
    # Fault containment is asserted, not merely tolerated: every `[fault] Cell`
    # line in the boot must be one of the deliberate, announced, address-classified
    # faults, and nothing else. Two fixtures produce them:
    #
    #   * `tier2-exploit`'s NULL store — the phase-02 containment witness, exactly
    #     one line, at `addr=0x0`, announced before it;
    #   * the phase-03 pair's five receiver generations (see below) — each ends in
    #     the store its phase exists to witness, classified to the exact grant
    #     address the owner handed over.
    #
    # The total is therefore fixed, and any other fault — a second NULL store,
    # another cell, another address — still fails the lane.
    FAULT_LINES="$(grep -aiE '\[fault\] Cell' "$LOG" || true)"
    FAULT_COUNT="$(printf '%s\n' "$FAULT_LINES" | grep -c . || true)"

    # The pair's fault addresses are the grant ids the owner published, so the
    # classification is read from the boot's own handoff line. Without it there is
    # no way to attribute a receiver fault to its phase.
    HANDOFF_LINE="$(grep -aoE 'S22-AARCH64-GRANT-PAIR-HANDOFF id1=[0-9]+ id2=[0-9]+ id3=[0-9]+ id4=[0-9]+' "$LOG" | tail -1 || true)"
    if [[ -z "$HANDOFF_LINE" ]]; then
        echo "FAIL: the grant pair published no handoff line, so no deliberate receiver fault can be classified" >&2
        exit 1
    fi
    handoff_id() { printf '%s' "$HANDOFF_LINE" | sed -n "s/.*$1=\([0-9]\+\).*/\1/p"; }
    A1="$(printf '0x%x' "$(handoff_id id1)")"
    A2="$(printf '0x%x' "$(handoff_id id2)")"
    A3="$(printf '0x%x' "$(handoff_id id3)")"
    A4="$(printf '0x%x' "$(handoff_id id4)")"
    if [[ "$A1" == "0x" || "$A2" == "0x" || "$A3" == "0x" || "$A4" == "0x" ]]; then
        echo "FAIL: unparsable handoff line: '$HANDOFF_LINE'" >&2
        exit 1
    fi
    # Count the faults classified to one exact address.
    faults_at() {
        grep -aciE "\[fault\] Cell [0-9]+ \(task [0-9]+ generation [0-9]+\) terminated: cause=0x[0-9a-f]+ pc=0x[0-9a-f]+ addr=$1\$" "$LOG" || true
    }
    # Byte offset of the Nth fault at one exact address (1-based); empty when absent.
    fault_offset_at() {
        grep -aboE "\[fault\] Cell [0-9]+ \(task [0-9]+ generation [0-9]+\) terminated: cause=0x[0-9a-f]+ pc=0x[0-9a-f]+ addr=$1\$" "$LOG" \
            | sed -n "$2p" | cut -d: -f1 || true
    }

    if [[ "$(faults_at 0x0)" != "1" ]]; then
        echo "FAIL: expected exactly one contained NULL-store fault, found $(faults_at 0x0):" >&2
        printf '%s\n' "$FAULT_LINES" >&2
        exit 1
    fi
    if ! printf '%s\n' "$FAULT_LINES" | grep -aqE 'terminated: cause=0x[0-9a-f]+ pc=0x[0-9a-f]+ addr=0x0$'; then
        echo "FAIL: the contained NULL-store fault is not present: $FAULT_LINES" >&2
        exit 1
    fi
    if ! grep -aqF "[tier2-exploit] deliberately writing to NULL (0x0) — expect Page Fault termination" "$LOG"; then
        echo "FAIL: the contained fault was not preceded by tier2-exploit's announcement" >&2
        exit 1
    fi
    # The pair's five deliberate faults: one at the freed grant (phase 1), one on
    # the ReadOnly mapping (phase 2) and a second at the same address once
    # `GrantUnregister` revoked it (phase 4), one at the downgraded grant (phase
    # 3), and one at the address the owner's exit revoked (phase 5). The counts
    # are exact: a phase that did not fault, or faulted twice, is a different
    # lifetime from the one this lane asserts.
    PAIR_FAULTS=$(( $(faults_at "$A1") + $(faults_at "$A2") + $(faults_at "$A3") + $(faults_at "$A4") ))
    if [[ "$(faults_at "$A1")" != "1" || "$(faults_at "$A2")" != "2" \
        || "$(faults_at "$A3")" != "1" || "$(faults_at "$A4")" != "1" ]]; then
        echo "FAIL: the pair's deliberate fault counts are wrong (want id1:$A1=1 id2:$A2=2 id3:$A3=1 id4:$A4=1," >&2
        echo "      found id1=$(faults_at "$A1") id2=$(faults_at "$A2") id3=$(faults_at "$A3") id4=$(faults_at "$A4")):" >&2
        printf '%s\n' "$FAULT_LINES" >&2
        exit 1
    fi
    EXPECTED_FAULTS=$(( 1 + PAIR_FAULTS ))
    if [[ "$FAULT_COUNT" != "$EXPECTED_FAULTS" ]]; then
        echo "FAIL: expected exactly $EXPECTED_FAULTS accounted cell faults (the announced NULL store plus the" >&2
        echo "      pair's five classified store faults), found $FAULT_COUNT:" >&2
        printf '%s\n' "$FAULT_LINES" >&2
        exit 1
    fi
    # Byte offsets make the ordering a property of the boot, not of the grep. The
    # anchor is the kernel's own admission line, which is printed inside the
    # publication path — before the cell can run. (init's own "admitted" print
    # lands *after* the spawn syscall returns, so a cell that runs immediately can
    # fault before it: measured, 174 bytes earlier.)
    offset_of() { grep -aboF -- "$1" "$LOG" | head -1 | cut -d: -f1 || true; }
    ADMIT_OFFSET="$(offset_of "[domain] admitted cell 'tier2-exploit' to Tier 2 Paged Domain")"
    FAULT_OFFSET="$(fault_offset_at 0x0 1)"
    TERMINAL_OFFSET="$(offset_of '[vfs-test] ALL TESTS PASSED')"
    if [[ -z "$ADMIT_OFFSET" || -z "$FAULT_OFFSET" || -z "$TERMINAL_OFFSET" ]]; then
        echo "FAIL: missing admission/fault/terminal line (admit=$ADMIT_OFFSET fault=$FAULT_OFFSET terminal=$TERMINAL_OFFSET)" >&2
        exit 1
    fi
    # Each pair fault must follow the announcement that names it, so a fault can
    # never be attributed to a phase whose store had not been announced yet. The
    # unregister phase's fault is the *second* one at `id2`, so its announcement is
    # checked against that one.
    assert_announced_before() {
        local announcement="$1" address="$2" nth="$3" description="$4"
        local announced_at faulted_at
        announced_at="$(offset_of "$announcement")"
        faulted_at="$(fault_offset_at "$address" "$nth")"
        if [[ -z "$announced_at" || -z "$faulted_at" ]]; then
            echo "FAIL: pair phase '$description' has no announcement or no classified fault at $address" >&2
            exit 1
        fi
        if [[ "$announced_at" -ge "$faulted_at" ]]; then
            echo "FAIL: pair phase '$description' faulted at $address before its announcement (announced=$announced_at faulted=$faulted_at)" >&2
            exit 1
        fi
    }
    assert_announced_before "S22-AARCH64-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED" "$A1" 1 "GrantFree revoke"
    assert_announced_before "S22-AARCH64-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED" "$A2" 1 "ReadOnly write"
    assert_announced_before "S22-AARCH64-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED" "$A2" 2 "GrantUnregister"
    assert_announced_before "S22-AARCH64-GRANT-PAIR-RECEIVER-DOWNGRADE-WRITE: FAULT-EXPECTED" "$A4" 1 "same-recipient downgrade"
    assert_announced_before "S22-AARCH64-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED" "$A3" 1 "owner exit"
    if [[ "$ADMIT_OFFSET" -ge "$FAULT_OFFSET" ]]; then
        echo "FAIL: the faulted cell was not admitted before it faulted (admit=$ADMIT_OFFSET fault=$FAULT_OFFSET)" >&2
        exit 1
    fi
    if [[ "$FAULT_OFFSET" -ge "$TERMINAL_OFFSET" ]]; then
        echo "FAIL: the boot did not continue past the contained fault (fault=$FAULT_OFFSET terminal=$TERMINAL_OFFSET)" >&2
        exit 1
    fi
    if [[ "$(grep -acF "[domain] admitted cell 'tier2-smoke'" "$LOG" || true)" -lt 1 ]]; then
        echo "FAIL: tier2-smoke was never admitted to a private root" >&2
        exit 1
    fi
    # `quarantine_frames` is the only sink for a frame whose invalidation was
    # never acknowledged, so its log line is a leak: the lane fails on any
    # teardown that had to retire frames that way. The reaper's own give-up path
    # is the same outcome one step later.
    if grep -qaiF "[aspace] quarantining" "$LOG"; then
        echo "FAIL: a domain teardown quarantined frames instead of releasing them" >&2
        grep -aiF "[aspace] quarantining" "$LOG" | head -5
        exit 1
    fi
    if grep -qaiF "[aspace] deferred release abandoned" "$LOG"; then
        echo "FAIL: the deferred reaper abandoned a tag instead of releasing it" >&2
        grep -aiF "[aspace] deferred release abandoned" "$LOG" | head -5
        exit 1
    fi
    if grep -qaiF "[selftest] DOMAIN-FRAME-RELEASE: DEFERRED" "$LOG"; then
        echo "FAIL: a domain teardown deferred its release on a one-PE boot, where no remote hart can be unacknowledged" >&2
        grep -aiF "[selftest] DOMAIN-FRAME-RELEASE" "$LOG" | head -5
        exit 1
    fi
fi
if [[ "$DEVELOPMENT_SILO" == "1" ]] \
    && grep -qia "\[silo\].*failed\|\[silo\].*fault\|\[silo\].*reset" "$LOG"; then
    echo "FAIL: development Silo fault detected during aarch64 test-hooks boot" >&2
    grep -ai "\[silo\].*failed\|\[silo\].*fault\|\[silo\].*reset" "$LOG" | head -20
    exit 1
fi


# Verify core test-hooks markers
REQUIRED_MARKERS=(
    "vfs-lifetime self-test PASS"
    "stack-probe self-test PASS"
    "stack-sizing policy self-test PASS"
    "admission-core self-test PASS"
    "ATOMIC_PUBLICATION_ARMING: PASS"
    "ATOMIC_PUBLICATION_AP-15: armed for trusted init"
    "[vfs-test] ALL TESTS PASSED"
    "S22-AARCH64-SAS-FASTPATH: PASS"
    "S22-AARCH64-PLAN: PASS"
    "S22-AARCH64-RESUME-ROOT: PASS"
    "S22-AARCH64-PIN-DYING: PASS"
    "S22-AARCH64-ROOT-SWITCH: PASS"
    # Phase 02 private-root invalidation. `LEAF-NONG` and `RELEASE-FLUSH` are
    # positive assertions (every private-root leaf carries PTE_nG; releasing a
    # root invalidates its tag and not every context, with a live counter
    # control). `ASID-INVALIDATION` asserts the behavioural witness *ran* and
    # reported a verdict: it prints `PASS` only in an environment that scopes
    # `tlbi aside1is` to the named ASID and honours the global bit, and
    # `UNPROVEN` (with the environment's exact failure) otherwise — QEMU 8.2.2
    # retires an unrelated tag's entry on `aside1is`, so it prints `UNPROVEN`.
    # A regression to the old global-leaf composition would print `FAIL` and
    # lose the marker.
    "S22-AARCH64-LEAF-NONG: PASS"
    "S22-AARCH64-RELEASE-FLUSH: PASS"
    "S22-AARCH64-ASID-INVALIDATION:"
)
# Phase 02 Tier-2 entry on one PE, plus the admission posture it reopened. These
# are asserted only on the EL1 machine: the development-Silo machine boots with
# `virtualization=on`, where the AArch64 EL2 switch has no root argument, so it
# can neither enter a private root nor enable the admission posture.
if [[ "$DEVELOPMENT_SILO" != "1" ]]; then
    REQUIRED_MARKERS+=(
        # `ENABLED` is the dev-profile decision `enable_for_boot` now makes on
        # AArch64 test images; the four assertions after it are that the
        # *closed* posture still denies and that the single publication point
        # refuses a domain-class launch outright — reopening the gate did not
        # weaken the refusal path the fleet profile depends on.
        "Tier 2 admission: ENABLED (development profile)"
        "S22-AARCH64-ADMISSION-ENABLED: PASS"
        "S22-AARCH64-ADMISSION-DENY: PASS"
        "S22-AARCH64-ADMISSION-DRAIN: PASS"
        "S22-AARCH64-ADMISSION-PUBLICATION-DENY: PASS"
        "S22-AARCH64-ADMISSION-CEILING: PASS"
        # The launch, from the boot order.
        "Init: tier2-smoke admitted."
        "Init: tier2-exploit admitted."
        # The real domain: admitted to a private root, executed to completion,
        # and observed from the kernel side while its own `TTBR0_EL1` was live.
        "[domain] admitted cell 'tier2-smoke' to Tier 2 Paged Domain (TTBR0 isolation)"
        "[tier2-smoke] PASS: All Tier 2 runtime invariants verified successfully!"
        "S22-AARCH64-DOMAIN-LIVE: PASS"
        # Fault containment: a domain cell's deliberate store to NULL kills the
        # cell and nothing else. The count, the address and the boot's survival
        # of it are asserted above.
        "[tier2-exploit] deliberately writing to NULL (0x0) — expect Page Fault termination"
        # Teardown: the retired domain's root and pages went back to the
        # allocator, and nothing was withheld. `releases=` is part of the
        # requirement: it is the targeted-invalidation delta that proves the
        # reading was taken *after* a root was actually retired, not merely after
        # a domain was switched away from.
        "[selftest] DOMAIN-FRAME-RELEASE: PASS"
        "S22-AARCH64-DOMAIN-TEARDOWN: PASS releases="
        # Phase 03 domain grant lifecycle on this architecture. The kernel-side
        # fixtures drive the production `handle_syscall` entry points with two
        # real `TaskAddressSpace::Domain` tasks, so every property below is an
        # ABI observation: the owner's backing is supervisor-only in the SAS root
        # and RW+NX in its own root; ReadOnly resolves R+NX and ReadWrite RW+NX;
        # write-only and foreign peers are refused; revoke removes both PTEs and
        # refuses a re-slice; a failed second page undoes the first; a retired
        # root keeps the alloc/slice sentinels; and the post-revoke record
        # refuses a receiver slice, an owner slice and a re-share.
        #
        # This is one PE, so a deferred invalidation is not a legitimate outcome
        # here (no remote hart can be unacknowledged) — the lane already fails a
        # deferred domain release above, so `-SLICE-RW`/`-REVOKE` are required in
        # their first-attempt form rather than the RV64 lane's invariant form.
        "S22-AARCH64-GRANT-REVOKE-OWNER-MAPPED: PASS"
        "S22-AARCH64-GRANT-REVOKE-OWNER-SLICE: PASS"
        "S22-AARCH64-GRANT-REVOKE-SLICE-RO: PASS"
        "S22-AARCH64-GRANT-REVOKE-SLICE-RW: PASS"
        "S22-AARCH64-GRANT-REVOKE-WO-REFUSED: PASS"
        "S22-AARCH64-GRANT-REVOKE-FOREIGN-PEER: PASS"
        "S22-AARCH64-GRANT-REVOKE-REVOKE: PASS"
        "S22-AARCH64-GRANT-REVOKE-FRAME-REUSE: PASS"
        "S22-AARCH64-GRANT-REVOKE-PARTIAL-MAP: PASS"
        "S22-AARCH64-GRANT-REVOKE-DEAD-ROOT: PASS"
        "S22-AARCH64-GRANT-REVOKE: PASS"
        "S22-AARCH64-GRANT-GATE-ALLOC: PASS"
        "S22-AARCH64-GRANT-GATE-REGISTER: PASS"
        "S22-AARCH64-GRANT-GATE-WO: PASS"
        "S22-AARCH64-GRANT-GATE-SHARE: PASS"
        "S22-AARCH64-GRANT-GATE-SLICE: PASS"
        "S22-AARCH64-GRANT-GATE-RETIRED: PASS"
        "S22-AARCH64-GRANT-GATE-SAS: PASS"
        "S22-AARCH64-GRANT-GATE-RETIRE-REFUSAL: PASS"
        "S22-AARCH64-GRANT-GATE-FRAMES: PASS"
        "S22-AARCH64-GRANT-GATE: PASS"
        # One PE: the revoke must be acknowledged on its first attempt, so the
        # recorded outcome is part of the requirement rather than informational.
        "S22-AARCH64-GRANT-GATE-RETIRE-OUTCOME: COMPLETED"
        # Phase 03 step-5: the *pair*, end-to-end on this architecture. The
        # kernel-side fixtures above drive the production `handle_syscall` entry
        # points with synthetic domain tasks; these two cells are real
        # `PROTECTION_CLASS_UNTRUSTED` private roots launched through the real
        # `SpawnFromPath` edge, so what follows is the same lifecycle observed from
        # the outside: the owner allocates through both entry points and proves its
        # own mapping; a foreign peer and a WriteOnly domain share are refused; it
        # publishes the handoff; five receiver generations take a ReadWrite and a
        # ReadOnly slice, are downgraded in place, and are revoked by `GrantFree`,
        # `GrantUnregister` and the owner's exit. Each generation ends in a store to
        # the address the owner handed over, which must fault — those five faults
        # are classified to the handoff ids above, and their announcements are
        # asserted to precede them.
        #
        # There is no interactive window on this image (its own test root exits the
        # VM long before a shell prompt could be typed at), so the pair is launched
        # from the boot order by init and takes the grant ids in band: the receiver
        # asks the owner for them over the pair's own IPC protocol, because they are
        # kernel-assigned after the owner starts and no command line can carry them.
        # `app-init` launches the pair on the two reviewed init edges whose services
        # are not built into this image (`/bin/silo`, `/bin/net-broker`); the
        # compile-error guards there refuse the feature combinations that would make
        # init launch the real services at those paths.
        "Init: tier2-grant-pair owner admitted."
        "Init: tier2-grant-pair complete."
        "[domain] admitted cell 'silo' to Tier 2 Paged Domain (TTBR0 isolation)"
        "[domain] admitted cell 'net-broker' to Tier 2 Paged Domain (TTBR0 isolation)"
        "S22-AARCH64-GRANT-PAIR-OWNER-BEGIN: public Grant* owner path"
        "S22-AARCH64-GRANT-PAIR-OWNER-ALLOC: OK id="
        "S22-AARCH64-GRANT-PAIR-OWNER-REGISTER: OK id="
        "S22-AARCH64-GRANT-PAIR-OWNER-MAPPED: OK"
        "S22-AARCH64-GRANT-PAIR-OWNER-REG-MAPPED: OK"
        "S22-AARCH64-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY"
        "S22-AARCH64-GRANT-PAIR-HANDOFF id1="
        "S22-AARCH64-GRANT-PAIR-RECEIVER-BEGIN: mode=rw"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-BEGIN: mode=ro"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-BEGIN: mode=downgrade"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-BEGIN: mode=unregister"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-BEGIN: mode=exit"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-ALLOC: OK id="
        "S22-AARCH64-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY"
        "S22-AARCH64-GRANT-PAIR-OWNER-SHARE-WO: DENY"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-SLICE-RW: OK"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-RW: OK"
        "S22-AARCH64-GRANT-PAIR-OWNER-FREE: OK"
        # The revoked id must not resolve again: the frames the free returned stay
        # private to the owner rather than being handed to another receiver.
        "S22-AARCH64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED"
        # Phase 2: a ReadOnly slice is readable and its store must fault.
        "S22-AARCH64-GRANT-PAIR-RECEIVER-SLICE-RO: OK (read "
        # Phase 3: the same recipient is downgraded ReadWrite → ReadOnly in place.
        # `DOWNGRADE-READ` is the "not merely unmapped" half — the byte written
        # through the writable mapping is still readable afterwards.
        "S22-AARCH64-GRANT-PAIR-RECEIVER-DOWNGRADE-RW: OK"
        "S22-AARCH64-GRANT-PAIR-OWNER-DOWNGRADE-RESHARE: OK"
        "S22-AARCH64-GRANT-PAIR-RECEIVER-DOWNGRADE-READ: OK (read 0xa5)"
        # Phase 4: `GrantUnregister` revokes the persistent buffer.
        "S22-AARCH64-GRANT-PAIR-OWNER-UNREGISTER: OK"
        # Phase 5: the owner's exit revokes the mapping the receiver still holds.
        "S22-AARCH64-GRANT-PAIR-OWNER-EXIT: OK"
        # The pair's terminal: the owner is the only half that survives its phase,
        # so it is the marker the lane treats as the pair's verdict.
        "S22-AARCH64-GRANT-PAIR-OWNER: PASS"
    )
    # The marker prefix is architecture-honest: an AArch64 boot that emitted an
    # RV64-tagged grant marker would satisfy a lane's requirement with another
    # architecture's evidence, so it is refused outright.
    if grep -qaE 'S22-RV64-GRANT' "$LOG"; then
        echo "FAIL: an AArch64 boot emitted an RV64-tagged grant marker:" >&2
        grep -aE 'S22-RV64-GRANT' "$LOG" | head -5 >&2
        exit 1
    fi
fi
if [[ "$DEVELOPMENT_SILO" == "1" ]]; then
    REQUIRED_MARKERS+=(
        "[silo] DEV_REFERENCE ready and registered; accepting only live KMS"
        "[kms] DEV_REFERENCE Silo TLS signature self-verified"
        "[silo-test] PASS: direct live Silo purpose frame denied"
        "[silo-test] PASS: no direct Silo or unbound KMS signing path remains"
    )
fi


ALL_PASSED=1
for marker in "${REQUIRED_MARKERS[@]}"; do
    if ! grep -Fq "$marker" qemu-aarch64-test-hooks.log; then
        echo "FAIL: missing required test marker: '$marker'" >&2
        ALL_PASSED=0
    fi
done

if [[ $ALL_PASSED -eq 0 ]]; then
    echo "FAIL: test-hooks assertions failed" >&2
    tail -40 qemu-aarch64-test-hooks.log
    exit 1
fi

echo "PASS: AArch64 test-hooks self-tests passed (semihosting enabled)"
cat qemu-aarch64-test-hooks.log | grep -E "PASS|spawned init"
exit 0
