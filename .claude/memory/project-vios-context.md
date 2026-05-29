---
name: project-vios-context
description: ViOS OS project context, active implementation plan, and phase completion status
metadata:
  type: project
---

ViOS is a `no_std` Rust OS using Cellular Single Address Space (SAS) + Language-Based Isolation (LBI).
Primary target: RISC-V 64 (QEMU virt machine), secondary: AArch64, x86_64.

**Active plan:** `.agents/260528-2016-vios-full-implementation/plan.md` (23 phases, v0.2→v1.0)
**Why:** Session 260528, full feature roadmap from baseline cleanup through CI, services, runtimes, community.

**Phase status (as of 2026-05-29):**
- Phase 01 (Workspace Cleanup): ✅ COMPLETE
- Phase 02 (CI/CD): ✅ COMPLETE
- Phase 03 (Boot Stability + Ring 3): ✅ COMPLETE — fence rw,rw fix; user_hello.rs Ring-3 smoke test; Exit syscall reap fix
- Phase 04 (VirtIO Block Fix): ✅ COMPLETE (code) — MMIO mapping, HHDM-safe DMA, CSR poll-warn, flush() device-check; QEMU smoke test pending
- Phase 05 (Keyboard Fix): ✅ COMPLETE — interrupt storm fixed
- Phase 06 (External ELF Loading): ✅ COMPLETE (code) — SpawnFromPath syscall, early loader, reloc engine, init cell de-embedded; QEMU boot test pending
- Phase 07 (VFS FileHandle IPC): ✅ COMPLETE (code) — CapId/CapPerms, CapTable park/unpark, OpenCap/ReadCap/CloseCap, ostd::fs::File cap-backed, revoke_all_for on exit
- Phase 08 (AArch64 HAL): ✅ COMPLETE (code) — boot/context/trap/paging/gic/timer/uart_pl011; QEMU boot test pending
- Phase 09 (x86_64 HAL): ✅ COMPLETE (code) — uart_16550/gdt/idt/context/paging/apic/timer/syscall
- Phase 10 (Lua C Binding): ✅ COMPLETE — glue/build.rs multi-arch; ffi.rs clean bindings; REPL main.rs
- Phase 11 (Tests): 🟡 PARTIAL — integration harness + coverage script; no_std unit tests deferred
- Phase 12 (Security Audit): ✅ COMPLETE — STRIDE model docs; geiger CI gate; fuzz harnesses
- Phase 13 (VFS Service): 🟡 PARTIAL — extended IPC protocol (ListDir/Stat/Write stub); sub-modules; FAT32 runtime deferred
- Phase 19 (Docs Automation): ✅ COMPLETE — docs.yml, release.yml, cliff.toml, CONTRIBUTING.md, CHANGELOG.md, llms.txt
- Phase 21 (RV32+AArch32 HAL): ✅ COMPLETE (code) — Arch trait impls with cfg guards; context structs; interrupt control

**Next:** Phase 15 (Network Service, unblocked), Phase 23 (Community, needs Phase 19). Phases 14/16/18 blocked by Phase 13.

**How to apply:** When resuming, all code-complete phases need QEMU end-to-end verification. Phase 11 (tests) will drive that.
