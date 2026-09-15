# Phase 04 — Restore the kernel root across the trap boundary

**Status**: design — no code written
**Ceiling**: qemu (riscv64), then the hosted CI gate
**Evidence for the defect**: [phase-03-console-fault.md](phase-03-console-fault.md) — the panic fires
with `satp` naming a cell root (ASID 1) instead of the kernel's (ASID 0) while S-mode code loads the
UART's line-status register.

## The invariant to establish

> S-mode kernel code always executes with `satp == KERNEL_SATP` (ASID 0). A private domain root is
> installed only while the Cell's own code is executing.

Today neither half holds. `asm/trap.S` never touches `satp`, so a trap from a private domain runs the
whole handler — including `vi_timer_tick`'s console poll (`kernel/src/task.rs:832`) and the PLIC
claim path — under the Cell's root. The cell root contains the kernel's registered ranges
(`domain_supervisor_registry`: text, read-only, writable, heap, stacks) but no MMIO, so any UART/PLIC
access faults (phase-03 measurement).

## What has to change

### A. Trap entry installs the kernel root

`hal/arch/riscv/src/rv64/asm/trap.S`, after the frame is saved and before `call vi_trap_handler`:
read `satp`; if its ASID is non-zero, install the kernel root and `sfence.vma`. The kernel root must
be readable from the vector without a call, which is what a fixed-name symbol
(`VI_KERNEL_SATP`-style, already prototyped and then removed as unread in the diagnostic commit)
is for — `hal/arch/riscv/src/rv64/domain.rs::record_kernel_satp` already writes it.

The frame itself stays valid across the switch: cell kernel stacks are registered as
`SupervisorRangeKind::KernelStack` and therefore map in both the domain root and the kernel root.

### B. The interrupted root has to come back on the way out — and its storage is a real decision

`ViTrapFrame` is a **288-byte layout shared with x86_64 and aarch64** (`hal/arch/arm/src/aarch64/trap.rs`,
`hal/arch/x86/src/x86_64/trap.rs`, `syscall.rs`), and `vi_trap_handler` is a
`hal-arch-trait` hook with the same ABI on all three. Parking the interrupted root requires one of:

| option | cost | hazard |
|---|---|---|
| extra field in `ViTrapFrame` | +8 bytes on every arch, every frame; offsets in three assembly files | cross-arch ABI change, needs its own review |
| extra argument to `vi_trap_handler` | one register on every arch's trap/syscall entry | same review, wider blast radius |
| per-task field written by Rust | no ABI change | needs the value to reach Rust before the root changes: either the asm passes it (same as above) or the Rust side re-derives it from `hart_local::current_domain()` |
| per-hart slot (asm-owned) | cheapest | **unsound** — a handler that yields mid-traffic overwrites the slot, and the resumed frame would restore a foreign Cell's root |

A frame can be suspended across a context switch (the trap handler calls `yield_cpu`), so the parked
root must be per-frame or per-task; a per-hart slot is not an option, however convenient it looks.

### C. The scheduler must re-activate on resume

`SwitchPlan` returns `(0, 0)` for `DomainTransition::SameDomain`, and `switch.S` treats a zero PPN
as "leave `satp` alone" (its own comment: "a zero PPN preserves the SAS/same-domain path"). That is
only correct while nothing else ever changes `satp` — which is exactly what change A breaks. Once the
kernel root can be live when a domain task is resumed, `SameDomain` **must write the root** (or the
resume path must always activate). Without this, a Cell resumes under the kernel root with the
kernel's mappings visible to its S-mode code: a silent isolation failure, strictly worse than the
fault being fixed.

`DomainTransition::ToSafeRoot` and `hart_local::{mark,take,acknowledge}_safe_root*` already model the
opposite transition and give the shape this must follow.

### Disallowed shortcut

Mapping kernel MMIO (UART `0x1000_0000`, PLIC `0x0c00_0000`) into every domain root, or registering
those ranges with `domain_supervisor_registry`: a Cell runs in **S-mode** under that root, so it would
receive the console's and the interrupt controller's MMIO. PMP does not cover this (S-mode has no
second tier here).

## Verification matrix

Acceptance is the phase-03 instrument: the direct QEMU drive (fresh copy of `build/disk_srv.img`,
kernel `cellos-kernel-srv-test`, `posix-shim-test` typed at the shell) with **N consecutive boots and
zero `[KERNEL PANIC]` / `Kernel exception` lines**. `srv-cellosfs` greens do not count — the fault is
intermittent and the suite passed on the same kernel that panicked on the next boot.

| # | check | why |
|---|---|---|
| 1 | phase-03 harness, N ≥ 10 boots, no kernel exception | the fault itself |
| 2 | `cargo test --test srv-cellosfs` (all three tests) | syscalls, disk I/O, console under the new root discipline |
| 3 | RV64 domain regressions — `S22-RV64-SWITCH`, `S22-RV64-PIN-DYING`, `S22-RV64-REGISTRY` (`kernel/src/task/domain_switch_tests.rs`, `address_space_tests.rs`, `user_copy_tests.rs`) | change C alters the write count per switch; the tests assert the old tuple and must be re-derived, not re-pinned |
| 4 | a second Cell admitted after the first, then both run | a per-hart or non-atomic parked root breaks under interleaving |
| 5 | negative isolation: a Tier 2 Cell must not read kernel memory after a trap round-trip | the failure mode of getting C wrong |
| 6 | preemption under load (`posix-shim-test` plus timer ticks) | the handler yields mid-traffic — the case the per-hart slot cannot survive |
| 7 | x86_64 / aarch64 boot smoke | only if option A/B in §B is chosen; option C touches RV64 only |

## Scope notes

- x86_64 does not have this defect shape (CR3 is not live-swapped by the same path) but shares
  `ViTrapFrame`; keep any frame change cross-arch-consistent or avoid it entirely.
- The `native-domains` feature gate stays: the trap-entry switch must be a no-op on configurations
  without private roots (ASID 0 always) — that is a property of the condition, not a `cfg`, so the
  assembly stays one path.
- The owning lane for the domain substrate is `260823-phase07-rv64-domain-qemu`; this phase is the
  CI-gate consumer of it and must not redesign the substrate's ABI unilaterally.
