---
phase: 4
title: "RV64 Physical Hart Mapping and HSM Startup"
status: completed
priority: P0
effort: "1d"
dependencies: [2]
tier: thinking
---

# Phase 4: RV64 Physical Hart Mapping and HSM Startup

> **Required — deviation-log:** Log every Decision / Deviation / Surprise immediately in § Deviation Log.

## Overview

Make the RV64 HSM `-smp 2` lane tolerant of an arbitrary physical boot hart solely to prove W^X RFENCE on two real harts. This is private to RV64's existing two-logical-hart policy; firmware that enters all harts at `_start` remains host-gated.

## Requirements

- Record the HSM boot physical hart as logical role 0 and map the other QEMU `virt -smp 2` physical hart to logical `HART_RT`.
- Translate every RFENCE/IPI target to SBI `{hart_mask: 1, hart_mask_base: physical_id}`; no SBI caller may shift a logical ID directly.
- Support only a single `_start` entrant plus an HSM STOPPED target. HSM absent, an already STARTED target, or duplicate `_start` entry is `RUNTIME-GATED`/`HOST-GATED`, never a partial all-harts-started implementation.
- A secondary must install its logical-hart trap vector and HartLocal while interrupts are disabled; no trap path may load the singleton `HART_LOCAL_TP_ADDR`.
- No public ABI, syscall, manifest, ASID, Tier-2, non-RV SMP/IPI, or generic N-hart scheduler redesign.

## Architecture

1. HSM firmware passes raw physical `a0` to `kmain`; before task/SMP initialization, Cellos records it as the physical backing for logical boot role 0.
2. The startup classifier logs every boot entry and physical 0/1 HSM state. More than one `_start` entrant is `HOST-GATED`; the supported lane selects only the other physical hart in HSM STOPPED state.
3. `task::init()` retains logical boot index 0. `smp_hart_entry(physical_a0)` translates to logical `HART_RT` before accessing logical scheduler or HartLocal state.
4. RV64 uses two logical-hart-specific `stvec` entry stubs. Each loads the fixed `HART_LOCALS[logical]` pointer before common frame save; `sscratch` remains exclusively the existing nested-safe stack protocol.
5. Logical online state remains `HART_ONLINE[logical]`. RFENCE and scheduler IPI each translate the logical target to `{mask: 1, base: physical_id}` before SBI.
6. Phase 3 logs physical IDs, logical roles, HSM state/mode, trap-vector markers, and positive/negative W^X oracle results.

**Resolved observations:** `MAX_HARTS == 2`, `HART_RT == 1`, and `HART_ONLINE` remains logical. RFENCE and scheduler IPI now use `logical_sbi_target`; the RV64 trap entry preserves user `tp` through `sscratch` and selects `HART_LOCAL_TP_ADDRS[logical]`. The QEMU lane maps either physical boot hart to logical 0 and starts only the other HSM STOPPED hart as logical 1.

## Assumptions

- **Claim:** QEMU `virt -smp 2 -bios default` has one S-mode boot entrant and one HSM STOPPED target.
  **Confidence:** medium
  **How to verify:** log boot entrants plus HSM states for physical 0 and 1 in five runs; otherwise preserve the runtime/host gate.
- **Claim:** two logical harts are sufficient for this proof.
  **Confidence:** high
  **How to verify:** keep `MAX_HARTS == 2` and reject `-smp >2` evidence claims.

## Related Files

- Modify: `kernel/src/main.rs`, `kernel/src/task.rs`, `kernel/src/task/smp.rs` — physical/logical mapping, classifier, HSM target selection, publication ordering.
- Modify: `hal/arch/riscv/src/rv64/boot.rs` — preserve HSM-secondary physical ID and emit only safe entry diagnostics; no all-harts election.
- Modify: `kernel/src/task/hart_local.rs`, `hal/arch/riscv/src/rv64.rs`, `hal/arch/riscv/src/rv64/trap.rs`, `hal/arch/riscv/src/rv64/asm/trap.S` — per-logical-hart trap-vector protocol and removal of singleton `tp` restore.
- Modify: `kernel/src/memory/tlb_shootdown.rs`, `kernel/src/task/scheduler.rs`, `hal/arch/riscv/src/common/sbi.rs` — mapping-only SBI RFENCE/IPI targets.
- Modify: `kernel/src/memory/tlb_shootdown_selftest.rs`, `tests/integration/src/lib.rs`, `tests/integration/tests/wx-cross-hart-tlb.rs` — identity/mode assertions and five-run evidence.
- Create: `.agents/reports/wx-rv64-physical-hart-hsm-startup-<date>.md`.

## Implementation Steps

