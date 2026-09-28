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

# Fault containment is an asserted outcome, not a forbidden one: exactly one
# cell fault, it must be the announced NULL store `tier2-exploit` performs, and
# the boot must carry on past it. Any other `[fault] Cell` line — a second one,
# another address, an unannounced fault — still fails the lane. The EL2
# development-Silo machine witnesses no Tier-2 entry at all, so there it stays a
# blanket prohibition.
#
# The cells are launched from the boot order (init), not typed at the shell: this
# image's shell sleeps ~2 s before its first prompt and its own test root exits
# the VM at ~4.5 s, so a prompt-driven sequence cannot run at all. Containment is
# therefore read off the boot instead: the deliberate fault must be announced by
# its launcher first, and the boot's terminal marker must come *after* it — the
# kernel, init and the remaining test root all survived a domain cell's fault.
if [[ "$DEVELOPMENT_SILO" == "1" ]]; then
    if grep -qaiE '\[fault\] Cell' "$LOG"; then
        echo "FAIL: cell fault detected during the AArch64 development-Silo boot" >&2
        grep -ai "\[fault\] Cell" "$LOG" | head -20
        exit 1
    fi
else
    FAULT_LINES="$(grep -aiE '\[fault\] Cell' "$LOG" || true)"
    FAULT_COUNT="$(printf '%s\n' "$FAULT_LINES" | grep -c . || true)"
    if [[ "$FAULT_COUNT" != "1" ]]; then
        echo "FAIL: expected exactly one contained cell fault, found $FAULT_COUNT:" >&2
        printf '%s\n' "$FAULT_LINES" >&2
        exit 1
    fi
    if ! printf '%s\n' "$FAULT_LINES" | grep -aqE 'terminated: cause=0x[0-9a-f]+ pc=0x[0-9a-f]+ addr=0x0$'; then
        echo "FAIL: the single cell fault was not the announced NULL store: $FAULT_LINES" >&2
        exit 1
    fi
    if ! grep -aqF "[tier2-exploit] deliberately writing to NULL (0x0) — expect Page Fault termination" "$LOG"; then
        echo "FAIL: the contained fault was not preceded by tier2-exploit's announcement" >&2
        exit 1
    fi
    # Byte offsets make the ordering a property of the boot, not of the grep. The
    # anchor is the kernel's own admission line, which is printed inside the
    # publication path — before the cell can run. (init's own "admitted" print
    # lands *after* the spawn syscall returns, so a cell that runs immediately can
    # fault before it: measured, 174 bytes earlier.)
    offset_of() { grep -aboF -- "$1" "$LOG" | head -1 | cut -d: -f1 || true; }
    ADMIT_OFFSET="$(offset_of "[domain] admitted cell 'tier2-exploit' to Tier 2 Paged Domain")"
    FAULT_OFFSET="$(offset_of '[fault] Cell')"
    TERMINAL_OFFSET="$(offset_of '[vfs-test] ALL TESTS PASSED')"
    if [[ -z "$ADMIT_OFFSET" || -z "$FAULT_OFFSET" || -z "$TERMINAL_OFFSET" ]]; then
        echo "FAIL: missing admission/fault/terminal line (admit=$ADMIT_OFFSET fault=$FAULT_OFFSET terminal=$TERMINAL_OFFSET)" >&2
        exit 1
    fi
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
    )
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
