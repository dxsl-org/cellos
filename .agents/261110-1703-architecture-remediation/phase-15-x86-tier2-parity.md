---
phase: 15
title: "x86-64 Tier-2 parity"
status: in-progress
priority: P1
effort: "7–10d"
dependencies: [14]
tier: thinking
checkpoint: false
---

# Phase 15: x86-64 Tier-2 parity

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-10](../reports/architecture-review-261110.md) · `kernel/src/task/domain_grant.rs` · `kernel/src/task.rs` · `kernel/src/task/syscall.rs` · `kernel/src/memory/address_space.rs` · `kernel/src/task/user_copy/` · `kernel/src/loader/domain_admission.rs` · [Spec 22](../../docs/specs/22-native-domain-cell-implementation-gate.md) · [ADR-0022](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md)

## Overview

Bring the active architecture up to the Tier-2 feature and fixture level that today exists only on RV64/AArch64, so Tier-2 C/C++ consumers on Intel are not limited to copied IPC and have regression fixtures.

## Key Insights

- Reviewer-cited: domain grants compile only for RV64 and AArch64 test images (`task.rs:14-21`, `syscall.rs:385-393`); single-arch fixture gates in memory/loader/user-copy/domain code number 41 riscv64, 12 aarch64, 6 x86_64; the fault-guard header documents only the RV64 trap path (`user_copy/guard.rs:1-4`).
- x86 domain admission is qualified in code (`loader/domain_admission.rs:150-158`) although Spec 22 still says "refused".
- Audit H-5 (publish/revoke race) was fixed with a per-record gate; the port must carry that gate, not the pre-fix shape.

## Requirements

- Functional: domain grant publish/map/revoke on x86-64 with the same record gate and deferred-release contract as RV64; negative fixtures ported (revoke while mapped, receiver death, stale grant id, cross-domain forgery, fault during copy); user limit sourced from the HAL instead of `1<<38` literals.
- Non-functional: no new syscall numbers (the grant syscalls exist); no relaxation of admission; evidence ceiling x86 QEMU TCG + KVM.

## Architecture

Uses Phase 14's `arch::page_probe` / `user_access` and `AddressSpace` (already arch-generic over `hal::PageTableTrait`). The x86-specific parts are PCID/ASID lease handling and the shootdown path for private roots, which exist for Tier-2 code pages today.

## Assumptions

- **Claim:** the RV64 grant code is arch-neutral above the page-table trait, so the port is mostly removing cfg gates plus x86 shootdown wiring.
  **Confidence:** low
  **How to verify:** read `domain_grant.rs` for RV64-specific calls before estimating.
- **Claim:** x86 private-root invalidation supports the probe-then-defer contract (`deferred_release.rs`).
  **Confidence:** medium
  **How to verify:** read `tlb_shootdown.rs` x86 private-root branch.
- **Claim:** existing x86 Tier-2 witness cells (`cells/tests/tier2-grant-owner`, `tier2-grant-receiver`) build for x86_64.
  **Confidence:** low
  **How to verify:** `cargo check -p tier2-grant-owner --target x86_64-unknown-none` per the lane's flags.

## Related Code Files

- Modify: `kernel/src/task/domain_grant.rs`, grant syscall arms, `kernel/src/task.rs` (module gates), `kernel/src/memory/address_space.rs`, `kernel/src/task/user_copy/{range,guard}.rs`, `scripts/build-x86_64-domain-test-ci.sh`, `scripts/x86/qemu-domain-test.sh`, `scripts/lanes.toml`, `docs/specs/22-native-domain-cell-implementation-gate.md`
- Create: x86 variants of the RV64 grant fixtures under the existing test-hooks modules; `reports/phase-15-evidence.md`

## Implementation Steps

1. Inventory RV64-only gates in the grant path; classify each as neutral, needs-HAL, or genuinely RV64.
2. Replace literals with the HAL user limit; remove gates classified neutral; build x86.
3. Wire x86 private-root invalidation into grant revoke with probe-then-defer.
4. Port negative fixtures one by one; each must fail when its guarded check is disabled (fault-injection control already used on RV64).
5. Extend the x86 domain test script to run the grant suite at 1, 2 and 4 CPUs.
6. Update Spec 22 status for x86.

