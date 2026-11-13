---
phase: 14
title: "Kernel arch seam and boot split"
status: in-progress
priority: P2
effort: "10–14d"
dependencies: [9]
tier: thinking
checkpoint: false
---

# Phase 14: Kernel `arch/` seam and boot split

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-9, Medium IPC/memory/hardware](../reports/architecture-review-261110.md) · `hal/core/src/lib.rs` · `hal/traits/` · `kernel/src/main.rs` · `kernel/src/board.rs` · `kernel/src/platform.rs` · `kernel/src/task.rs` (`yield_cpu`) · `kernel/src/memory/paging.rs` · `kernel/src/task/user_copy/sv39_probe.rs` · [Spec 04](../../docs/specs/04-hardware.md) · [ADR-0029](../../docs/decisions/0029-platform-image-families-and-runtime-detection.md) · owner decision 4

## Overview

Give the kernel one place where architecture differences live, so x86 work stops paying a cfg tax across 92 files and parked architectures are quarantined rather than interleaved.

## Key Insights

- Reviewer-cited: `Timer`, `InterruptController`, `BootController` have zero implementors; `ViHypervisor` is stub-only; kernel names `hal::aarch64::` 103× and `hal::x86_64::` 36×; 1,807 `target_arch` lines (riscv64 667, x86_64 630); 123 `asm!` sites in 31 kernel files; `kmain` (`main.rs:151-1781`) has 124 cfgs and enforces "VT-d before any DMA driver" by position.
- `yield_cpu` (`task.rs:1378-1917`) has two near-duplicate switch bodies and three HAL entry points.
- x86 has two page-table paths (`paging.rs` raw PTE flags vs `hal::PageTableTrait`), and walkers copied into `task/user_copy/`.
- Three board-selection models with four accessor names.

## Requirements

- Functional: `kernel/src/arch/{x86_64,aarch64,riscv64}/` export the same function set (`irq`, `clock`, `power`, `pci_irq`, `iommu`, `smp`, `user_access`, `page_probe`, `boot::platform_init`) selected by one `cfg` at `mod arch`; `arch::boot::platform_init()` returns a typed `PlatformReady`; `kmain` keeps the arch-neutral tail; `hal::arch::SwitchPlan` with one `switch(plan)` per architecture and an RAII interrupt-state type; x86 `KERNEL_ROOT` uses the HAL page-table trait; `leaf(root, va)` and `current_root()` live in the HAL; one `board::active()`; unimplemented HAL traits deleted (or implemented where one line suffices); 32-bit targets per decision 4.
- Non-functional: move-first, then unify; no behaviour change; CI guard caps `target_arch` occurrences outside `arch/` and ratchets down.

## Architecture

```
kernel/src/arch/mod.rs:  #[cfg(target_arch="x86_64")] mod x86_64; pub use x86_64::*;   (one cfg per arch)
kmain: arch::boot::platform_init() ─► PlatformReady { console, irq, clock, pci, iommu } ─► neutral init
sched: plan = SwitchPlan { out, in, root, asid, irq } ; arch::switch(plan)
```

`PlatformReady.iommu` being a required field encodes the ordering that is positional today.

## Assumptions

- **Claim:** the two `yield_cpu` bodies differ only in root/ASID handling, so a null root expresses the SAS case.
  **Confidence:** low
  **How to verify:** diff the two bodies line by line before designing `SwitchPlan`.
- **Claim:** `scripts/check-hal-boundaries.sh` can host the `target_arch` ratchet.
  **Confidence:** medium
  **How to verify:** read the script.
- **Claim:** x86 raw-PTE path has no flag the HAL trait cannot express.
  **Confidence:** low
  **How to verify:** compare `page_protect.rs:229` `pte_bits_from_flags` with `hal/arch/x86/src/x86_64/paging.rs:385`.

## Related Code Files

- Create: `kernel/src/arch/{mod.rs,x86_64/,aarch64/,riscv64/,legacy/}`
- Modify: `kernel/src/main.rs`, `kernel/src/board.rs`, `kernel/src/platform.rs`, scheduler switch path, `kernel/src/memory/{paging,page_protect}.rs`, `kernel/src/task/user_copy/**`, `hal/core/src/lib.rs`, `hal/arch/*/` (add `SwitchPlan`, page probe, user-access window), `hal/core/Cargo.toml` and `kernel/build.rs` (default arch → x86_64), `scripts/check-hal-boundaries.sh`, `docs/specs/04-hardware.md`, `docs/system-architecture.md` (board-feature sentence)
- Delete: `hal/traits/{timer,interrupt}` and `BootController` if left unimplemented, `ViHypervisor` stubs, `boards` `aarch64_catalog()` if unused

