---
phase: 7a
title: "Per-CPU structures for N CPUs (behaviour-preserving at 2)"
status: completed (pending S2 close)
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

- [x] Size audit (static kept; see Deviation Log D3 and `reports/phase-07a-evidence.md`)
- [x] Per-arch MAX_HARTS + arrays
- [x] Topology table
- [x] x86 per-CPU tables sized by `MAX_CPUS` = 64 (static, not allocated: D3)
- [x] All-arch regression (c2c-render smoke fails identically at base: S4)

## Success Criteria

- [x] `cargo test -p cellos-kernel` green; x86 suites green (TCG and KVM, `-smp 2`); riscv64 and aarch64 boot lanes (`scripts/qemu-*`) produce the same markers as before. Exception: c2c-render smoke, red at base 26d8f3f3c with the same VFS write error (S4).
- [x] `qemu-domain-test.sh` topology line unchanged at 2 CPUs.
- [x] `.bss` delta at N = 64 recorded in evidence.

## Risk Assessment

- Cross-arch regression from the mapping change. Mitigation: ARM/RISC-V lanes in this phase's gate; the mapping table is unit-tested for the boot-hart-1 case (RISC-V boots on an arbitrary hart).

## Security Considerations

None.

## Next Steps

Phase 07b turns on more CPUs.

## Deviation Log

Line cites are as of the S2 request base `28dee8b3d`; the code was re-located by symbol on `26d8f3f3c`.

- **D1 Deviation (H-S2-5):** x86_64 `MAX_HARTS = 64`, not 256 (Requirements line "x86_64 = 256").
  Why: owner decision H-S2-5 keeps every `u64` hart mask and `ViRetirementV1.remote_hart_mask` exact.
  Impact: `scheduler.rs`, `quiesce.rs`, `cpu_accounting.rs`, `snapshot.rs` stay byte-identical; x86 `MAX_HARTS` is `hal::x86_64::syscall::MAX_CPUS` (64); G1 lane (`cellos_g1_cpu`), riscv64, aarch64 and 32-bit targets keep 2 by `cfg` in `smp.rs`.
  Revert: raise `MAX_CPUS` with a new owner decision on the masks.
- **D2 Deviation (plan-text drift):** Context Links cite `hal/arch/x86/src/x86_64.rs:255` (the `init_secondary_cpu` bound, `:264` at `28dee8b3d`) and `apic.rs:17` (`assert!(MAX_CPUS == 2)`, `:26` at `28dee8b3d`).
  Why: both moved before this phase.
  Impact: none on scope. `init_secondary_cpu` needed no edit (its `logical >= syscall::MAX_CPUS` check now bounds at 64); the `apic.rs` assert is removed with the packed `AtomicU64`.
  Revert: n/a.
- **D3 Decision (step 1, size audit):** GDT/TSS, GS records and IST stacks stay static, sized by `MAX_CPUS` = 64, although the IST region (2.5 MiB) exceeds the plan's 1 MiB threshold for frame allocation.
  Why: every IST stack has unmapped 4 KiB guard pages (`ist::install_guards` refuses huge leaves); the kernel image is 4 KiB-mapped, while frame-allocator memory is reached through the HHDM, which Limine maps with 1 GiB leaves. Allocation would need a guarded-VA allocator in the HAL (not in 07a).
  Impact: x86_64 `.bss` 668,688 → 3,625,552 B (+2,956,864); 257 guard pages are installed at boot (boot panics otherwise; all boots passed). `CLAIMS` (384 KiB at 64) also stays static.
  Revert: move IST/GDT to per-started-CPU allocation once the HAL has a guarded mapping primitive.
- **D4 Decision:** the HAL keeps the name `MAX_CPUS` as the single source (`syscall.rs`); the kernel's x86 `MAX_HARTS` is defined from it with `const _: () = assert!(MAX_HARTS <= MAX_CPUS)` (the plan said "replaced by `MAX_HARTS`").
  Why: the HAL cannot depend on the kernel; G1 keeps a smaller kernel bound under the same HAL.
  Impact: one definition per layer, no duplicate constant.
  Revert: rename only.
- **D5 Decision:** x86 fills `CPU_TOPOLOGY` from Limine MP order only: logical 0 = BSP, logical 1 = first non-BSP MP record (as before); no MADT cross-check, no other CPUs published.
  Why: 07a must not change bring-up; full MP enumeration and the `cpu N ignored` path are 07b's.
  Impact: `count()` is 2 on x86 after bring-up; the `topology online=2 processor_ids=0;1` line is unchanged.
  Revert: n/a (07b extends it).
