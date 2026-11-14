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

None.