## Implementation Steps

1. Record the baseline: `target_arch` count per directory; add the ratchet guard at today's numbers.
2. Create `arch/` and move code file-by-file (asm sites, direct `hal::<arch>` uses) with no logic edits; lower the ratchet each commit.
3. Split `kmain` into per-arch `platform_init` + neutral tail; x86 first, boot lane after each arch.
4. `SwitchPlan`: diff the bodies, implement per arch, keep the old path behind a test-posture A/B switch for one cycle, then delete.
5. Page tables: route x86 shared root through the HAL trait; move walkers and current-root readers into the HAL; rename Sv39-specific vocabulary.
6. Unify board accessor; delete dead traits; flip defaults to x86_64; quarantine or keep 32-bit code per decision 4.
7. Correct Spec 04 §1/§7 to the real seam.

## Todo List

- [x] Ratchet guard
- [ ] Code moved into `arch/`
- [x] `kmain` split
- [ ] `SwitchPlan`
- [ ] Single page-table path
- [ ] Board accessor, dead traits, defaults

## Success Criteria

- [ ] `target_arch` occurrences outside `kernel/src/arch/` reduced by at least half from the recorded baseline, and the guard fails when one is added.
- [ ] No `asm!` outside `kernel/src/arch/` and `hal/`.
- [ ] x86 1/2/4-CPU domain lanes, RV64 `--harts 2`, AArch64 `virt-vhe` 2-CPU lane pass; context-switch A/B fixture shows identical markers before the old path is deleted.
- [ ] Kernel builds for all three 64-bit targets with no `cfg_attr(not(target_arch…), allow(dead_code))` added.

## Risk Assessment

- Context switch and paging are the highest-risk code in the kernel; the A/B cycle and per-arch boot lanes are mandatory, not optional. Rollback: revert per step; nothing persistent.
- AArch64/RV64 have less CI coverage on hosted runners; run their SMP lanes locally and record logs.

## Security Considerations

A single page-table path removes the chance that Tier-1 and Tier-2 mappings disagree on flag encoding. No authority change.

## Next Steps

Phase 15 ports Tier-2 grants to x86 using the neutral copy/probe interface.

## Deviation Log

