## Portable acknowledged shootdown is the only P0 fit
**Verdict:** Rank 1 is a synchronous, per-page permission-lowering shootdown on the harts/cores that can use the address space; rank 2 is the same protocol with a coarser full-ASID/full-TLB invalidate; rank 3 is to avoid Tier-2/domain-table redesign in this plan.
- This fits the current `protect_page` / `wx::enforce` surface and keeps HANDOFF §8 as a separate narrow plan, with no ABI expansion and no unrelated Midori work.
- Current Cellos backends are split: RV64 and x86_64 are explicitly local-only today, while AArch64 already has a broadcast `TLBI ...IS` path in the HAL.
- Trade-off: rank 1 minimizes blast radius and preserves W^X scope; rank 2 is simpler but higher latency; rank 3 violates YAGNI for this exception.
- Adoption risk is rendezvous correctness, not PTE semantics: the hard part is making every relevant CPU participate and ack before reuse/trust resumes.
**Source:** `.agents/reports/HANDOFF-260731.md:160` · [kernel/src/memory/page_protect.rs](/home/dmin/cellos/kernel/src/memory/page_protect.rs:11) · [hal/arch/riscv/src/rv64/paging.rs](/home/dmin/cellos/hal/arch/riscv/src/rv64/paging.rs:7) · [hal/arch/x86/src/x86_64/paging.rs](/home/dmin/cellos/hal/arch/x86/src/x86_64/paging.rs:166) · [hal/arch/arm/src/aarch64/paging.rs](/home/dmin/cellos/hal/arch/arm/src/aarch64/paging.rs:63)

