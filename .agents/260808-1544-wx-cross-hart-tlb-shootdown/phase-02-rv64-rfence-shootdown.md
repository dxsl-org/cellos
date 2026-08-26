---
phase: 2
title: "RV64 RFENCE and Reuse Barrier"
status: completed
priority: P0
effort: "1.5d"
dependencies: [1]
tier: thinking
---

# Phase 2: RV64 RFENCE and Reuse Barrier

> **Required — deviation-log:** Log every Decision / Deviation / Surprise immediately in § Deviation Log.

## Overview

Close the RV64 SMP hole using SBI RFENCE, including the stale-translation window when a dead cell's VA/frame is reused.

## Requirements

- Functional: after W^X lowers or unmaps a cell-segment page on RV64 SMP, every other online hart must execute a subsuming `SFENCE.VMA` before the cell can run or its VA/frame can be reused.
- Non-functional: fail closed on RV64 SMP if remote fence is unavailable; no ABI changes; no x86_64/AArch64 interrupt-controller work.

## Architecture

Data flow: boot RFENCE probe -> enable SMP only if supported -> PTE store/clear -> compiler-visible store barrier + `fence rw,rw` -> local `SFENCE.VMA` -> SBI RFENCE for all online harts except `current_hart_id()` -> synchronous completion -> allow task execution or VA/frame reuse.

- OBSERVED: `HART_ONLINE[1]` is set by the secondary; hart 0 is always online but absent from the array, and W^X may execute on hart 1 (`kernel/src/task/smp.rs:21`, `kernel/src/task/hart_local.rs:146`).
- OBSERVED: current SBI wrapper has IPI and HSM but no RFENCE wrapper (`hal/arch/riscv/src/common/sbi.rs:139`).
- OBSERVED: current SBI call helper passes only `a0..a2`, while RFENCE range size requires `a3` (`hal/arch/riscv/src/common/sbi.rs:13`).
- OBSERVED: cell teardown currently unmaps locally, then frees frames/PIE VA without a remote invalidate (`kernel/src/task/stack.rs:331`, `kernel/src/task/stack.rs:349`).
- PRIOR: RISC-V privileged spec describes shootdown as local data fence, IPI/remote action, remote `SFENCE.VMA`, ack.
- PRIOR: SBI RFENCE exposes `remote_sfence_vma`; OpenSBI's TLB event waits in `tlb_sync` before returning, so prefer it over repurposing SSIP.

## Assumptions

- **Claim:** OpenSBI on the QEMU lane implements SBI RFENCE.
  **Confidence:** medium
  **How to verify:** probe RFENCE before `sbi_hart_start`, log the result, and require the QEMU `-smp 2` lane to show the secondary was enabled only after a successful probe.

## Related Files

- Modify: `hal/arch/riscv/src/common/sbi.rs`
- Modify: `kernel/src/task/smp.rs`
- Modify: `kernel/src/memory/page_protect.rs`
- Modify: `kernel/src/loader/wx.rs`
- Modify: `kernel/src/task/stack.rs`
- Create: `kernel/src/memory/tlb_shootdown.rs` as the focused internal contract/backend module

## Implementation Steps

1. Add private SBI Base-probe and RFENCE wrappers, including an RFENCE-specific four-argument ecall path that places `size` in `a3`; propagate every nonzero error.
2. Probe RFENCE before secondary startup. If absent, keep Cellos single-hart and log the security gate; never bring up hart 1 then silently fall back to local invalidation.
3. Compute `remote_mask = all_online_harts - current_hart_id()`: treat hart 0 as always online and each secondary as online only after `Acquire` observes `HART_ONLINE[i]`.
4. After PTE writes, issue a compiler-visible barrier plus `fence rw,rw`, local `SFENCE.VMA`, then synchronous remote RFENCE before returning success.
5. Batch contiguous W^X VAs into page-aligned ranges; use a bounded per-page list for gaps and full-address-space RFENCE only as a documented fallback.
6. Apply the same completion boundary to cell-segment teardown: serialize PTE clears against remap, invalidate locally/remotely, then and only then deallocate frames or release the PIE VA slot.
7. If an RFENCE fails after SMP is active or after PTE mutation, fail-stop/reboot before releasing the page-table lock, frames, or VA; a normal spawn error is not sufficient because reuse would expose stale translations.
8. Keep existing `ViError` discriminants and public ABI unchanged. Do not add SSIP payload queues or a runtime bypass.

## Success Criteria

- [x] RV64 W^X lowering cannot return success on SMP unless remote RFENCE succeeded or there are no online remote harts.
- [x] Teardown cannot free a cell frame or VA slot until a post-unmap all-hart invalidation completes; runtime RFENCE failure is fail-stop.
- [x] A hart-1 caller targets hart 0, while a hart-0 caller targets every online secondary.
- [x] Single-hart RV64 behavior remains compatible with existing `wx-text-write` baseline.
- [x] AArch64 path remains the existing TLBI broadcast path.
- [x] x86_64 path remains local-only and explicitly evidence-gated, with no fake closure.
- [x] Exact target checks pass: `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`; `cargo check -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc`; `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc`.

## Security Considerations

Fail-open is the main hazard. Probe failure keeps the system single-hart; any RFENCE error after RV64 SMP activation or PTE mutation is fail-stop before execution or reuse.

## Risk Notes

- Risk: wrong online mask skips hart 0 when hart 1 is the caller. Mitigation: model hart 0 as always online, then subtract the actual current hart.
- Risk: `remote_sfence_vma` range semantics are mis-sized. Mitigation: use page-aligned start and page-size multiples; fall back to full address-space remote fence if uncertain.
- Risk: lock-order inversion during teardown. Mitigation: complete page-table mutation/shootdown before taking `FRAME_ALLOCATOR`; remote firmware must not enter the S-mode scheduler path.
- Rollback: revert the RFENCE/reuse-barrier changes together and restore the explicit local-only limitation; never revert only teardown ordering while keeping the closure claim.
- Irreversible: writes committed through a skipped stale translation cannot be undone; this is why RFENCE failure after mutation is fail-stop.

## Deviation Log

- 2026-08-08 — Implemented page-granular RFENCE rather than range batching. This is a reversible performance deviation: the current W^X caller already lowers one page at a time, while the completion and reuse invariants stay intact.
- 2026-08-08 — QEMU `-smp 2` evidence is deferred to Phase 3: bundled OpenSBI exposed no HSM device and selected boot hart 1, so `sbi_hart_start(1)` returned `SBI_ERR_INVALID_PARAM` and Cellos stayed single-hart.
