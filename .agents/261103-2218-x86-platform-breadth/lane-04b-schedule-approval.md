# Phase 04b — Intel WIP lane scheduling

Recorded 2026-11-05. The owner (the user) chose sequencing option **(a)** of [s2-handoff-request.md](s2-handoff-request.md) §5: **04b first, then S2**. This answers H-S2-1 only.

**Effect:**
- Phase 04b ([phase file](phase-04b-irq-driven-pcie-drivers.md)) holds the Intel WIP lane from 2026-11-05.
- The [option B CP1](cp1-04b-waitcompletion-irq.md) (approved 2026-11-04) and its §6 handoff are now **effective**, for exactly the §6 file set. That includes the kernel-repair Phase 07 overlap on `kernel/src/task/scheduler.rs` lines 2223-2226 and the WaitCompletion/WaitIrq sweep arms (2303-2318) only.
- The Law-1 delta is CP1 §4 (`completion::IRQ = 1 << 3` and the MSI wake hint). CP2 is required after implementation.
- The integrating owner of the lane also adds the remaining H-1 launch rows (`/bin/x86-fpu-preserve`, `/bin/cpp-hosted-smoke`, `/bin/drm-lite-x86`) once those Cells exist, as the runtime plan's [handoff record](../261104-0802-x86-tier2-gpu-runtime/handoff-and-xsave-approval.md) requires.

**Not changed:**
- S2 (phases 06, 07a, 07b) stays a draft request. H-S2-2…H-S2-11 are unanswered, and S2 starts only after 04b closes and the owner answers them.
- The iGPU DMA-export and large-memory exceptions stay not yet effective (condition 3 also needs phase 12).
- All 38 lock relations keep their whole-plan scope.
