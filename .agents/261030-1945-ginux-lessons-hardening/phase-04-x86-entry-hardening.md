---
phase: 4
title: "SAS-compatible x86 entry safety and measured protection options"
status: pending
priority: P1
dependencies: [1]
tier: thinking
---

# Phase 04: SAS-compatible x86 entry safety

## Overview

IST/GS entry correctness is necessary even for trusted SAS Cells: CPL and address-space isolation are separate dimensions. LBI does not make hardware exception delivery safe. Conversely, Ginux's mandatory copy-boundary model must not replace Cellos's admitted shared-memory ownership contracts.

## Requirements and architecture

- Preserve Tier 1 shared page tables and admitted Rust ownership paths. No automatic Tier 2 conversion, new CR3 switch, bounce buffer, generic per-IPC revalidation or mandatory copying introduced to enable SMAP.
- Extend architectural TSS and vector-specific IST for NMI/#DF/#MC, per supported CPU. Derive stack capacity from actual frame/handler nesting and measured high-water, not an unexplained fixed 16 KiB. Preallocate, charge, align and guard both ends before use; map supervisor-only NX in every CR3 that can receive entry, including a private-domain CR3 before software switches it.
- Decode saved old RSP/SS for same-CPL IST entry. Select GS/CR3 entry behavior safely during SWAPGS and address-space transition windows; do not trust saved CS alone. Per-CPU arrays must not alias during AP initialization or migration.
- Preserve the existing fatal NMI/#DF/#MC policy initially. Do not adopt log-and-return as a side effect of stack hardening. A future resumable NMI requires separate policy approval and nested-NMI/GS/CR3 proof.
- Fatal diagnostics use bounded preallocated state and an emergency lock-free/nonblocking output path, never normal logger, allocator, scheduler or locks that an interrupted CPU may hold. If output is unavailable, retain the record and halt; no false ordinary-Cell recovery for kernel corruption.
- Clear live AC on every kernel entry before normal kernel work, without destroying saved RFLAGS. On return, restore interrupted kernel AC only as part of an actually active authorized access scope; a Cell cannot use its saved AC to bypass SMAP on entry.
- SMEP/SMAP/UMIP are CPUID/profile-qualified defense in depth, not LBI substitutes. Evaluate independently. Inventory supervisor execution of shared U=1 code before SMEP and legitimate supervisor access to U=1 mappings before SMAP.
- For existing authorized SAS pointers/grants/rings/MMIO, retain source-owned lifetime/pin and bounded borrowed access. An audited access window may cover the operation without an intermediate copy. Keep `stac` scope non-preemptible or explicitly balanced across entry/return, error and fault fixup; no blanket AC window for all syscalls.
- User pointers from Tier 2 remain validated and fault-recoverable. Pointer presence or SAS locality is not resource authority.

## Related Files

HAL `hal/arch/x86/src/x86_64/{gdt.rs,idt.rs,idt/entry.rs,idt/dispatch.rs,idt/fatal.rs,syscall.rs}` and page-table mappings; kernel entry init/AP setup and `kernel/src/task/{user_copy,copy_glue}` only where required. Reuse IDT/domain runners. Classify added state under Phase 02 when that gate is available.

## Implementation Steps

1. Map supported CPU/CR3/CPL transitions, known U=1 accesses and callback execution. Establish source-pinned old behavior.
2. Install guarded per-CPU IST mappings before loading gates. Use layout/offset checks against architectural TSS and hardware frame formats.
3. Implement assembly-safe GS/CR3 entry and AC handling; preserve fatal policy and bounded diagnostics.
4. Add a private assembly-only rendezvous at each vulnerable transition stage. It must be reachable with invalid/noncanonical user RSP, mapped in that CR3, use no user-stack or ordinary Rust call, and be releasable via a prevalidated host/debug or handler mechanism. Record the mechanism before implementing it; do not assume a magic runner flag can write guest memory.
5. Inject NMI/#DF/#MC where QEMU supports it; require vector, origin, actual IST RSP bounds and recorded terminal outcome. Fatal-policy tests use a fresh boot per case. Where #MC/nesting cannot be faithfully injected, mark that scenario pending; `int 18` is not hardware MCE proof.
6. Evaluate protection options without copying regressions: if an option cannot preserve legitimate SAS ownership/data paths with bounded audited windows, leave that option disabled for that profile and record its remaining acceptance gate, never declare it qualified. Entry correctness still lands independently.
7. Run existing IDT/domain regression scenarios plus native SAS IPC, driver MMIO/DMA and relevant hypervisor paths; compare Phase 03 costs when available.

## Success Criteria

- [ ] Deterministic entry-window injection uses the expected guarded IST under CPL0/CPL3 and private/shared CR3, with exact recorded outcome; negative control lacks that protection.
- [ ] NMI/#DF/#MC stacks cannot alias per CPU; AP execution and guard faults are observed where supported.
- [ ] Existing copied IPC and authorized local borrowed/grant paths still work; no new bounce copies or CR3 switch on SAS path.
- [ ] Enabled protection options pass positive authorized access and negative unauthorized access/AC-set entry tests; disabled/unsupported options are labeled pending, not PASS.
- [ ] Fault fixup and nested exception paths balance AC. Production image contains no rendezvous/injection path.

## Assumptions

QEMU support for real MCE, AP-targeted NMI and exact transition rendezvous must be established before claiming coverage. Private mappings of entry substrate must be inspected, not inferred from a global kernel mapping.

## Security and risk assessment

A bad entry mapping/GS change is node-fatal; use isolated images and preserve a known-good cold-boot artifact. Do not promote LBI into a hostile-code guarantee. Undo by reverting this slice and cold boot; no persistent state changed.

## Deviation Log

SAS review: dropped mandatory SMAP/copy migration and unrequested resumable NMI; protection knobs are distinct qualified options. Removed Phase 02/03 tooling/KVM from the security-fix critical path.
