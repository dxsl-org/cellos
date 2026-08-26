## W^X currently stops at the local hart
**Verdict:** The current runtime guarantee is explicitly local-only; a second hart can retain stale writable translations after `wx::enforce`.
- `spawn_from_mem` relocates first, then calls `wx::enforce` before the task exists because another hart can steal and start it otherwise.
- `wx.rs` states the kernel has no way to shoot down writable PTEs cached on another hart.
- `protect_page` guarantees the lowered permissions only on the calling hart and requires an IPI shootdown for system-wide safety.
- The shipped docs and D7 ruling already record “no cross-hart TLB shootdown” as a live limit of the guarantee.
**Source:** [kernel/src/task.rs](/home/dmin/cellos/kernel/src/task.rs:939), [kernel/src/loader/wx.rs](/home/dmin/cellos/kernel/src/loader/wx.rs:21), [kernel/src/memory/page_protect.rs](/home/dmin/cellos/kernel/src/memory/page_protect.rs:7), [docs/specs/02-memory.md](/home/dmin/cellos/docs/specs/02-memory.md:65), [.agents/reports/decision-docket-260730.md](/home/dmin/cellos/.agents/reports/decision-docket-260730.md:64)

## RV64 already has the only outbound IPI path
**Verdict:** RV64 is the only arch in this checkout with both a sender and a receiver for cross-hart preemption, so any narrow P0 shootdown should start there.
- `Scheduler::pend_preempt_if_needed` sends an SBI IPI when a higher-priority task must preempt on another hart.
- SBI `send_ipi` is implemented and documented to raise SSIP on target harts.
- RV64 trap handling consumes SSIP (`scause=1`), clears `sip.SSIP`, and reuses `vi_timer_tick()` to enter the scheduler.
- SMP boot is RV64-only: `start_secondaries()` is real on RV64 and a no-op elsewhere.
**Source:** [kernel/src/task/scheduler.rs](/home/dmin/cellos/kernel/src/task/scheduler.rs:159), [hal/arch/riscv/src/common/sbi.rs](/home/dmin/cellos/hal/arch/riscv/src/common/sbi.rs:139), [hal/arch/riscv/src/rv64/trap.rs](/home/dmin/cellos/hal/arch/riscv/src/rv64/trap.rs:69), [kernel/src/task/smp.rs](/home/dmin/cellos/kernel/src/task/smp.rs:32)

## The scheduler is already multi-hart only on RV64
**Verdict:** The current work-stealing scheduler architecture fits an RV64-only shootdown helper cleanly; it does not have equivalent execution lanes on x86_64 or AArch64.
- Hart-local ready queues, current-task tracking, and work stealing are built around `MAX_HARTS=2`, `HART_RT=1`, and `steal_from_busiest()`.
- `yield_cpu()` dispatches by `current_hart_id()` and `pick_next(hart_id)`, so a per-hart deferred shootdown queue has a natural home in existing hart-local state.
- On non-RISC-V, `current_hart_id()` hard-returns `0`, so the scheduler is effectively single-hart in current builds.
- Hart 0 owns the global sweep while other harts only do local pick/steal, which limits where a shootdown acknowledgment path can run without broad scheduler churn.
**Source:** [kernel/src/task/hart_local.rs](/home/dmin/cellos/kernel/src/task/hart_local.rs:121), [kernel/src/task/hart_local/ready.rs](/home/dmin/cellos/kernel/src/task/hart_local/ready.rs:1), [kernel/src/task/smp.rs](/home/dmin/cellos/kernel/src/task/smp.rs:12), [kernel/src/task.rs](/home/dmin/cellos/kernel/src/task.rs:610), [kernel/src/task/scheduler.rs](/home/dmin/cellos/kernel/src/task/scheduler.rs:685)

## AArch64 and x86_64 only have local flushes today
**Verdict:** Both paged non-RV64 arches can invalidate a local TLB entry, but neither arch exposes an outbound kernel IPI/send path in this tree.
- AArch64 `flush_tlb_page` broadcasts TLBI across the inner-shareable domain for the current PE/regime, but the AArch64 GIC code only initializes distributor/CPU interface plus claim/complete; no SGI send helper exists in the checked-in GIC path.
- AArch64 IRQ handling is timer/GPIO/VirtIO dispatch only; there is no scheduler-owned SGI receive lane analogous to RV64 SSIP.
- x86_64 `flush_tlb_page` is an `invlpg` wrapper and explicitly says other cores keep stale entries until an IPI reaches them.
- x86 APIC code covers LAPIC timer, EOI, and IOAPIC redirection only; grep over `hal/arch/x86/src` found no ICR/x2APIC send helper or scheduler IPI consumer.
**Source:** [hal/arch/arm/src/aarch64/paging.rs](/home/dmin/cellos/hal/arch/arm/src/aarch64/paging.rs:37), [hal/arch/arm/src/aarch64/gic.rs](/home/dmin/cellos/hal/arch/arm/src/aarch64/gic.rs:33), [hal/arch/arm/src/aarch64/trap.rs](/home/dmin/cellos/hal/arch/arm/src/aarch64/trap.rs:181), [hal/arch/x86/src/x86_64/paging.rs](/home/dmin/cellos/hal/arch/x86/src/x86_64/paging.rs:160), [hal/arch/x86/src/x86_64/apic.rs](/home/dmin/cellos/hal/arch/x86/src/x86_64/apic.rs:90)

