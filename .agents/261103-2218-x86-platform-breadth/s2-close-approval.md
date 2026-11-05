# Slice S2 (phases 06, 07a, 07b) — closing checkpoint

Recorded 2026-11-05. The owner (the user) answered **"Ký, đóng S2"** to the S2 closing checkpoint (H-S2-11: the 07b checkpoint is S2 close with an independent race review).

**Delivered at the QEMU TCG + local KVM ceiling (nested KVM on WSL2, QEMU 8.2.2):**
- **06:** ACPI reset, S5 poweroff, the SCI power button and `SystemPower = 436` (frozen, [CP2-06](cp2-06-system-power-approval.md)); `/bin/supervisor` ships in every x86 image ([grant](s2-phase06-supervisor-image-grant.md)).
- **07a:** per-CPU state sized by a per-arch `MAX_HARTS` (x86 64, others 2) and a write-once `CpuTopology` table.
- **07b:** every MP CPU up to 64 online, set-based shootdown with fail-stop kept, N-way stealing with RT excluded, per-AP kvmclock. Review PASS-with-fixes (fixes applied); stress 10/10 after the fixture fix.
- **R1:** batched shootdowns for MMIO mapping ([grant](s2-phase07b-paging-grant.md)), ELF loading and W^X lowering ([grant](s2-phase07b-flush-batching-grant.md)). Review PASS (4 Minor fixed in `ea6fdc533`).
- Evidence: [06](reports/phase-06-evidence.md), [07a](reports/phase-07a-evidence.md), [07b and R1](reports/phase-07b-evidence.md).

**Effect:** S2 is closed. Its file-bounded exception, the kernel-repair Phase 07 line grant and the `260808-1544` narrowing end with the slice. The Intel WIP lane is released once branch `feat/x86-s2-flush-batching` is merged into `main`; the owner chose to merge it after the concurrent security session commits its edits to `kernel/src/loader/elf.rs`. Not claimed: physical Intel, VMware, more than 8 CPUs, bare-metal VMX. All 38 lock relations keep their whole-plan scope.