- **Decision (step 1, 2026-11-13):** census in [research/p14-census.md](research/p14-census.md). Ratchet added to `scripts/check-hal-boundaries.sh` at 1 837 `target_arch` lines / 124 `asm!` sites outside `kernel/src/arch/`.
- **Surprise (step 1):** 32-bit targets do not build today (riscv32 17 errors, arm 13, x86-32 stops in `libs/abi` on the `ViVmExit` size assert). "Build-check only" for `arch/legacy` is therefore not available without an ABI change; the code is quarantined unbuilt, as before. Not fixed here.
- **Decision (step 1):** the 486 lines of `any(riscv64, aarch64, x86_64)` exist only to exclude 32-bit; `build.rs` emits `cellos_wide` (same set as `posture.rs`) and they collapse to it. Counted toward the ratchet.
- **Decision (step 1):** `asm!` sites outside the nine function sets get `arch::{tlb, cpu, pio}`; hypervisor-owned asm moves with its module.
- **Surprise (step 1):** the kernel already has `sched::domain_switch::SwitchPlan`; the HAL type needs another name or the old one is renamed. Body B of `yield_cpu` lacks the dispatch marker, and only builds for 32-bit/compile-only; x86 does not write CR3 in the switch (root reaches user entry through `gs:[24]`).
- **Decision (step 1):** `kmain` splits in two arch phases (`early()` → heap → `platform_init()`); `PlatformReady.iommu` is optional (aarch64 starts VirtIO DMA with no IOMMU).
- **Note (step 1):** RV64 `--harts 2` and AArch64 test-hooks lanes were red at Phase 10 base; baselines are taken before step 4.
- **Decision (step 2, M0):** `kernel/src/arch/mod.rs` selects each arch with a `#[cfg]` on its `mod` and on its `pub use <arch>::*`; the glob re-export of an arch is added in the commit that gives it content (an empty glob is an unused import, and the domain-test build denies warnings). `posture::wide` is the one definition of the 64-bit set; `build.rs` emits `cellos_wide` from it. `scripts/mutable-globals.toml` gains `cfg_true = ["cellos_wide"]` and its keys follow the rewritten cfg chains; the generated `docs/generated/{mutable-globals,lock-ranks}.md` are rewritten (the first was already stale at base: 4 unclassified `init_death` statics remain, not this phase's).
- **Decision (step 2, M1):** moved modules keep their old place in the module tree under `arch` (`crate::dev::pcie_msi` → `crate::arch::dev::pcie_msi`, served by the arch glob re-export); `kernel/residents.toml` entries move with the files and keep their category and owner. Items that were `pub(super)` for the old parent become `pub(crate)`; children that reached the old parent by `super::` name it by `crate::` path. No other edits.
- **Decision (step 2, M3–M8, owner via Main):** asm moves are byte-identical, `options(...)` included; call sites whose blocks differ in text or options get separate `crate::arch` functions (e.g. `user_access::{syscall_open, boot_open, secondary_open, guard_open, sum_mask_set}`, `irq::{raise_local_resched, pend_local_ssip}`, `cpu::{stack_pointer, oom_stack_pointer}`). A no-op exists only where the original was a no-op on that arch (`tlb` on 32-bit).
- **Deviation (step 2):** M7 (`pci_irq`/`iommu` cfg and `hal::aarch64` paths) and the x86 half of M8 (`sched/smp.rs` x86 AP bring-up, shootdown/handoff fixtures and their two `global_asm!` blocks) are not moved yet; 20 `asm!` lines remain outside `arch/` (smp.rs x86 13, task.rs yield masks 2 = step 4, main.rs CurrentEL + rv64 fault snapshot 2 = step 3, layer2_selftest 4 … see the step-2 report).
- **Decision (step 2b):** the remaining rows use two shapes only. Where every arch has an arm, the arms become a neutral set in `crate::arch::<set>` with the same signature on x86_64, aarch64, riscv64 and `legacy` (new sets: `iommu`, `virtio_mmio`, `gpio_irq`, `sdhci`, `hart_local`, `vm`); the shared function keeps its docs and forwards. Where single-arch items read their module's private state, they move verbatim into a `#[path]` child module under `arch/<arch>/` (`dev/pcie_ecam.rs` x86, `resource_registry.rs` aarch64, `sched/smp.rs` aarch64 and riscv64, `hart_local/ready.rs`, `thread_user_entry_selftest.rs` riscv64 test fixtures), as the x86 SMP block did; only `pub(super)` (or `pub(in …)`) is added. An arch whose arm was a no-op or a fixed value keeps exactly that.
- **Decision (step 2b):** `tools/mutable-globals` normalizes `..` in `#[path]` module files, so `#[path = "../arch/…"]` children are keyed by their real file; the `smp_bringup.rs` `[[unreached]]` acknowledgement is gone.
- **Decision (step 2b):** `#[path]` child files keep their parent's `kernel/residents.toml` category and owner; neutral-set files are `A`/`arch`.
- **Deviation (step 2b):** `hypervisor::registry::pi_monitor_smoke` stays as an aarch64-only forwarder to `crate::arch::vm::pi_monitor_smoke` because its caller is `main.rs` (step 3).
- **Left in place (step 2b, needs a logic edit or gains nothing):** `sched/smp.rs` `send_ipi` and `smp_hart_entry`'s non-riscv64 `hart_id`/halt arms (the entry is exported on every target though only rv64 boot calls it; dropping it elsewhere changes the image); `sched/quiesce.rs` (riscv64 is the park-hook backend of an arch-neutral protocol, its `lane` fixture's helpers are called from the shared park path, so a child module needs visibility edits for at most 1 line); `memory/tlb_shootdown.rs` `flush_range` arms (the x86 arm reads the private remote-ack wait, the riscv64 arm a test-hooks control; the arms interleave with shared ordering); `loader/domain_admission.rs` (its `target_arch` lines are capability predicates that decide admission, not arch mechanism); `task/syscall/dev.rs` x86/non-x86 function pairs and `grant.rs` `anon_memory` (policy per arch, a child saves ≤2 lines); `task/copy_glue/mod.rs` and `task/tcb.rs` have no `target_arch` line left after M0; `dev/iommu.rs` `cfg_attr(..., allow(dead_code))` on the x86-only/riscv64-only helpers stays (still dead on the other arches).
- **Deviation (step 2b):** row 5 (per-arch arms in `task/syscall/{grant,dev,misc}.rs`, `memory/tlb_shootdown.rs`, `loader/domain_admission.rs`) is not done: the run hit its request budget. Ready next: `tlb_shootdown` `counter_now`/`tag_probe_ticks` → `arch::tlb` (−6) plus its riscv64 test-hooks items as a child (−4); `grant.rs` DMA granule and cache-sync arms → an `arch::dma` set (−6); `misc.rs` getrandom-sas-test items as a riscv64 child (−5).
- **Decision (step 3, x86):** `kmain` is `arch::boot::early(hartid, dtb)` → heap and RT heap (neutral) → `arch::boot::platform_init(early) -> PlatformReady` → neutral tail. Neutral code that sat inside an arch range (banner, boot memory and frame allocator, the `HAL initialized` line, `dev::uart::init`, `reserve_snapshot_scratch`) stays neutral (`crate::boot_banner`, `crate::boot_memory`, `crate::log_info`) and the arch phase calls it at its old position. `PlatformReady` and its parts live in `crate::boot::platform_ready` as `pub` types, so a field one arch does not read raises no dead-code lint; the interrupt enable is `IrqReady::enable(ClockReady)`, the Platform Cell spawn takes `&IommuReady`, and `ConsoleReady` keeps the boot description for the x86 framebuffer console (read at its old point, not earlier).
- **Decision (step 3):** the arch code left in the tail runs through fixed-point hooks in `arch::<a>::boot` (`fixtures_after_policy`, `fixtures_after_task_init`, `fixtures_after_loader_corpus`, `fixtures_after_secondaries`, `supervisor_device_spans`, `browser_engine_admission`, `fixtures_domain`, `fixtures_park`, `fixtures_hardware`, `framebuffer_console`, `diag_probe`, `fixtures_user_mode`, `quiet_log_level`, `idle_probes`). An arch with no code at a point gets an empty hook: nothing ran there before either.
- **Decision (step 3):** the split lands one arch per commit. Until its commit an arch keeps the unsplit `kmain` body under `cfg(not(<split arches>))`; the legacy commit deletes it.
- **Decision (step 3, aarch64):** the `CurrentEL` read is `arch::cpu::current_el()` (asm and options byte-identical; still read before the `[pi-monitor]` line is formatted); `platform_init` calls `crate::arch::vm::pi_monitor_smoke` directly and the `hypervisor::registry` forwarder kept for `main.rs` (step 2b) is gone. `PlatformReady.iommu` is `Absent` (VirtIO MMIO DMA starts in `dev::init` with no IOMMU). The idle-probe counter's `mutable-globals` key follows it into `arch::aarch64::boot::idle_probes`.
- **Decision (step 3, riscv64):** the fault snapshot (`vi_rv64_kernel_fault_snapshot`, its `csrw satp` asm unchanged) and `TEST_SAFE_ROOT` move into `arch::riscv64::boot`; `main.rs` has no `asm!` left. `PlatformReady` carries `pci: Some(Gpex)`, `iommu: Deferred` (`try_deferred_init` stays in the `RegisterPciDevice` path) and `storage: VirtioMmio`. The supervisor MMIO spans become `supervisor_device_spans()` (CLINT, PLIC, the UART page when there is a UART, RTC), registered by one neutral loop in the same order.
- **Decision (step 3, legacy):** the 32-bit targets get `arch::legacy::boot` (UART, `ARCH.init`, published allocator, bare-physical paging, `PlatformReady` with no PCI, `Absent` IOMMU, no storage) and the unsplit `kmain` is deleted. The panic handler writes through `arch::boot::putc` (`#[inline(always)]`, the old `panic_putchar` arms) and reboots through `arch::power::reset_after_panic` (SBI SRST on riscv64/riscv32, nothing elsewhere); `qemu_exit` forwards to `arch::power::qemu_exit`. `main.rs` keeps one `target_arch` line, the `qemu-dev-https` compile guard. The legacy code still does not build (step 1); `cargo check --features compile-only` for riscv32imac and armv7a reports the same errors and warnings as the base tree (17 and 13 errors).
- **Decision (step 3, move):** the per-arch blocks of `boot.rs`, `platform.rs` and `cpu_features.rs` move under `arch/<a>/boot/` as `firmware` (fallback memory map; on aarch64 also `firmware_smp`, the ECAM note and the `FDT_NOP`/backend tree notes, now called right after `effective_dtb`), `platform` (riscv64 DTB parser and `PlatformInfo` helpers, aarch64 backend facts) and `cpu_features` (x86 virt vendor and root entry, riscv64 H-extension, aarch64 EL2 latch and monitor verdict). What both DTB arches share stays in `crate::boot` under `any(riscv64, aarch64)` (`dtb_memory`, `fallback_memory_entry`, `DTB_MEMORY_MAP`, `DTB_BOOT_INFO`), as does `firmware_cpus` (`any(aarch64, test)`, host-tested); the neutral statics (`HAS_*`, `PLATFORM`, `DTB_*`) and `SimpleBootInfo`'s fields become `pub(crate)` so the arch files reach them. `has_el2` asks `arch::boot::cpu_features::el2_monitor_verified()` first (aarch64 board monitor; `None` elsewhere). The x86_64/32-bit `platform::init` no-op had no caller left and is deleted. `mutable-globals` keys follow `X86_VIRT_KIND` and the x86 fallback map.
- **Decision (step 3, board):** one `board::active()` on all three arches (riscv64 and aarch64: the validated compiled-in descriptor; x86_64: the bound profile, panicking before binding as `selected()` did); `const fn board::compiled()` serves the riscv64 and aarch64 const fallback maps; the SoC policy is `arch::<a>::board::soc()` (riscv64 profile match, x86_64 `x86_soc_profile(active())`, now `pub(crate)`). `selected`, `selected_riscv64_board`, `default_aarch64_board`, `selected_riscv64_soc` and `selected_x86_64_soc` are gone; `x86_machine_inputs()` stays (syscall 433 answers before binding).
- **Deviation (step 3):** `PlatformReady.iommu` is a required `IommuReady` (`VtdActive` | `Deferred` | `Absent { reason }`), not the optional field step 1 proposed: aarch64 (VirtIO MMIO DMA, no IOMMU) and x86_64 without a validated MCFG (no PCIe, so no PCI DMA device) are `Absent`.
- **Decision (step 4, HAL):** each 64-bit HAL exports `Root { base, asid }` (riscv64: the table's PPN; aarch64, x86_64: its physical address; a zero base is no root), `#[must_use] IrqSaved` (`!Send`, restores on drop), `SwitchPlan { out, inc, root: Option<Root>, irq }`, `switch(plan) -> IrqSaved` and `set_kernel_stack_for(&Context)`. `root: None` writes no root on riscv64 and aarch64; on x86_64, which never writes CR3 in the switch, it inherits the incoming context's saved `user_cr3`. riscv64 hands the token's `sstatus` to `__switch` and returns a spent token; aarch64 and x86_64 return the caller's token, which `yield_cpu` drops after `tls::apply_on_resume` as before. The x86_64 token keeps the scheduler's own `pushfq; pop; cli` and `push; popfq` blocks byte for byte (no `nomem`, unlike the HAL's `save_and_disable_interrupts`). The free `switch(old, new)` aliases on aarch64 and x86_64 collided with the new entry and are gone; their one caller each (`Arch::switch_context`) calls `CpuContext::switch`, the same zero-root switch.
- **Decision (step 4, kernel):** the seam is `crate::arch::sched`: `MASK_AT_ENTRY` (riscv64, x86_64 mask on entry; aarch64 at selection, so the housekeeping drains keep the caller's IRQ state), `REPORTS_DISPATCH` (aarch64, x86_64 log `[sched] hart N dispatched a task`), `root_for_table` (the root encoding `domain_switch` used to cfg), and the HAL types re-exported. `arch/legacy/sched.rs` gives the 32-bit targets an empty token, an uninhabited `Root`, `Context::switch` and their `set_kernel_stack` arms. `sched::domain_switch::SwitchPlan` is now `RootDecision`; its root side effects (domain identity, safe-root pending, activation counters, `S22-RV64-SWITCH`) run only in `RootDecision::switch`, which consumes the decision and performs the HAL switch. `yield_cpu` has one body: the old body B (builds without `native-domains`) is a root-less `SasSwitch` on the same path, which also passes `note_task_dispatch` on aarch64/x86_64; those 64-bit builds only exist as `compile-only` and do not compile at base (`task/syscall/grant.rs` E0433/E0609, unchanged). The pre-selection pass is `drain_before_selection()` (`#[inline(always)]`). No `asm!` remains outside `arch/` and the HAL.
- **Deviation (step 4, fixtures):** PLAN, RESUME-ROOT, PIN-DYING and SAS-FASTPATH read decisions through the side-effect-free `RootDecision::preview(current, task)` instead of `SwitchPlan::new(..).root_switch()`, so no fixture pins the hart, publishes a domain identity or logs `S22-RV64-SWITCH` (the two fixture lines per riscv64 boot are gone; the runner's `switch` case counts genuine Activate lines, at least one). RESUME-ROOT asserts that the re-selected domain programs the same root and publishes nothing; PIN-DYING clears its own `begin_execution` pin. The aarch64 ROOT-SWITCH safe-root handoff (c6d9d49a39) is undone: nothing leaks an identity any more, so the witness's first raw switch is root-less again. The fixture address spaces drop when their fixture returns (their `DOMAIN-FRAME-RELEASE` lines come earlier) instead of at the first scheduler switch after them.
- **Decision (step 4, guardrails, `test-hooks` only):** `RootDecision::new` panics, naming the tid, when the incoming Context has `sp == 0` or its task has no kernel stack (the riscv64 trap-overflow root cause); `RootDecision::switch` notes a root-less switch per hart, and `vi_context_switch_complete` asserts before the safe-root acknowledge that such a switch left no domain recorded (the aarch64 DOMAIN-LIVE root cause).
- **Decision (step 4, A/B):** `CELLOS_SWITCH_AB=legacy` (read by `kernel/build.rs`, emits `cfg(cellos_switch_legacy)`, refused in release posture) builds the pre-`SwitchPlan` `yield_cpu` verbatim from `kernel/src/arch/switch_ab_old.rs`, a `#[path]` child of `task`, with `RootDecision::root_switch()` for its tuple; `main.rs` allows dead code in that build only. The fixtures are the same in both modes, so the A/B isolates the switch path.
- **Evidence (step 4, A/B fixture, 5dd1cf4fd7):** both paths built from the same commit, old with `CELLOS_SWITCH_AB=legacy` (scratch worktree), new without, and booted on every required lane. Runner `/tmp/p14/ab/ab.sh`, logs `/tmp/p14/ab/{legacy,new}/` (`x86dom-cpu{1,2,4}.log`, `a64th.log`, `a64.log`, `rvdom-qemu/h2-<case>-*/qemu*.log`), diff `/tmp/p14/ab/abdiff.sh` → `/tmp/p14/ab/diff/result.txt`. Markers are every `S22-*` line, `[sched] hart N dispatched a task` and every `[smp]` line, compared as sets with hex values masked and, in `S22-*` lines, decimals masked (ids, generations, counters); hart numbers stay. Lane verdicts are identical in both modes: x86 domain 1/2/4 CPU PASS, AArch64 test-hooks PASS, AArch64 virt-vhe 2 CPU PASS, RV64 native domain `--harts 2` 14/15 PASS with `grant-pair` (last) failing on `S22-RV64-GRANT-PAIR-OWNER-MAPPED` in both, the known shell refusal. Marker sets are identical on all three arches: x86 13/17/20 markers (1/2/4 CPU), aarch64 test-hooks 79, virt-vhe 7, riscv64 65 per case for all 15 cases. The only raw differences are values that vary run to run (x86 `DOMAIN-LIVE` domain/generation per hart and `DOMAIN-TEARDOWN releases=`, aarch64 grant ids, riscv64 `PARK frozen_ticks`) and one x86 4-CPU `[sched] hart 2 dispatched a task` that the old path's run printed split across two lines by another CPU's console output (present once newlines are removed). The old path is deleted in the next commit.