## RV64 requires explicit remote-hart participation
**Verdict:** `SFENCE.VMA` alone does not close the SMP window; the architecture-correct sequence is PTE store -> make that store globally visible -> notify remote hart(s) -> remote `SFENCE.VMA` -> ack completion.
- The RISC-V privileged spec states that `SFENCE.VMA` orders only the local hart’s implicit page-table references.
- The same spec gives the shootdown pattern explicitly: local data fence, interprocessor interrupt, remote `SFENCE.VMA`, then signal completion back.
- The spec also allows a hart to keep using any translation valid since the last subsuming `SFENCE.VMA`, so stale permissive entries are architecturally allowed until the remote hart fences.
- Current Cellos RV64 HAL still documents `flush_tlb_page` as single-hart only.
**Source:** [RISC-V Privileged ISA v1.13, Supervisor-Level ISA `SFENCE.VMA`](https://docs.riscv.org/reference/isa/v20260120/priv/supervisor.html) · [hal/arch/riscv/src/rv64/paging.rs](/home/dmin/cellos/hal/arch/riscv/src/rv64/paging.rs:7)

## x86_64 needs all sharers to invalidate after the PTE change
**Verdict:** `INVLPG` or `MOV CR3` on one CPU is insufficient; every logical processor using the paging structures must invalidate after the modification, usually via IPI, before the page is trusted as non-writable or reallocated.
- Intel SDM §4.10.5 defines propagation to multiple processors as TLB shootdown and requires that all logical processors using the paging structures participate and perform appropriate invalidations after the modifications are made.
- Intel SDM §4.10.4.1 and §4.10.4.2 define `INVLPG` / `MOV CR3` as the recommended local invalidation mechanisms and note those invalidating instructions are serializing.
- Intel also warns that delayed invalidation can leave writes, reads, and instruction fetches observing either the old or new translation before shootdown completes.
- Current Cellos x86_64 HAL still documents `flush_tlb_page` as non-shootdown local invalidation only.
**Source:** [Intel SDM Vol. 3A §4.10.4.1-§4.10.5](https://cdrdv2-public.intel.com/819717/325384-sdm-vol-3abcd.pdf) · [hal/arch/x86/src/x86_64/paging.rs](/home/dmin/cellos/hal/arch/x86/src/x86_64/paging.rs:166)

## AMD-specific broadcast assists exist, but they are the wrong P0 dependency
**Verdict:** Do not make `INVLPGB`/`TLBSYNC` the plan’s dependency; keep the baseline algorithm generic and treat AMD broadcast invalidation as an optional future optimization.
- AMD’s EPYC 7003 microarchitecture overview advertises broadcast TLB invalidation via `INVLPGB` and `TLBSYNC`, which proves vendor-specific acceleration exists.
- AMD’s Family 19h revision guide records errata around `INVLPGB` behavior, including failures to flush some global translations and a monitor-state interaction after `TLBSYNC`.
- That risk profile is fine for later optimization work, not for a P0 closure whose goal is portable correctness across x86_64 CPUs Cellos may boot on.
- Architectural fit: generic IPI-based shootdown works on Intel and AMD; vendor-specific fast paths can be layered later without changing the W^X contract.
**Source:** [AMD EPYC 7003 Series Microarchitecture Overview](https://docs.amd.com/api/khub/documents/cdbcpYJAub6P1i3lB2DRJg/content) · [AMD Family 19h Models 00h-0Fh Revision Guide](https://www.amd.com/content/dam/amd/en/documents/processor-tech-docs/revision-guides/56683.pdf)

## AArch64 `TLBI ...IS` is already cross-PE within the shareability domain
**Verdict:** Yes: `TLBI VAAE1IS`/`VAE1IS` plus `DSB ISHST` before, `DSB ISH` after, and `ISB` is architecture-sufficient cross-PE invalidation inside the Inner Shareable domain; no SGI/IPI is required for correctness there.
- Arm’s TLBI instruction pages state that `VAAE1IS` / `VAE1IS` invalidation applies to all PEs in the same Inner Shareable domain as the executing PE.
- Arm’s memory-management guide says `TLBI ...IS` is broadcast to other cores in the Inner Shareable domain and shows the canonical sequence `STR` PTE -> `DSB ISH` -> `TLBI ...IS` -> `DSB ISH` -> `ISB`; `DSB ISHST` is the narrower pre-barrier when only prior writes need to complete.
- Arm’s `DSB` and `ISB` docs define the two ordering roles needed here: complete prior page-table stores before TLBI, then ensure TLBI completion and context synchronization before later execution.
- Scope caveat: this is stage-1 EL1/EL0 correctness inside the shareability domain; if Cellos ever relies on EL2 or stage-2 translations for the same mapping, the matching EL2 / S1+S2 TLBI regime must be targeted too.
**Source:** [Arm TLBI VAAE1IS](https://developer.arm.com/documentation/111107/2026-03/AArch64-Instructions/TLBI-VAAE1IS--TLBI-VAAE1ISNXS--TLB-Invalidate-by-VA--All-ASID--EL1--Inner-Shareable) · [Arm TLBI VAE1IS](https://developer.arm.com/documentation/ddi0487/maa/-Part-C-The-AArch64-Instruction-Set/-Chapter-C5-The-A64-System-Instruction-Class/-C5-5-A64-System-instructions-for-TLB-maintenance/-C5-5-60-TLBI-VAE1IS--TLBI-VAE1ISNXS--TLB-Invalidate-by-VA--EL1--Inner-Shareable) · [Arm Memory Management 101, §8 TLB maintenance](https://developer.arm.com/-/media/Arm%20Developer%20Community/PDF/Learn%20the%20Architecture/LearnTheArchitecture-MemoryManagement-101811_0100_00_en.pdf) · [Arm `DSB (A64)`](https://developer.arm.com/documentation/dui0801/latest/A64-General-Instructions/DSB--A64-) · [Arm `ISB`](https://developer.arm.com/documentation/100069/0610/A64-General-Instructions/ISB?lang=en) · [Arm TLBI VMALLS12E1IS](https://developer.arm.com/documentation/ddi0487/mb/-Part-C-The-AArch64-Instruction-Set/-Chapter-C5-The-A64-System-Instruction-Class/-C5-5-A64-System-instructions-for-TLB-maintenance/-C5-5-81-TLBI-VMALLS12E1IS--TLBI-VMALLS12E1ISNXS--TLB-Invalidate-by-VMID--All-at-Stage-1-and-2--EL1--Inner-Shareable)

## Current Cellos evidence narrows the real gap by architecture
**Verdict:** The live repo does not support a blanket “all arches are local-only” claim anymore: RV64 and x86_64 still are, but AArch64 already implements the textbook broadcast sequence in the HAL.
- `page_protect.rs` and the D7 docket entry still describe the generic limitation as “no cross-hart shootdown,” which remains true for RV64/x86_64 but is now too broad for current AArch64 code.
- The AArch64 HAL issues `dsb ishst; tlbi vaae1is; [if EL2 active] tlbi vae2is; dsb ish; isb`, exactly the architecture sequence needed for stage-1 EL1/EL0 invalidation inside the Inner Shareable domain.
- `wx.rs` still correctly calls out the real unresolved risk class: another hart caching a writable translation before or across permission lowering if the arch backend does not propagate the invalidate.
- Planning consequence: the P0 exception should be arch-scoped, not a repo-wide rewrite. RV64/x86_64 need mechanism work; AArch64 mainly needs proof and wording cleanup.
**Source:** [hal/arch/arm/src/aarch64/paging.rs](/home/dmin/cellos/hal/arch/arm/src/aarch64/paging.rs:63) · [kernel/src/memory/page_protect.rs](/home/dmin/cellos/kernel/src/memory/page_protect.rs:11) · [kernel/src/loader/wx.rs](/home/dmin/cellos/kernel/src/loader/wx.rs:21) · [.agents/reports/decision-docket-260730.md](/home/dmin/cellos/.agents/reports/decision-docket-260730.md:296)

## QEMU closure needs an SMP stale-write proof, not another single-hart W^X pass
**Verdict:** QEMU evidence should prove stale writable translation invalidation on at least two virtual CPUs; the existing `wx-text-write` single-hart pass is necessary regression coverage but not sufficient closure.
- Required test shape: one CPU caches a writable translation for a mapped page, another CPU lowers the same PTE to RO/RX, then the first CPU retries the write after the shootdown path; pass only if the retry faults before any write commits.
- Required controls: CPU affinity/pinning, explicit phase handshakes, a negative lane that bypasses remote participation on RV64/x86_64, and enough iteration to expose races rather than one clean run.
- Existing Cellos evidence is still single-hart functional W^X (`wx-text-write` 2/2 PASS) plus documentation that `protect_page` was the hole on SMP.
- This QEMU gate is architecture-correctness plumbing proof; it is not yet hardware closure.
**Source:** [.agents/reports/decision-docket-260730.md](/home/dmin/cellos/.agents/reports/decision-docket-260730.md:296) · [kernel/src/memory/page_protect.rs](/home/dmin/cellos/kernel/src/memory/page_protect.rs:11) · [kernel/src/loader/wx.rs](/home/dmin/cellos/kernel/src/loader/wx.rs:21)

## Hardware closure must be separate from QEMU closure
**Verdict:** Real security closure needs one real SMP machine per supported paged architecture class, because emulator success proves protocol wiring, not that real cores stop using stale writable translations under contention.
- Required hardware lane: one real RV64 SMP platform with the actual S-mode/firmware path Cellos uses, one real x86_64 SMP machine, and one real AArch64 SMP board if AArch64 remains a supported Tier-1/2 target.
- Required proof: the same stale-write test as QEMU, run under affinity and preemption stress, with no writable commit after the lowering point and with the page/frame withheld from reuse until every participant acks.
- Inference, not vendor-manual text: QEMU can serialize or simplify TLB/cache behavior, so it is valuable for regression and protocol bring-up but weak as the final security evidence for stale-translation races.
- Limitation of this research: I did not run the lane or measure Cellos’ current interrupt/firmware rendezvous cost; the recommendation is about correctness and scope, not timing/WCET.
**Source:** [RISC-V Privileged ISA v1.13, Supervisor-Level ISA `SFENCE.VMA`](https://docs.riscv.org/reference/isa/v20260120/priv/supervisor.html) · [Intel SDM Vol. 3A §4.10.5](https://cdrdv2-public.intel.com/819717/325384-sdm-vol-3abcd.pdf) · [Arm Memory Management 101, §8 TLB maintenance](https://developer.arm.com/-/media/Arm%20Developer%20Community/PDF/Learn%20the%20Architecture/LearnTheArchitecture-MemoryManagement-101811_0100_00_en.pdf)
