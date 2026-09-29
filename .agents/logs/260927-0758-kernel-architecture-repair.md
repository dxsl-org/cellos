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

- A second defect fell out of the same reading: the AArch64 catch-all trap arm inferred a trap's origin
  from `spsr_el1 & 0xF == 0`, which cannot work on QEMU virt (the host runs at EL2 with TGE=1, so that
  field holds the host's own `SPSR_EL2` and reads zero exactly when the host was about to `eret` to EL0).
  One run in thirty killed an innocent cell for a host exception inside `__trap_exit`. The arm now
  requires the lower-EL vector marker. Open: why a host exception with EC 0 happens there at all — the
  next one is reported as a kernel trap with the marker instead of as a cell fault.
- Two of forty runs also failed on a lane bug, not a kernel one: counting every `[fault] Cell` line made
  a deliberate fault's termination record and its "already retired; deferred fault dropped" note read as
  two faults. Terminations only now: 40 of 40.

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

## 2026-09-29 — phase 08 residual closed: an IPI is not a timer tick

## What happened
Chased the AArch64 two-hart lane's ~5-in-10 flakiness to a single cause and fixed it, then verified
every runnable lane again. Also started the second AArch64 hart properly enough that the lanes assert
it (PSCI over HVC) and corrected the plan bullets that still described the pre-work tree.

## Decisions
- An SGI must not enter `vi_timer_tick()`. It carried an invalidation or a retirement request, and the
  tick path adds a scheduler decision the requester never asked for. On a hart parked in `wfi` — the
  whole life of `smp_aarch64_secondary_main` — that loses the hart (measured: 33 acknowledgements, last
  at `epoch=35`, then deaf while the pair asked for 36-38); on a hart running a task it re-enters the
  scheduler between a reader's load and its use. `vi_ipi_service` now does only the IPI's duties:
  flush the local TLB and publish the epoch, and answer a retirement request when the hart holds no task
  ("no task" is the proof a switch gives, and the only one a parked hart can offer).
- A root retirement off RV64 must publish from the switch path: `complete_incoming_switch` never called
  `complete_retirement_switch`, so a busy hart only answered when it happened to go idle. RV64 has
  published this from its assembly boundary all along.
- A deferred release is decided by its own tag's acknowledgement, not by "does any hart owe any
  invalidation" — the global question kept an already-acknowledged tag waiting behind unrelated
  teardowns (measured 235 attempts ≈ 2.3 s; now `attempts=1`).
- Fixture-side, an asynchronous revocation may not be asserted synchronously: both grant-pair fixtures
  retry their store until the revocation traps, bounded.

## Lessons
- "The hart answered 33 IPIs and then stopped" is a *state* bug, not a timing bug: look at what the
  handler does to the hart, not at how long the requester waits.
- A reader that sees three different values of one slot inside one path is telling you a writer ran
  between two of its loads. The writer was a nested `yield_cpu`.
- Debug output that changes the pass rate (here 12 of 12 with a probe, about half without) is a signal
  that the bug is a race with a window, not that the probe "fixed" anything.

## Next steps
- Done in this session: the bootstrap `disk_v3.img` is regenerated with `scripts/gen-disk-ci.sh`
  (not built by hand — that was the whole problem: `launch-profile` saw `snapshot: supervisor
  unavailable` and `tier2-fault-isolation` hit loader cap refusals). Both lanes are green on the
  fresh disk: 1/1 and 5/5.
- Phase 02's remaining items are hardware-gated and named: ASID-faithful invalidation witness (QEMU
  8.2.2 retires unrelated ASIDs), the x86 `INVPCID` instruction path (KVM only), and x86_64 second-hart
  bring-up.
- Phase 07's board witness and Phase 08's MMC/remote-TLB/latency rows stay hardware-gated.

## Environment notes for whoever runs the lanes next
- Another session had `hal/arch/arm` mid-edit (uncommitted; a stray `\` in an asm block breaks the
  AArch64 image). Verification was run from a clean worktree of these commits instead.
- `.cargo/config.toml` is untracked: a fresh worktree needs it copied, or cell linking fails with
  `R_AARCH64_ABS64 cannot be used against local symbol`.
