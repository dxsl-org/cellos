# 2026-09-27 — Kernel architecture repair plan

## What happened
Audited prior kernel findings against live code and wrote the eight-phase repair plan at `.agents/260927-0739-kernel-architecture-repair/`. Four adversarial reviews tightened grant ABI denial, switch ordering, ASID reuse, snapshot state closure and memory-map fallback.

## Decisions
- Keep domain grants and snapshot unavailable until real end-to-end witnesses exist; deny AArch64/x86 Tier-2 admission until ordered single-CPU context switching is proven. Do not silently downgrade to SAS.
- Preserve public ABI. `GrantAlloc`/`GrantRegister` denial returns zero, not the generic error sentinel, which the wrapper treats as a pointer.
- Snapshot requires coherent capture, mutable kernel roots, crash-safe consume state, and external freshness if storage is adversarial; no QEMU-only warm-boot qualification.
- Multi-region allocation follows the contiguous heap fix; malformed authoritative maps halt, not fallback to unsafe x86 guessed RAM.

## Lessons
- Private one-page `DomainGrant` selftest cannot qualify public multi-page, SAS/domain, VFS/DMA grant flows.
- The same ASID wrap fault affects all three backends, not just x86 PCID; tests must force live-tag rollover.

## Next steps
Implement phase 01 fail-closed gates, then phase 02 targeted TLB/switch correctness. Keep exact-board and SMP qualification gates closed until proven; continue phases in `plan.md` dependency order.

## 2026-09-28 — phases 02/03/07 advanced; docs reconciled

## What happened
Closed the phase-02 slice-3 items that were still open (non-RV64 incoming completion hook,
tag-targeted acknowledged frame release, x86 PCID/INVPCID runtime gate), implemented the RV64
domain grant lifecycle behind a capability-shaped gate, rewrote the snapshot format as a
crash-safe v2 with an explicit address inventory, built a real two-cell public-syscall grant
lane, and reconciled the docs with the evidence (including withdrawing the unwitnessed
Phase-29 "COMPLETE" snapshot status and its sub-100 ms figure).

## Decisions
- Keep AArch64/x86_64 Tier-2 admission refused. Two named blockers: no non-RV64 image has
  entered a real domain task, and AArch64 private-root page-table leaves are global (no
  `PTE_nG`), so ASID-targeted invalidation cannot reach them and a stale entry of one private
  root stays usable under another ASID. Recorded as a Finding with the exact encoder line.
- The x86 PCID gate now requires a correct INVPCID probe (`CPUID.07H:EBX[10]`, not
  `01H:ECX[12]` = FMA) and refuses a nonzero PCID without INVPCID, because a CR3 reload only
  invalidates the current tag.
- The awaited flush is fail-closed but unsound as a *synchronous* design: a remote hart can stop
  acknowledging (272 acks then 267 timeouts in one 2-hart boot), so phase 03 must defer release
  to a reaper instead of widening the 25 × 200 ms budget.
- Domain grants are RV64-only: the phase-01 sentinels stay byte-for-byte for every non-capable
  shape, and the RV64 path is proven by a real owner/receiver Tier-2 pair rather than the old
  private one-page fixture.
- Snapshot stays disabled (`snapshot-qualified` untouched): the format and the corruption/reset
  matrices are done, but quiescence, closure, coherent staging, freshness and the board witness
  are not, so no warm-boot claim is made.

## Lessons
- A pre-fix writer/reader pair can only be shown red by transcribing it into a throwaway harness
  when the feature is gated off and the kernel block device is `NullBlock` — say so explicitly
  instead of implying an in-tree red run.
- Test-hooks lanes embed architecture-specific artifacts (`kernel/src/embedded-test-hooks/init`):
  the last lane built wins, so the RV64 image must be rebuilt before committing or the RV64 lanes
  inherit an AArch64 init.
- Cross-target `-D warnings` must be checked per feature tuple: an import gated for
  `native-domains + riscv64` was unused on AArch64 with the same features.

## Next steps
- Phase 03 gaps: 2-hart pair shootdown, deferred-ack tolerance in the boot fixture,
  same-recipient RW→RO assertion, DMA/VFS interaction.
- Phase 02: implement non-global private leaves on AArch64, then prove a real domain entry on
  AArch64/x86_64 one CPU before reopening admission.
- Phase 07: all-hart quiescence and closure, then the two-boot witness on a block-capable board.
- `docs/system-architecture.md` corrections remain uncommitted (the file also carries unrelated
  in-flight edits); commit them with that work.
