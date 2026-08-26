# x86_64 HAL Full Bring-up Plan

**Status:** In Progress  
**Priority:** High — G2 prerequisite  
**Plan folder:** `.agents/260607-1543-x86-hal-bringup/`  
**Red-team:** Completed — 4 blocking issues added as Phase 00 + Phase 01 updates

---

## Goal

Boot ViCell kernel on QEMU `x86_64-q35` to the `[ViCell]` banner + scheduler idle loop,
using the complete x86_64 HAL: APIC, IDT, 4-level paging, SYSCALL/SYSRET, and HPET-calibrated timer.

**Scope of Phase 06 success:** serial banner + `Scheduler initialized` — ring-3 cell
spawning on x86_64 requires a separate plan (ELF loader and `spawn_from_mem` trap-frame
setup are RISC-V-specific and must be x86_64-ported later).

---

## Phase Overview

| Phase | Slug | Status | Blocks |
|-------|------|--------|--------|
| 00 | [x86_64 HAL `arch` module + ViTrapFrame](phase-00-arch-module.md) | TODO | ALL |
| 01 | [Kernel compile wiring](phase-01-kernel-compile-wiring.md) | Partial ✓¹ | 02, 03 |
| 02 | [Boot entry + `_start`](phase-02-boot-entry.md) | TODO | 03, 06 |
| 03 | [Kernel `kmain` x86_64 path](phase-03-kmain-x86.md) | TODO | 04, 06 |
| 04 | [HPET driver](phase-04-hpet.md) | TODO | 05 |
| 05 | [SYSRET + IDT hardening](phase-05-sysret-idt-hardening.md) | TODO | 06 |
| 06 | [QEMU q35 run script + boot verify](phase-06-qemu-verify.md) | TODO | — |

¹ Cargo.toml x86_64 dep already added. Remaining: linker sections + limine.rs fixes.

**Parallel-eligible:** Phase 04 (HPET) can run in parallel with Phase 03 after Phase 00+01+02 complete.

---

## Red-Team Findings Incorporated

| Finding | Severity | Resolution |
|---------|----------|------------|
| B1: `hal::arch` module missing from x86_64 HAL | Blocking | New Phase 00 |
| B2: `BOOT_CONTEXT`, `FALLBACK_BOOT_INFO` x86_64 arm missing | Blocking | Phase 03 |
| B3: Cargo.toml x86_64 dep | Blocking | Done ✓ |
| H1: ARCH.init() must split — GDT/IDT early, HPET late | High | Phase 04 uses `init_timers()` free fn after paging |
| H2: BASE_REVISION struct wrong (5 words, not 3) | High | Phase 01 |
| H3: SYSRET canonicality check guards RCX (via RAX scratch) | High | Phase 05 |
| M3: ring-3 cell spawning deferred — init ELF riscv64-only | Medium | Phase 06 success criteria narrowed |

---

## Key Dependencies

- Phase 00 (HAL `arch` module) blocks all others — without it, x86_64 kernel doesn't link
- Phase 01 (linker + limine fixes) + Phase 02 (`_start`) required before any QEMU test
- HPET (Phase 04) call must be AFTER paging activated — use `init_timers()` free fn in kmain
- SYSRET fix (Phase 05) required before any ring-3 code ever runs on x86_64

---

## What Already Works (No Changes Needed)

| Component | File | Notes |
|-----------|------|-------|
| GDT + TSS | `hal/arch/x86/src/x86_64/gdt.rs` | Complete |
| IDT (256 vectors) | `hal/arch/x86/src/x86_64/idt.rs` | Complete, hardening in Ph05 |
| 4-level paging + CR3 | `hal/arch/x86/src/x86_64/paging.rs` | Complete |
| LAPIC (periodic) | `hal/arch/x86/src/x86_64/apic.rs` | Complete, calibration in Ph04 |
| SYSCALL/SYSRET | `hal/arch/x86/src/x86_64/syscall.rs` | Complete, CVE fix in Ph05 |
| UART 16550 COM1 | `hal/arch/x86/src/x86_64/uart_16550.rs` | Complete |
| hal-core feature | `hal/core/Cargo.toml` features | `x86_64` feature already wired |
| Kernel Cargo.toml dep | `kernel/Cargo.toml` | Added ✓ |
| Rust toolchain target | `rust-toolchain.toml` | `x86_64-unknown-none` listed |
