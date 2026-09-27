---
phase: 8
title: "Run cross-architecture acceptance and correct architecture claims"
status: pending
priority: P1
effort: "1 day plus hardware availability"
dependencies: [2, 3, 4, 5, 6, 7]
tier: thinking
---

# Phase 08: Integration / release gate

## Requirements / architecture
Make the tested software posture and the documented product claim agree. `docs/specs/22-native-domain-cell-implementation-gate.md:3-6` claims multi-arch production while `:21-31,201-257` still describes future/default-off and explicitly limits development; current boot policy is enabled only for development (`kernel/src/main.rs:1016-1045`). Spec 15 permits kernel mechanisms but identifies MMC/ECAM/snapshot orchestration residue; do **not** migrate those drivers as collateral fixes in this plan. `docs/system-architecture.md:616-623,756-761` documents allocator single-region loss and memory budget; replace only with measured outcomes.

## Related files
`docs/{system-architecture.md,specs/02-memory.md,specs/03-runtime.md,specs/17-ipc-wire-contract.md,specs/22-native-domain-cell-implementation-gate.md,project-changelog.md}`, `CHANGELOG.md` (preserve unrelated user edits), `scripts/{qemu-native-domain-test.sh,check-hal-boundaries.sh}`, target-specific QEMU tests/harnesses and CI matrix, `tests/integration/tests/{tier2_fault_isolation.rs,launch-profile.rs}`.

## Implementation steps
1. Build **fresh** debug/test-hooks and production-profile images per RV64 1/2-hart, AArch64 **one PE** and x86 **one CPU** with PCID-on/off; for non-RV64 multi-CPU images assert Tier-2 admission denies until per-CPU state and remote ack have their own evidence. Record compile tuple, exact artifact hash, QEMU CPU model, test terminal and per-arch root state. Expand CI runners for real syscall grant owner+receiver, permission-negative, revoke/reuse and heap fragmentation tests, not only old `domain_grant` selftest.
2. Run host allocator/format unit tests and `scripts/check-hal-boundaries.sh`; run real boot-to-shell and Tier-2 null/peer/kernel memory isolation, malformed/admission fail-closed, copied IPC, grant rights/revocation on each enabled arch. Run RV64 2-hart TLBI/RFENCE race and RT wake; explicitly test no-PCID x86. Ensure clean off-feature build denies Tier-2 without SAS downgrade. Trigger snapshot unavailable on QEMU and actual two-boot scenario only on board with verified storage.
3. Check each Spec-22 negative row `:177-195`; record PASS / FAIL / HARDWARE-GATED (with named board/transport), never claim one architecture or selftest covers another. Run full integration suite only after per-path smoke. Review security/locking and performance; measure Tier-1 SAS no extra root writes and frame allocator pressure/boot time.
4. Update current architecture status, Spec 02/17/22, test matrix, changelog; remove contradictory stale statements about snapshot speed, Tier 2 production, trust class and dropped RAM. No ABI change; if discovered unavoidable, **stop** for both Law-1 owner checkpoints before editing `libs/api`/`libs/types`.

## Success criteria
- [ ] Every supported architecture's available test matrix passed on fresh artifact; missing physical/device witnesses explicitly block **their named claims**, not all development work.
- [ ] Tier-1 SAS, Tier-3 VM, Supervisor snapshot authority, signed spawn, boot and driver regression paths still work.
- [ ] Evidence links and docs do not say production-ready, physical qualified or warm-boot performant without the exact witness; no test-only code ships in production.

## Assumptions / risk / rollback
- [UNVERIFIED] All required emulators/physical boards are accessible to CI; where not, mark named qualification gate unresolved and retain disabled profile, not a passing placeholder. Rollback to known-safe image with domain admission and snapshot disabled; reimage development storage if a corrupted snapshot was ever replayed. Security exposure or overwritten external data cannot be rolled back by a binary revert.

## Deviation Log
None.
