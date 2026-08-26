---
phase: 3
title: "Evidence and Closure Gates"
status: blocked
priority: P0
effort: "1d"
dependencies: [2, 4]
tier: thinking
---

# Phase 3: Evidence and Closure Gates

> **Required — deviation-log:** Log every Decision / Deviation / Surprise immediately in § Deviation Log.

## Overview

Prove the stale writable translation is gone where the runtime supports it, and leave unsupported lanes honestly blocked. Phase 4 satisfies the RV64 QEMU mapping prerequisite; this phase is blocked because real RV64 hardware remains `HOST-GATED` and the AArch64/x86_64 hardware/runtime gates remain open.

## Requirements

- Functional: produce QEMU SMP stale-write evidence and define hardware evidence before security closure.
- Functional: require actual physical boot hart ID, target secondary physical hart ID, logical roles, HSM mode/state, and two distinct harts before accepting RV64 QEMU evidence.
- Non-functional: compile/single-core boot cannot close this finding; no synthetic proof substitutes for stale-TLB runtime behavior.

## Architecture

Data flow: Phase 4 logs physical/logical hart mapping and HSM mode -> kernel-internal test hook primes a stale writable translation on one physical hart -> another distinct physical hart unmaps/reuses/lowers the VA -> completion marker -> first hart retries -> physical-byte oracle proves no post-completion write -> negative control proves stale write without RFENCE -> report updates only passed lanes.

## Assumptions

- **Claim:** A new kernel-internal hook under the existing `test-hooks` feature can coordinate logical boot and `HART_RT` without public affinity/ABI changes after Phase 4 maps them to distinct physical harts.
  **Confidence:** medium
  **How to verify:** prove both participants log their actual hart IDs; do not use `SpawnPinned`, which currently rejects nonzero `core_id`.

## Related Files

- Create: `tests/integration/tests/wx-cross-hart-tlb.rs`; keep `wx-text-write.rs` as the unchanged regression oracle
- Create: `tests/integration/src/qemu-rv64-smp.rs` and re-export its named `-smp 2` runner from `tests/integration/src/lib.rs`
- Create: `kernel/src/memory/tlb_shootdown_selftest.rs` under existing `test-hooks`
- Modify: `scripts/build-test-hooks-ci.sh` or the existing test-hooks build lane
- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`
- Modify: `docs/system-architecture.md`
- Create: `.agents/reports/wx-cross-hart-tlb-shootdown-<date>.md`

## Implementation Steps

1. Add a named RV64 runner with literal `-smp 2`; fail unless the guest logs actual boot physical hart ID, target secondary physical hart ID, logical roles, HSM mode/state, and two distinct physical harts.
2. Under existing `#[cfg(feature = "test-hooks")]`, add a kernel-internal rendezvous that places actors directly on hart 0 and `HART_RT`; do not depend on `SpawnPinned` or add a syscall.
3. Exercise the actual reuse hazard: prime a writable translation for a predecessor VA/frame, unmap and reuse on the other hart, lower permissions, then publish an invalidation-complete sequence number.
4. Retry the write after completion and compare the physical byte/hash before and after; a later fault marker alone is not a pass. Record any write during the intentionally writable load window separately as outside this shootdown claim.
5. Add a bypass only inside existing `test-hooks`; the negative lane must fail the oracle. If QEMU globally flushes and the control cannot fail, label QEMU evidence `INCONCLUSIVE`, not PASS, and keep hardware closure mandatory.
6. Re-run existing `wx-text-write` 2/2 and boot suite as regression coverage.
7. AArch64 can pass only with two active Cellos PEs and the same oracle; current no-op non-RV SMP means `-smp 2` alone is `RUNTIME-GATED`. Keep x86_64 blocked until SMP plus LAPIC ICR/vector support exists.
8. Run the same oracle on real RV64 SMP/OpenSBI hardware, x86_64 SMP hardware, and AArch64 SMP hardware for every supported lane; record exact commands, image hash, CPU IDs, sequence/ack markers, iteration count, and logs.
9. Update specs/living docs only per passed lane; the overall plan cannot be `completed` while a supported architecture remains hardware/runtime-gated.
10. Record the exact host command: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test wx-cross-hart-tlb -- --nocapture`; use the named runner's logged QEMU argv as the guest command evidence, and run the RV64 proof at least 5 times.

## Success Criteria

- [x] RV64 QEMU `-smp 2` stale-write proof passes repeatedly and records command, image hash, firmware strings, actual physical hart IDs, logical roles, HSM mode/state, and logs.
- [x] The RV64 negative control fails the oracle, or QEMU is explicitly `INCONCLUSIVE`; no silent single-hart fallback is accepted.
- [x] Existing single-hart W^X regression still passes.
- [ ] Hardware evidence uses the same reuse/content oracle on RV64 SMP, x86_64 SMP, and AArch64 SMP for every supported paged architecture.
- [x] Any unrun lane is labelled `HOST-GATED` or `RUNTIME-GATED`, never `PASS`.
- [x] Rollback instructions are included in the evidence report.

## Security Considerations

The test must prove no write commits after the lowering point. A mere page fault eventually, without checking memory contents and ordering markers, is insufficient.

## Risk Notes

- Risk: QEMU scheduling hides the race. Mitigation: use repeated runs, preemption stress, and a negative lane.
- Risk: QEMU boots a nonzero physical hart or lacks HSM-startable secondaries. Mitigation: Phase 4 classified HSM-supported vs all-harts-started firmware; keep unsupported lanes gated until actual hardware evidence is recorded.
- Risk: test requires SMP placement not available outside RV64. Mitigation: record the gate; do not implement cross-arch SMP as part of this plan.
- Risk: a test bypass leaks into production. Mitigation: compile it only under existing `test-hooks` and grep the release build/source path to prove the bypass is absent.
- Rollback: remove the new/extended test-hooks lane and evidence report; keep production RFENCE rollback from Phase 2 if runtime evidence fails.
- Irreversible: none.

## Deviation Log

- 2026-08-08 — Added a test-hooks-only two-hart physical-byte oracle and negative control. QEMU was invoked with literal `-smp 2`, but bundled OpenSBI reported `HSM Device: ---`, chose boot hart 1, and rejected `sbi_hart_start(1)` with `SBI_ERR_INVALID_PARAM`; test reports `RUNTIME-GATED`, not PASS.
- 2026-08-08 — Amended after narrow authorization: RV64 evidence now depends on Phase 4 physical/logical hart mapping and HSM/all-started startup classification.
- 2026-08-09 — Phase 4 closure recorded after the RV64 QEMU five-boot proof passed again with the recorded image hash; Phase 3 is blocked on real RV64 hardware and the AArch64/x86_64 hardware/runtime gates.
- 2026-08-09 — Reviewer verdict: BLOCKED. RV64 QEMU sub-lane passed 5/5 and `wx-text-write` passed 2/2; real RV64 hardware remains HOST-GATED, AArch64/x86_64 remain RUNTIME-GATED, and the current `test-hooks` hash is `fa2bd721dbbbb73dc3a85b0c3161815cb63e08933f600a347503fc0c8e685b09`.