## Todo List

- [ ] Gate inventory
- [ ] Neutral code un-gated
- [ ] x86 revoke invalidation
- [ ] Fixtures ported with fault-injection controls
- [ ] Lane extended
- [ ] Spec 22 updated

## Success Criteria

- [ ] x86 grant suite passes at 1/2/4 CPUs under TCG and, where available, KVM.
- [ ] Each ported negative fixture fails when its corresponding check is disabled by the test hook.
- [ ] RV64 `--harts 2` native-domain suite still passes.
- [ ] No `1 << 38` user-limit literal remains outside the RV64 HAL.

## Risk Assessment

- Shared memory between domains is the place isolation bugs become cross-domain corruption; the fixtures-with-controls rule is the safeguard. Rollback: re-gate the x86 path (feature-level revert), leaving copied IPC as today.
- KVM may be unavailable on the runner: record TCG as the ceiling and say so.

## Security Considerations

Carries the audit H-5 gate. Receiver mappings must be removed and flushed on every CPU before frames can be reused; timeout defers, never frees.

## Next Steps

Phase 16 unifies frame lifetime, including the grant path ported here.

## Deviation Log

- **Decision (D1, owner, 2026-11-14):** x86-64 *production* gets the domain-grant lifecycle, consistent with x86 production admission (`switch_ordering_qualified`). One cfg, `cellos_domain_grants` = `native-domains ∧ cellos_wide ∧ ¬(aarch64 ∧ ¬test-hooks)`, emitted by `kernel/build.rs` from `posture::domain_grants` (host-tested), replaces 29 per-site predicates (the `cfg!` switch included) and the two fixture gates. RV64/AArch64 gating is unchanged; the AArch64-production const pin stays. The `target_arch` count outside `kernel/src/arch/` drops 775 → 709 at `2216f05719` (that commit set the ceiling to 714) and to 704 at `4e623fa201` (ceiling 704).
- **Decision (D2, owner):** the x86 `USER_LIMIT` value stays `1 << 38`; it now comes from the HAL. Widening it toward the canonical `1 << 47` is not part of this phase.
- **Decision (D3, owner):** every 64-bit arch's `USER_LIMIT` is a HAL constant (`hal::paging::USER_LIMIT`), values unchanged. The bare literal remains only in the RV64 HAL (Sv39 lower half). AArch64 derives it from its own boot regime, `1 << (DEFAULT_VA_BITS - 1)` with `DEFAULT_VA_BITS = 64 - TranslationConfig::default_39bit().t0sz`. x86 has no natural `2^38` term, so it is the named lower half of PML4 slot 0, `PML4_SLOT_SPAN / 2` with `PML4_SLOT_SPAN = 1 << 39`, documented as matching RV64's window. Kernel sites: `address_space.rs`, `user_copy/range.rs`, `loader/image-va.rs` (64-bit arm now `cellos_wide`), `loader/va_alloc.rs` (static assert), `user_copy_tests.rs`. `address_space.rs:417` (`0x0000_8000_0000_0000`, the supervisor-copy lower-half filter) is not a user limit and is left alone.
- **Decision (R3, owner):** no new identity or RWX mapping in the x86 kernel root. The census F3 fix (owner backing zeroed through `phys_to_virt`) ships with the cfg widening. `free_grant_pages_inner` now restores the kernel RWX alias only where `phys_to_virt` is the identity (RV64/AArch64, unchanged) — the rule `release_shm_for` already followed. On x86 a freed grant frame (domain *or* SAS) is unpublished and freed with no lower-half alias, so x86 SAS grant frees also stop adding the f→f RWX alias that never existed at boot (x86 maps RAM only in the HHDM). Without the restore, the kernel-root page tables a SAS USER mapping needed become empty, so the same branch prunes them and invalidates before their frames return (the `CellSegments::drop` pattern); the first x86 run showed the gap as `S22-X86-GRANT-GATE-FRAMES: FAIL deficit=1`, the PT the SAS→SAS step had to create in the kernel root. The RV64/AArch64 RWX restore stays as Phase 16 debt. The SAS-only partial-map rollback in `alloc_grant_pages` is unchanged.
- **Deviation (fixture VAs):** the grant fixtures' private-root VAs moved from `0x0400_0000…` to `0x30_0400_0000…`. Grant pages are published at VA = PA and x86 RAM starts at 0, so the old VAs could coincide with a grant frame on x86. Fixture-only; no kernel behaviour change on any arch.
- **Deviation (fixture SAS-root assertion):** the `present && !user` checks now read `present` at the kernel alias (`phys_to_virt(base)`) and `user` at the grant address (`domain_grant::sas_root_state`). On RV64/AArch64 both are the same address, so the assertion is unchanged there; on x86 it observes the HHDM alias, where RAM has no identity alias (census F4).
- **Deviation (tier2-smoke):** `cells/tests/tier2-smoke` asserted the x86 *closed* grant posture (`GrantRegister` refused); with D1 it now runs the open-posture round trip on x86_64 too. Outside the request's file list, but a direct consequence of opening x86 grants (first x86 run: `PANIC` in tier2-smoke).
- **Surprise (step 3, census F5 confirmed):** x86 revoke needed no new invalidation mechanism. The x86 private-root tag path already probes then defers (`confirm_tag_invalidation` → `deferred_release`), so step 3 is the arch-neutral withhold-ack seam (`TEST_WITHHOLD_TAG_ACK`, previously RV64-only; AArch64 builds it but never arms it) plus the `S22-X86-GRANT-DEFERRED` fixture (`task::grant_negative_selftest`, wired on x86 only). The H-5 module (`domain-grant-gate.rs`) is untouched.
- **Deviation (step 3, lane carve-out in the same commit):** the fixture's withheld window logs `[tlb] tag … invalidation unconfirmed` by design, which the x86 lane failed on. The R5 bracket (`[grant-neg] withhold-ack window OPEN`/`CLOSED`, balanced, only `[tlb] tag` lines excused inside) ships with the fixture rather than in step 5, so every commit's lane stays green. Census §10 step 3's "neutral counter for `address_space_tests`" was not needed: the x86 fixture uses the existing arch-neutral `arch::clock::monotonic_ns`, and the RV64 fixture is left as it is.
- **Decision (step 3, R4 instrumentation):** `domain_grant::gate_hold` (test-hooks only) times every hold of a record's lifecycle gate (`publish`, `drain_receiver`) and keeps the boot maximum; the x86 fixture prints it as `[grant] gate-held max=<us>us cpus=<n>`. It records on every test-hooks image but is printed only by the x86 fixture, so RV64/AArch64 boot output is unchanged.
- **Decision (R4, measured, TCG):** before the change the longest gate hold was 1.7 ms at 1 CPU and **10.6 ms at 4 CPUs** (`/tmp/p15/c3/qemu-x86-domain-test-cpu{1,4}.log`), above "a few ms", so a `GrantShare` now drains the receiver it supersedes *before* the table lock and the publish gate (`syscall::drain_superseded_receiver`); the drain is still `serialized` by the record's gate (H-5) and `publish` re-checks the tuple. After: publish 962 µs / drain 3162 µs at 1 CPU, publish 266 µs / drain 860 µs at 4 CPUs (`/tmp/p15/c3b/…`); single runs, timings vary between boots. Applies on every arch (shared code, same observable semantics: the old receiver is drained before the new one is published, as before). Note: x86 syscalls run with IF masked (SYSCALL FMASK) end to end, so this shortens table-lock/gate contention for other CPUs, not the calling CPU's masked span. KVM measurement pending (step 5).
- **Decision (step 4, controls):** `task::fixture_controls` (compiled only with `test-hooks`; production has neither the module nor any consult site) holds one switch per guarded check, `GuardedCheck::{RevokeDrainsReceiver, DeathDrainsReceiver, SliceNeedsLive, SliceNeedsGrantee, CommitRewalksPages}`. Each is consulted at exactly the check it disables: `DomainGrant::revoke` (receiver drain), `syscall::misc::drain_domain_receiver` (receiver death only — the shared drain a re-share uses is untouched), the domain slice row's `Live` and `shared_to == caller` tests, and the domain copy commit's per-page re-walk (the control skips a page instead of refusing it, so the commit reports a copy that never happened). The x86 fixture `grant_negative_selftest` runs each negative as built (`S22-X86-GRANT-NEG-<CASE>: PASS`) and then with its one switch set (`S22-X86-GRANT-CONTROL-<CASE>: CAUGHT (<check> disabled: <what failed>)`; `MISSED` otherwise), clears the switch before returning, and runs before any cell starts.
- **Deviation (step 4, cases):** all five cases are new fixture cases rather than reuses of an existing witness, so each has a control: revoke while mapped (`REVOKE-MAPPED`), receiver death (`RECEIVER-DEATH`, the census found no explicit domain fixture), stale grant id (`STALE-ID`: a `Revoking` record is stale for the owner, the receiver and a re-share), cross-domain forgery (`FORGERY`: a third domain the grant never named), fault during copy (`COPY-FAULT`). Existing witnesses keep running unchanged (`GRANT-REVOKE-REVOKE`, `GRANT-GATE-RETIRE-REFUSAL`, `GATE-SHARE/SLICE`, `REVOKE-FOREIGN-PEER`). The copy case uses only `force_unmap_without_drain_for_test` (R7: no HHDM hole PA is needed); the staged-copy test hook is widened to x86_64 for it.
- **Deviation (step 4, lane in the same commit):** the ten NEG/CONTROL markers, `S22-X86-GRANT-NEG: PASS`, and a rule failing on any `S22-X86-…: FAIL` or `CONTROL-…: MISSED` ship with the fixture (as in step 3), so the lane proves every control from step 4 on; the pair, its fault accounting and the KVM matrix are step 5. Lane (TCG) 1/2/4 CPUs PASS with every control `CAUGHT` (`/tmp/p15/c4/`).
- **Surprise (step 4):** `kernel/residents.toml` did not list step 3's `grant_negative_selftest.rs` (`check-kernel-residents.py` failed from `4e623fa201`); it and `fixture_controls.rs` are listed as `test` residents now. `generate-code-metrics.py --check`, `check-lanes.py` (tier3 chrome/display scripts) and `mutable-globals --check` (`cell::init_death` statics) already fail at the phase base `5656697f05`; untouched here.
- **Surprise (review of steps 2–3, HIGH, pre-existing):** a domain row could be freed twice. Revoke is idempotent, `sweep_deferred_domain_grants` runs on every hart's reap unserialised, and `GrantFree` can race task exit or `CapRevoke`'s `reclaim_owned_grants`; every revoker that saw `Ok` removed the key and freed the frames. Opening x86 production SMP made it reachable there. Fix (`fix(kernel): review findings on x86 domain grants`): `syscall::misc::remove_row_if` removes a row only while it still carries the same record (`Arc::ptr_eq`), and only the caller whose removal took the row frees — `GrantFree`, both sweep passes and `release_deferred_domain_row` (whose failure branch now also clears ownership only on its own row). A key reused by a new grant over the freed frames is left alone. Host test `task::syscall::grant_row_tests` (second revoker takes nothing; a stale revoker leaves a new row at the same key).
- **Deviation (review, MED, x86 free path):** instead of a row flag, `free_grant_pages_inner` reads the kernel root: without an identity alias (x86) a run with no kernel-root leaf — a domain owner's backing, which is never published there — is only deallocated (no `unpublish_user_frames`/`flush_range`, no prune). Reading the root rather than the row also covers a domain run released later from the pin quarantine (`free_released_range`). RV64/AArch64 unchanged (`identity_alias` short-circuits). The partial-map rollback in `alloc_grant_pages` restores the RWX identity alias only where it is the identity and otherwise prunes, as the free path does (R3).
- **Deviation (review, LOW):** a forced (withheld) tag failure now logs `[tlb] tag N invalidation unconfirmed: withheld (test)` on every test-hooks image, RV64 included, and the x86 lane excuses only that line inside the window; a real `… silent through the … tick probe` line fails there too (and RV64's park evidence can no longer be satisfied by a forced line). `blk_read_async` refuses a domain-backed row. The `allow(dead_code)` on `set_test_withhold_tag_ack`/`test_tag_flush_observed` now follows the fixtures' cfg (`native-domains` ∧ RV64/x86); an x86 test-hooks kernel without `native-domains` is not buildable at all (build.rs refuses it; with `compile-only` it fails at `syscall/grant.rs` `TaskAddressSpace`, at `4e623fa201` too), so the warning could not be observed either way. The deferred fixture asserts the row (owned then ownerless, `Revoking`, then gone) and `deferred_release_frames_for(receiver tag)` instead of the global free-frame count. (`> 0` while withheld; `0` once the reaper has passed the confirmed tag — it leaves the queue one reaper pass after the row, so the fixture waits for it within its budget). `domain_row_for_test` carries an unconditional `allow(dead_code)` (only the x86 fixture reads it; a `target_arch` cfg would raise the ratchet). Checks: host 653 tests, every kernel/domain build, clippy unchanged; x86 TCG lane 1/2/4 CPUs PASS (`/tmp/p15/fix/lanes/`).
- **Deviation (step 5a, pair on x86):** the x86 domain-test image now packs the pair (`tier2-grant-owner` at `/bin/silo`, `tier2-grant-receiver` at `/bin/net-broker`) and builds its fixture launcher with `tier2-grant-pair`, as the AArch64 test-hooks image does. The lane's single-fault rule became address accounting: exactly six `[fault] Cell` lines — the NULL store once, then id1 once, id2 twice, id3 once, id4 once from `S22-X86-GRANT-PAIR-HANDOFF` — each after its announcement (byte offsets); the cause is not pinned (x86 reports `0x6` and `0x7`). It also fails on any RV64/AArch64-tagged grant marker in an x86 boot.
- **Deviation (step 5a, recovery watcher):** the watcher typed `ps` only once a prompt *followed* the NULL-store announcement; when the shell printed its prompt first (step 3's 4-CPU TCG failure, `/tmp/p15/c3/lane-cpu4.out`), nothing was typed and the lane failed. It now types once the contained NULL fault line and any prompt exist, retypes (at most three times) until the echo appears, and the witness is the `USER: ps` echo after the fault line followed by a prompt. The first draft of the new watcher died at its first empty probe (`set -e`/`pipefail` hold in the subshell); fixed before commit.
- **Decision (step 5a, matrix):** `X86_ACCEL` (`tcg|kvm`) and `X86_CPUS` already existed; CI keeps TCG at one CPU and `scripts/lanes.toml` records the local evidence matrix. Result: TCG and KVM at 1/2/4 CPUs all PASS, pair faults 1/2/1/1 plus the NULL store in every run (`/tmp/p15/c5/m/summary.txt`).
- **Decision (R4, measured, KVM):** longest gate holds under KVM (`-cpu host`, PCID on): publish 1/43/126 µs and drain 3992/4086/4245 µs at 1/2/4 CPUs (TCG same build: publish 118/213/226 µs, drain 747/825/827 µs). The drain maximum is ~4 ms at every CPU count, including one CPU, where no remote acknowledgement exists to wait for, so it is not a shootdown wait. No further change: the drain already runs under no table lock (step 3) and stays serialised by the record gate (H-5). See the next entry for its cause.
- **Surprise (R4, KVM, cause):** the ~4 ms KVM drain hold is serial output, not waiting. On the deferral path the drain logs inside the gate — the memory layer's `[aspace] deferred release queued: …` warning (~200 bytes, emitted in production too) and the test seam's `… withheld (test)` line — and KVM's emulated UART is slow per byte. A throwaway build without the test line (not committed) measured drain 3109/3336 µs at 1/4 KVM CPUs (`/tmp/p15/c5/r4/`), against 3992/4245 µs with it; the remainder is consistent with the `[aspace]` line [inference: no per-hold attribution was instrumented]. It only happens when an acknowledgement is missing (the fail-closed path). Moving that log out of the gate changes `memory/deferred_release.rs` logging for every arch, so it is left as an open item for the owner rather than changed here.
