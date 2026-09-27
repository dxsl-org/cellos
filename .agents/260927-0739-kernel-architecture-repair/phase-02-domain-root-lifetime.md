---
phase: 2
title: "Repair PTE lifetime and private-root switch completion"
status: pending
priority: P1
effort: "split by architecture; 2–3 reviewable changes"
dependencies: [1]
tier: thinking
---

# Phase 02: Domain root and TLB correctness

## Requirements / architecture
- `AddressSpace::unmap_grant_page` drops pruned `OwnedFrame`s before flush; `unmap_private_page` also frees the owned leaf with **no** remote flush (`kernel/src/memory/address_space.rs:630-665,743-773`). Use a private-root/ASID-targeted unmap transaction, not only `flush_page(vaddr)` on the currently active kernel root: remove ledger, drain readers, clear PTEs, retain detached table and leaf frames, order writes, await target-root local+remote invalidation, **then** release frames. On any failed ack retain/quarantine all affected frames. Do not wait for copy readers while holding a lock they require. Keep RV64 RFENCE fail-closed contract (`memory/tlb_shootdown.rs:65-90`); a local x86 `invlpg` is not remote PCID invalidation.
- AArch64/x86 switch currently activates TTBR0/CR3 **before** `Context::switch` saves outgoing registers (`task.rs:1356-1366`), then checks `current_domain()==0` before the method that clears it (`task/hart_local.rs:595-611`); RV64 incoming completion releases staged `Arc`/pin (`task.rs:910-924`). Inspect HAL assembly; change raw switch boundary to save outgoing context first, change root+stack under ordered transition, and call an incoming completion hook before re-enabling interrupts. Include first-entry and boot-context handoff, generation-tagged acknowledgement, pin release and user-copy guard reset. Audit trap/syscall root entry/return (x86 precedent `3376ef385`); never touch the outgoing stack after switching root.
- x86 backend mixes a nonzero software tag into CR3 (`hal/arch/x86/src/x86_64/domain.rs:22-53`, `kernel/src/memory/address_space.rs:837-855`). Detect CPUID PCID/INVPCID and CR4.PCIDE per CPU; enable only under valid kernel CR3 boot conditions, otherwise use PCID 0 and full-flush CR3 reload. Use x86 tag-width (12 bits), generation-aware reuse and actual target-context INVPCID when available; legacy fallback must flush all relevant entries before tag reuse. `current_hart()`/`current_hart_id()` on non-RV64 hard-code slot 0 (`task/hart_local.rs:293-356`): until multi-CPU hart identity, IPI and remote ack are implemented, keep non-RV64 Tier-2 admission **single-CPU only** rather than claiming SMP safe.
- The shared `AsidLease::acquire` masks to 16 bits and returns tag 1 at wrap, **then tag 1 again on the next acquisition** (`memory/address_space.rs:11,837-855`). RV64 SATP and AArch64 TTBR0 likewise use 16-bit masks; a live tag-1 root cannot share its tag with a new root. Make leases architecture-width-aware (x86 PCID 12, ARM supported ASID width, RV64 WARL probed width), track live owners/generations and reserve/release tags with completed local/remote invalidation **before** reuse. If no safe tag remains, refuse admission or use proven tag-0 full-flush mode, never wrap into a live tag.

## Related files
`kernel/src/memory/address_space.rs`, `kernel/src/memory/tlb_shootdown.rs`, `kernel/src/task.rs`, `kernel/src/task/domain_switch.rs`, `kernel/src/task/hart_local.rs`, `kernel/src/task/hart_local/ready.rs`, `hal/arch/x86/src/x86_64/domain.rs`, `hal/arch/x86/src/x86_64/{idt/entry.rs,syscall.rs}`, AArch64/RV64 domain backends and AArch64/x86 HAL context-switch assembly/trap backends, `kernel/src/task/domain_switch_tests.rs`, `kernel/src/memory/tlb_shootdown_selftest.rs`, `cells/tests/tier2-{smoke,exploit}` and target-specific image builders/runners.

## Implementation steps
1. Construct last-leaf grant and private-page unmap tests while a remote hart runs the root; check frame ownership across forced allocator churn. Collect detached `OwnedFrame`s; invalidate the **address space tag**, not just the kernel root; only then drop collection. Specify failure handling for `table.unmap`, `prune_empty` and partial ledger change.
2. Refactor AArch64/x86 raw switch ordering and incoming completion with exact `(id,generation,hart)` invariants; preserve RV64 assembly timing, test first entry, domain→SAS, domain→domain, domain→boot and back, exit/fault and same-domain return. Verify no root write for SAS→SAS, mandatory root restore when trap entry changed it. **Before reopening** non-RV64 Tier-2 admission, build/pack signed and unsigned Tier-2 smoke/fault Cells into fresh AArch64 one-PE and x86 one-CPU images; execute fail-hard integration tests that assert actual domain-admission/root-switch/fault markers, shell recovery and frame release. Prompt-only CI boot jobs are insufficient; add these phase-02 lanes, leaving phase 08 to rerun and aggregate.
3. Implement live-lease-aware per-architecture ASID/PCID allocator. Test forced rollover with a live tag-1 owner, exhaustion, tag recycle after full invalidation and targeted remote shootdown on RV64 2 harts; test AArch64/x86 single-CPU modes. Add x86 PCID-off/on CPU lanes, assert CR4.PCIDE and CR3 low bits; until `hart_local` and HAL have per-CPU remote ack, refuse non-RV64 multicore Tier-2 admission.
4. Re-run RV64 1/2-hart `switch,resume-root,sas-fastpath,migration,user-copy-race` fixtures and full target-specific trap/fault tests from fresh artifacts.

## Success criteria
- [ ] Reclaimed table/root frames cannot reallocate until all relevant harts acknowledge invalidation; no stale PTE walk under contention.
- [ ] After each non-RV64 safe-root transition current-domain ID is zero and displaced `current_harts` bit/Arc is released, with correct generation.
- [ ] PCID disabled/unsupported x86 never writes a nonzero low CR3 tag; tag 1 never duplicates a live owner on RV64/AArch64/x86, exhaustion fails closed, and PCID-enabled reuse has completed invalidate. Non-RV64 Tier-2 SMP stays disabled without per-CPU remote ack.
- [ ] Tier-1 SAS→SAS fast path unchanged and fault containment remains live on each architecture.

## Assumptions / risk / rollback
- [UNVERIFIED] Non-RV64 context-switch implementation supplies a point equivalent to RV64 incoming saved-context callback; inspect assembly and prove before selecting hook placement. Rollback: disable Tier-2 admission and cold reboot; leaving a stale TLB mapping or an already recycled frame cannot be reversed by reverting binaries. Stop deployment and retire any compromised dev workload. Preserve test evidence, no production qualification from QEMU alone.

## Deviation Log
None.
