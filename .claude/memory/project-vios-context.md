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

**Phase status (as of 2026-05-28):**
- Phase 01 (Workspace Cleanup): ✅ COMPLETE — profile blocks removed from 16 Cargo.tomls, posix feature added, zero warnings
- Phase 02 (CI/CD): ✅ COMPLETE — ci.yml, security.yml, rust-toolchain.toml, deny.toml, scripts, issue templates
- Phase 03 (Boot Stability + Ring 3): 🔴 PENDING — init_kernel_paging hang + U-mode implementation
- Phase 04 (VirtIO Block Fix): 🟡 PARTIAL — MMIO mapping added to paging, IRQ handling fixed; needs QEMU testing
- Phase 05 (Keyboard Fix): ✅ COMPLETE — interrupt storm fixed (INPUT_DEVICE_IRQ + ack_irq pattern)

**How to apply:** When resuming work, read phases in dependency order; Phase 03 is the next P0 blocker.