- **D6 Decision:** the HAL keeps its own logical → LAPIC table (`apic::CPU_APIC_IDS`, now one `AtomicU32` per CPU) beside `CPU_TOPOLOGY`.
  Why: `send_ipi`/`ioapic_redirect`/MSI run in the HAL and cannot read kernel state. Both tables are write-once and refuse IDs > 255 and conflicting pairs. For the AP the kernel publishes `CPU_TOPOLOGY` first, then the HAL slot; the BSP's HAL slot 0 is registered earlier by `publish_boot_cpu_policy` and re-confirmed in `start_secondaries`. HAL uniqueness without the packed `AtomicU64`: `PENDING|id` claim, SeqCst scan, withdraw on a duplicate (host test `cpu_registration_is_write_once_and_unique`).
  Impact: two copies of the x86 pair, both checked.
  Revert: n/a.
- **D7 Deviation (file set #1, granted by Main as integrator 2026-11-05):** `scripts/mutable-globals.toml` (new `CPU_TOPOLOGY` entry; `CPU_APIC_IDS` and the hal-x86 `critical_section` `MAX_CPUS = 2` text) and `docs/generated/mutable-globals.md` (regenerated with the tool).
  Why: the CI inventory gate fails on any new unclassified static.
  Impact: `--check` reports 37 problems before and after (all 35 unclassified statics and the stale md pre-date 07a, from phases 02-06/04b); the regenerated md now renders those 35 rows as UNCLASSIFIED. The owner sees this at the S2 checkpoint; Main tracks the 35 as a separate repair item.
  Revert: revert the two files with the code.
- **D8 Deviation (scope within `ready.rs`):** `steal_from_busiest_with` returns for `thief > 1` (outside the request's `:19-23,116` lines).
  Why: it computes `victim = 1 - thief`, which would underflow for a thief ≥ 2 now that x86 `MAX_HARTS` is 64. No such thief exists in 07a (at most harts 0 and 1 online), so behaviour is unchanged; the signature is kept (07b rewrites the body).
  Impact: none at 2 CPUs.
  Revert: 07b replaces the body.
- **D9 Decision:** AArch64 `publish_physical_cpu` is write-once (was an overwriting store) and returns `false` on a conflicting pair; `start_psci_secondaries` then does not start that hart.
  Why: the topology contract. No supported board publishes a conflicting pair (boot CPU must be index 0 before any AP is published).
  Impact: none on the default lanes (markers identical, see evidence).
  Revert: n/a.
- **S1 Surprise:** pinned `quiesce.rs` comments (`:53`, `:366`, `:1354`) say the kernel's `MAX_HARTS` sits under `MAX_QUIESCE_HARTS` (8); on x86 it is now 64. Code is unaffected (it fails closed above 8 online harts; host cases use hart ids 3/4 that only quiesce touches). Not edited (pinned); 07b or the quiesce owner should restate them.
- **S2 Surprise:** the G1 lane does not compile at base `26d8f3f3c` either: `E0614` at pinned `scheduler.rs:2131,2622` and clippy `empty line after doc comment` at `domain_switch_tests.rs:27`. The 07a tree produces the identical error list. The `M == MAX_HARTS` assertion in `cpu_accounting.rs:19` is therefore not reached by the compiler on either tree; `MAX_HARTS = 2` under `cellos_g1_cpu` holds by the `cfg` in `smp.rs`.
- **S3 Surprise:** `x86_64-boot` boots `-smp 1` (`boot_x86_bios` passes no `-smp`). The 2-CPU x86 coverage comes from the domain lane (`X86_CPUS=2`), the c2c smoke (`--cpus 2`) and a manual `-smp 2` boot of the canonical ISO through a temporary copy of `scripts/qemu-x86_64-test.sh`.
- **S4 Surprise:** the c2c-render smoke fails at base `26d8f3f3c` and with 07a, at 1 and 2 CPUs, TCG and KVM: `USER: c2c-render: ERROR VFS write /tmp/c2c-render.ppm failed: Err(3)`. Pre-existing, outside 07a (cells/VFS); the topology line before it is unchanged.
- **S5 Surprise:** `hal/arch/x86/src/x86_64/idt/probe_ist.rs:271` prints the literal `guards=9`; with 64 slots the checked count (`guards_installed() == GUARD_PAGES`) is 257, so the IDT fixture's text is now stale although its check passes. `scripts/qemu-x86_64-idt-test.sh:18` expects that literal. Both files are outside the 07a set; not edited.
- **S6 Surprise:** `scripts/qemu-x86_64-idt-test.sh` ends in `FAIL: forbidden failure/skip/reset marker present despite success status` at base `26d8f3f3c` as well. The phase-06 lines `[acpi] FADT: … reset=true …` and `[acpi] power: reset_reg=true …` match its forbidden-word regex. The X86-IDT-SELFTEST/X86-IST/X86-IDT-CPL3 markers all print PASS on both trees. This lane is not in CI; not edited.