## The W^X/page-protect API surface is intentionally tiny
**Verdict:** The permission-lowering surface is narrow enough for a small internal exception: `wx::enforce` is the only current caller of `protect_page`, and `protect_range` is unused.
- `wx::enforce` walks the page list and calls `memory::paging::protect_page` per page.
- Repo-wide grep found no second production caller of `protect_page`; `protect_range` exists but is not currently invoked.
- `tlb_flush_all()` exists, but nothing in the W^X path calls it, so there is no current fallback to “flush everything after lowering.”
- This keeps blast radius low: a shootdown helper can be added under `wx::enforce` or `protect_page` without ABI changes or unrelated syscall churn.
**Source:** [kernel/src/loader/wx.rs](/home/dmin/cellos/kernel/src/loader/wx.rs:119), [kernel/src/memory/page_protect.rs](/home/dmin/cellos/kernel/src/memory/page_protect.rs:135), [kernel/src/memory/paging.rs](/home/dmin/cellos/kernel/src/memory/paging.rs:46), `git grep -n -E "protect_page\\(|protect_range\\(" -- kernel/src`

## History already narrows the exception to D7 fallout
**Verdict:** This is not a new architecture direction; it is a documented post-W^X residual hole with direct implementation and runtime evidence.
- `d078c1a0` introduced post-relocation WRITE revocation; `8f9e3a16` hardened it into W^X plus signed admission.
- `phase-10-wx-implementor-260730.md` recorded the feature as code-complete but runtime-unverified and explicitly carried “no SMP shootdown” as an open limit.
- `a4-runtime-gates-260731.md` later recorded the RV64 `wx-text-write` gate as 2/2 PASS and boot 54/54 PASS.
- `e15af924` and `2d7d40fc` added RV64 HSM/IPI/work-stealing groundwork that a shootdown path can reuse without widening the ABI.
**Source:** `git log --oneline --all --grep="W^X\\|wx\\|IPI\\|SMP\\|preempt" -i`, [.agents/reports/phase-10-wx-implementor-260730.md](/home/dmin/cellos/.agents/reports/phase-10-wx-implementor-260730.md:42), [.agents/reports/a4-runtime-gates-260731.md](/home/dmin/cellos/.agents/reports/a4-runtime-gates-260731.md:64)

## Ranked recommendation
**Verdict:** Rank 1 is an RV64-only kernel-internal shootdown for W^X/page-permission lowering; rank 2 is a stop-the-world full-flush variant only if the per-VA path proves too invasive; rank 3 is leaving the gap documented but open.
- **1. Preferred:** batch the VAs inside `wx::enforce`, send SBI IPIs to online secondary harts, and have the SSIP/scheduler path drain a kernel-owned “flush these pages” queue before returning to user mode. Best architectural fit because RV64 already has sender, receiver, hart IDs, and SMP execution. Adoption risk is medium: requires careful acknowledgment/waiting but no ABI expansion and no non-RV64 work.
- **2. Fallback:** on RV64, trigger a coarser all-hart flush (`sfence.vma` local + remote rendezvous) after the lowering batch. Simpler proof story, higher latency, and worse future fit if the kernel later wants frequent permission changes outside spawn.
- **3. Reject:** x86_64/AArch64 parity in the same P0. Current tree lacks outbound IPI machinery there, so forcing symmetry would turn a narrow exception into a cross-arch interrupt-controller project.
- Scope guard: keep it page-permission invalidation only, kernel-internal only, and ordered after HANDOFF §8 work as a separate plan. Do not couple it to Tier 2, ASIDs, domain page tables, or generic scheduler redesign.
**Source:** [kernel/src/task/scheduler.rs](/home/dmin/cellos/kernel/src/task/scheduler.rs:159), [kernel/src/task/hart_local.rs](/home/dmin/cellos/kernel/src/task/hart_local.rs:19), [kernel/src/task.rs](/home/dmin/cellos/kernel/src/task.rs:939), [.agents/reports/HANDOFF-260731.md](/home/dmin/cellos/.agents/reports/HANDOFF-260731.md:64)

## Limitations
**Verdict:** This report proves current mechanisms and fit, not final implementation correctness.
- I did not build or boot new artifacts in this task; runtime claims are taken only from already-recorded reports.
- Absence claims on x86_64/AArch64 rely on current-tree grep plus the closest likely files (`apic.rs`, `gic.rs`, trap code); they are strong but still negative evidence.
- I did not trace hypervisor stage-2/EPT invalidation paths beyond confirming they are separate codepaths from Tier-1 W^X.
- I did not draft the phase plan or ack protocol; this is the evidence pack for that plan.
**Source:** [.agents/reports/phase-10-wx-implementor-260730.md](/home/dmin/cellos/.agents/reports/phase-10-wx-implementor-260730.md:6), [.agents/reports/a4-runtime-gates-260731.md](/home/dmin/cellos/.agents/reports/a4-runtime-gates-260731.md:6)