1. Add private RV64 mapping helpers: `set_boot_physical_hart`, `logical_to_physical`, `physical_to_logical`, `logical_sbi_target -> (mask, base)`, and `remote_online_sbi_targets`. Reject physical IDs outside this QEMU two-hart lane.
2. In `kmain`, record raw boot physical `a0` before `task::init`; preserve scheduler role 0. Count boot entries only to classify firmware: a second `_start` entrant emits `HOST-GATED` and the two-hart test cannot proceed.
3. Query physical 0 and 1. Start exactly the non-boot target when—and only when—HSM reports STOPPED. HSM missing, STARTED target, unexpected status, or a `hart_start` error keeps the W^X lane gated.
4. Replace global trap restoration with two logical-hart `stvec` entry stubs: each loads the matching fixed `HART_LOCALS` pointer before shared trap-frame work. Remove `HART_LOCAL_TP_ADDR` and its `trap.S` load; keep `sscratch` stack-only.
5. Install the logical trap vector and HartLocal with interrupts disabled on boot and secondary. Only then enable SSIE/STIE, arm the timer, log physical/logical identity and trap-vector marker, then Release-store `HART_ONLINE[logical]`.
6. Route RFENCE and scheduler IPI through `logical_sbi_target`, passing `{mask: 1, base: physical_id}`. Grep-review every SBI RFENCE/IPI call site and ban direct `1 << logical`.
7. Use Release when publishing mapping/HSM classification and Acquire for mapping/online consumers. Compute the RFENCE target only after those Acquire reads.
8. Extend the self-test to reject non-distinct physical participants and require HSM state/mode, logical roles, trap-vector markers, positive content oracle, and negative stale-write oracle.
9. Run the exact QEMU `-smp 2` proof at least five times. Record QEMU argv/version, firmware strings, image hash, physical/logical IDs, HSM states, trap markers, and result per iteration.

## Test Matrix

- Compile: RV64 release and `test-hooks` check; grep release source/build for no new public ABI and no `HART_LOCAL_TP_ADDR` trap load.
- Integration: `scripts/build-test-hooks-ci.sh`, then `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test wx-cross-hart-tlb -- --nocapture`.
- Positive: one boot entrant, STOPPED target, two distinct physical harts, logical 0/`HART_RT`, both trap markers, RFENCE PASS, unchanged content oracle.
- Negative: test-hooks RFENCE bypass changes the physical word; otherwise label QEMU `INCONCLUSIVE`.
- Gates: all-harts-started firmware and real RV64 hardware are `HOST-GATED` unless actually run; AArch64/x86_64 stay separately gated.

## Success Criteria

- [x] Logs identify distinct boot/secondary physical IDs and map them to logical 0/`HART_RT`.
- [x] HSM-started and all-harts-started firmware are distinguished; the latter is `HOST-GATED`, not partially supported or passed.
- [x] No secondary publishes online before its trap vector/HartLocal are installed; `trap.S` has no `HART_LOCAL_TP_ADDR` load.
- [x] RFENCE and IPI use `{mask: 1, base: physical_id}` via the mapping; no SBI caller shifts a logical ID directly.
- [x] Phase 3 positive and negative oracle passes five times on two distinct harts.
- [x] No excluded ABI, scheduler-redesign, non-RV, ASID, or Tier-2 files are changed.

## Security Considerations

This prevents a false W^X closure: `-smp 2` alone proves nothing without two physical IDs, correct SBI targeting, and a negative control that can still write when RFENCE is bypassed.

## Risk Notes

- All-harts-started firmware can race relocation/BSS. It is out of scope and host-gated; no partial election is allowed.
- A singleton trap restore corrupts cross-hart attribution. Fixed `stvec` stubs restore `tp` before common save while `sscratch` stays stack-only.
- A logical SBI shift can miss the remote hart. Central single-target `{mask: 1, base}` translation and a call-site grep gate prevent it.
- Firmware can be misclassified. Only one boot entrant plus STOPPED target permits the QEMU lane.
- Rollback: revert boot mapping/classifier, trap-vector protocol, RFENCE/IPI target translation, and Phase 3 assertions as one unit, then restore Phase 3's prior `RUNTIME-GATED` state.

## Deviation Log

- 2026-08-08 — Added after QEMU evidence showed physical boot hart 1 and `hart_start(1)` failure; remains a prerequisite for Phase 3, not a generic SMP redesign.
- 2026-08-08 — Red-team revision: all-harts-started support removed from scope; SBI base semantics, scheduler IPI, and concrete per-hart trap-vector ordering made mandatory.
- 2026-08-08 — Surprise: HSM secondaries entered with `satp=0`; installed the published kernel root and SUM before HartLocal/trap activation. The five-run oracle then passed.
- 2026-08-08 — Review fix: `ARCH.init()` restored the boot trap vector; secondary now reinstalls its logical vector immediately afterward, before interrupt enable/online publication.
- 2026-08-09 — Closure evidence: commit `7ee86d55` contains the Phase 4 implementation; the exact QEMU integration command passed again over five internal boots, and the release image SHA-256 matched `b9833cd9a1902627ad8bde24430eaa42f10ea862c5e22388cb9582d7f4be4a1e`. Reviewer verdict remained CLEAR after the secondary `stvec` ordering fix.
