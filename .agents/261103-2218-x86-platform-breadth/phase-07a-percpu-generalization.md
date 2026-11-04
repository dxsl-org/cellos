---
phase: 7a
title: "Per-CPU structures for N CPUs (behaviour-preserving at 2)"
status: pending
priority: P1
effort: "3d"
dependencies: [3]
tier: thinking
checkpoint: false
---

# Phase 07a: Per-CPU structures for N CPUs

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `kernel/src/task/smp.rs:14-24,34-35,154-160,185,264-330,732,966+`
- `hal/arch/x86/src/x86_64/syscall.rs:57-61`, `apic.rs:21-25`, `gdt.rs`, `ist.rs`
- `kernel/src/task/hart_local.rs:43`, `hart_local/ready.rs:116`, `cpu_accounting.rs:19`, `memory/heap/p01_owner.rs:16`, `task.rs:1340`, `task/domain_switch_tests.rs:418`, `task/scheduler.rs:265,1533,2002,3010`, `task/quiesce.rs:53,366,1354`, `task/telemetry.rs:35-85,279,475-557`, `memory/deferred_release.rs:70,297-307`, `loader/atomic_publication_tests/denials.rs`, `hal/arch/riscv/src/critical_section.rs:14` (doc), `scripts/x86/p01_ledger_smoke.rs`
- x86 `MAX_CPUS` users: `hal/arch/x86/src/x86_64.rs:255`, `apic.rs:17` (`assert!(MAX_CPUS == 2)`), `:22,46`, `gdt.rs:92-99`, `ist.rs:19-78`, `syscall.rs:58,78,96`
- `.agents/260808-1544-wx-cross-hart-tlb-shootdown/` (locked plan touching TLB shootdown)

## Overview

Remove the structural 2-CPU limit: sizes and mappings become per-architecture constants and tables. The scheduler and the online-CPU count stay unchanged (still 2), so every existing lane must produce identical results.

## Key Insights

- `MAX_HARTS = 2` is baked into array literals (`[AtomicBool::new(false), AtomicBool::new(false)]`), into the `boot ^ 1` physical/logical mapping, and into `assert!(logical == 1)` for x86 APs.
- `HART_RT = 1` (49 uses) is a role, not a count. Keep logical 1 as the RT hart; harts 2..N become additional normal harts in phase 07b.
- `hart_local/ready.rs:116` `[[BucketClaims; 256]; MAX_HARTS]` grows linearly. Check the bss cost at N = 64.

## Requirements

- Functional:
  - `MAX_HARTS` per architecture: x86_64 = 256 (dual-socket Xeons exceed 64 threads), reduced to 64 only if the step-1 size audit shows static per-hart arrays cannot move to boot-time allocation; riscv64/aarch64 stay 2 (no non-Intel scope change, per ADR-0022).
  - All array literals become `[const { … }; MAX_HARTS]`.
  - `logical_to_physical` / `physical_to_logical` become a table filled at boot: x86 from Limine MP order + MADT; RISC-V/ARM keep their current boot-hart swap semantics through the same table.
  - x86 HAL `MAX_CPUS` is replaced by `MAX_HARTS`. GDT/TSS/IST stacks are allocated per started CPU from the frame allocator instead of static arrays, if static size at `MAX_HARTS` exceeds 1 MiB (decide in step 1).
  - `x86_secondary_main` accepts any logical index below `MAX_HARTS`.
  - `start_secondaries` still starts only logical 1 in this phase (count unchanged).
- Non-functional: kernel `.bss` growth at N = 64 recorded; zero behaviour change at 2 CPUs.

## Architecture

`kernel/src/task/smp/topology.rs`: `CpuTopology { logical → physical (APIC ID / hart ID), count }`, written once before APs start and read lock-free afterwards.

## Assumptions

- **Claim:** RISC-V/ARM code only depends on `MAX_HARTS == 2` through the arrays and the `^ 1` mapping. **Confidence:** medium. **How to verify:** `grep -rn 'MAX_HARTS\|HART_RT\|\^ 1' kernel/src hal/arch` before editing.
- **Claim:** the locked TLB-shootdown plan does not hold edits on `smp.rs` that conflict. **Confidence:** low. **How to verify:** read `.agents/cell-contract-0024-locks.md` entry for `260808-1544`; obtain the handoff in phase 00 or a later handoff.

## Related Code Files

- Create: `kernel/src/task/smp/topology.rs` (or `smp_topology.rs` if `smp.rs` stays a file)
- Modify: the files in Context Links

## Implementation Steps

1. Measure the static size of every per-hart array at N = 64; decide static vs allocated for GDT/TSS/IST; log the decision.
2. Make `MAX_HARTS` per-arch and convert the array literals.
3. Add the topology table and replace the `^ 1` mapping on all architectures.
4. x86: replace `MAX_CPUS`, widen `x86_secondary_main`, allocate per-CPU GDT/TSS/IST.
5. Run all lanes on every architecture.

## Todo List

- [ ] Size audit
- [ ] Per-arch MAX_HARTS + arrays
- [ ] Topology table
- [ ] x86 per-CPU allocation
- [ ] All-arch regression

## Success Criteria

- [ ] `cargo test -p cellos-kernel` green; x86 suites green (TCG and KVM, `-smp 2`); riscv64 and aarch64 boot lanes (`scripts/qemu-*`) produce the same markers as before.
- [ ] `qemu-domain-test.sh` topology line unchanged at 2 CPUs.
- [ ] `.bss` delta at N = 64 recorded in evidence.

## Risk Assessment

- Cross-arch regression from the mapping change. Mitigation: ARM/RISC-V lanes in this phase's gate; the mapping table is unit-tested for the boot-hart-1 case (RISC-V boots on an arbitrary hart).

## Security Considerations

None.

## Next Steps

Phase 07b turns on more CPUs.

## Deviation Log

None.
