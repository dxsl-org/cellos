# 2026-09-27 — Kernel architecture repair, phases 04–06 + AArch64 lane

## What happened
Second stretch of the kernel repair session, after phase 01 and phase-02 slices 1–2:

- **Phase 06** (`89188d72c`): `wake_sender_token` discarded `push_ready`'s priority, so a
  consumed message never requested preemption for the sender; `pend_preempt_if_needed` also
  decided from the calling hart while targeting `HART_RT`. New fixture `S22-RV64-RT-WAKE`
  (case `rt-wake`, `--harts 2`) observes the decision through a test-hooks per-hart counter.
- **Phase 04** (`c787028d0`): the 4 MiB boot heap was reserved as 1,024 single frames and then
  initialized as if adjacent; `reserve_contiguous_run` is one `allocate_contiguous` transaction
  and boot refuses a fragmented map before `init_heap`.
- **Phase 05** (`8a6d45e8c`): the allocator kept only the largest `Usable` map entry. Every
  usable range is now managed (index space = concatenation of ranges), malformed maps halt,
  `allocate_contiguous` is confined to one range, `manages()` backs the framebuffer guard, and
  the boot log names each range.
- **AArch64 test-hooks lane** (`10dcc83aa`): repaired (RV64-only scaffolding lacked its cfg) —
  it builds and boots to the vfs-test suite; its `admission-core` marker stays red for a
  pre-existing quota-table interaction, proved not caused by these changes.

## Decisions
- Phase 02 slice 3 (non-RV64 ordered switch) is **not** half-built: `Context::switch` is a
  single assembly routine per architecture, the plan requires a domain-level witness before
  such a change lands, and no AArch64 domain fixture exists yet. The prerequisites are written
  into `plan.md` § Remaining work instead of guessed at.
- The remaining todos are blocked with reasons (missing x86 lane, missing board, dependency
  ordering) rather than left as pending work that looks actionable.

## Lessons
- Anything on the allocator's init path runs **before the heap exists**: a `Vec` in
  `plan_managed_ranges` took the RV64 boot down inside `new_from_map`, and the same mistake in
  `allocate_contiguous` was waiting behind it. The boot smoke caught both.
- A `git stash` of files that are already committed reverts nothing — one "exoneration"
  experiment was void until it was redone by editing the hunk directly. Verify the experiment
  changed what you think it changed.
- Fixture arithmetic deserves the same care as the code: an 8-frame range is `0x8000` bytes, so
  a "hole" fixture with ranges 0x1000 and 0x9000 had no hole at all.

## Next steps
Per `plan.md` § Remaining work: build the AArch64 domain fixture and the per-hart invalidation
ack, then the non-RV64 switch ordering; x86 PCID needs the x86 lane; phase 03 follows the ack;
phase 07 needs a block-capable board; phase 08 aggregates 02–07.
