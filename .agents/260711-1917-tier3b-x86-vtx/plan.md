---
title: "Tier 3b: x86_64 hardware-virt VMM (SVM-first / VT-x) booting Alpine Linux"
description: "Port the shipped ARM64-EL2 Tier-3b VMM to x86_64 behind a vendor-neutral HAL: AMD SVM first (TCG-testable), Intel VT-x second (real-HW/KVM). PVH direct-boot, EPT/NPT Stage-2, no-LAPIC MVP, reuse the arch-generic virtio stack + run loop → boot Alpine x86_64 to a serial shell, then virtio-blk→VFS + virtio-net→Net."
status: pending
priority: P2
effort: 10 phases (~5-6K new LOC; heavy reuse of shipped ARM64 cell)
branch: main
tags: [hypervisor, x86_64, vt-x, vmx, svm, ept, pvh, virtio, vmm, tier3b]
created: 2026-07-11
---

# Tier 3b — x86_64 hardware-virtualization VMM (boots Alpine Linux)

Same KVM-style split as the shipped ARM64-EL2 track (`.agents/260613-2134-tier3b-vmm-arm64-el2/`,
DONE): the **kernel** owns all privileged virt ops (VMXON/EFER.SVME, VMCS/VMCB, EPT/NPT,
world-switch, exit decode) via the **already-shipped syscalls 220-227**; the **`cells/services/hypervisor/`
Tier-1 cell** (`#![forbid(unsafe_code)]`) owns guest-image load, device models, virtqueues, and the
run loop. **The syscall ABI, VM registry, virtio-mmio/virtqueue/blk/net/console models, and run-loop
structure already exist and are arch-generic — this port ADDS an x86 platform personality, never
rewrites them.**

**CI-accelerator decision (research #9, evidence in Validation Log): AMD SVM first, behind a
vendor-neutral trait; Intel VT-x second on a non-blocking KVM/real-HW lane.** QEMU TCG emulates SVM
(`target/i386/tcg/system/svm_helper.c`) but has **zero** VMX support and WHPX exposes no nested virt —
so SVM is the *only* path that runs on the existing Windows+QEMU+TCG CI host, preserving the ARM
track's "TCG makes CI cheap" property. Dev/test: `qemu-system-x86_64 -cpu qemu64,+svm -accel tcg`
(SVM, CI); `-accel kvm -cpu host` (VMX/SVM on real hardware).

## Phases

| # | Phase | Effort | Status | Law 1? | Depends on |
|---|-------|--------|--------|--------|------------|
| 01 | [Vendor detect (CPUID) + SVM/VMX root-operation enablement + x86 HypervisorCap gate](phase-01-vendor-detect-root-enable.md) | L | **code ✓ (compiles; QEMU log pending)** | no | — |
| 02 | [Nested paging (EPT/NPT) builder + guest-RAM carve (GPA base 0) + MMIO-unmapped invariant](phase-02-ept-npt-guest-ram.md) | M | **code ✓ (compiles; registry wiring + walk-test pending)** | no | 01 |
| 03 | [VMCS/VMCB + vCPU world-switch + exit decode + **bare-metal smoke (M1)**](phase-03-vmcs-worldswitch-exit.md) | **XL** | **SVM DONE — M1 PASS on QEMU +svm TCG 2026-07-23** (VMX stub deferred to P09) | no | 01,02 |
| 04 | [`ViVmExit` ABI extension (PortIn/PortOut/Hlt/Msr) + x86 registry wiring + smoke cell ⚠️](phase-04-vmexit-abi-registry.md) | M | **ABI DONE (VERSION=2, Law-1 2× confirmed) — api-path smoke PASS 2026-07-23**; userspace cell → P05 | **YES ✓** | 03 |
| 05 | [Hypervisor cell x86 personality: PVH loader + boot info + 16550 UART + 8259 PIC + PIT (no-LAPIC MVP) → **BOOT ALPINE (M2)**](phase-05-cell-pvh-boot-alpine.md) | **XL** | **P05a code-complete 2026-07-23** (cell modules + HAL rework; 3-arch build + M1 re-PASS); **P05b pending** = vmlinux artifact + QEMU boot iteration | no | 04 |
| 06 | [virtio-mmio on x86 guest + virtio-console (reuse arch-generic stack)](phase-06-virtio-mmio-x86.md) | M | pending | no | 05 |
| 07 | [virtio-blk → VFS Cell → mount rootfs (M3)](phase-07-virtio-blk-vfs.md) | M | pending | no | 06 |
| 08 | [virtio-net → Net Cell (M4, apk works)](phase-08-virtio-net.md) | M | pending | no | 06 |
| 09 | [Intel VT-x backend bring-up (KVM/HW lane) + optional LAPIC/APICv upgrade](phase-09-vtx-backend-apic.md) | M | pending | no | 03,06 |
| 10 | [Run scripts (SVM-TCG CI smoke + KVM note) + CI job + ENOSYS→real x86 trait finalize + docs ⚠️](phase-10-run-ci-docs.md) | M | pending | **YES(light)** | 05,07,08 |

## Dependency Graph

```
01 ─► 02 ─► 03 ─► 04 ─► 05 ─┬─► 06 ─┬─► 07 ─┐
                            │       ├─► 08 ─┼─► 10 (CI: blk+net validated)
                            │       └─► 09  │
03 ──────────────────────────────────► 09  │ (VT-x backend reuses P03 world-switch scaffold)
                            └───────────────┘ (10 needs only 05 for boot-to-shell smoke)
```

- **Critical path (Alpine-to-shell, SVM/TCG):** 01 → 02 → 03 → 04 → 05. **P05 is the central deliverable.**
- **Parallelizable after P06:** P07 (blk/VFS), P08 (net) own disjoint cell files → concurrent. P09
  (VT-x backend + APIC) depends on P03 (world-switch scaffold) + P06 (device set to validate) and is
  gated on a real-Intel-HW / nested-KVM lane existing — it is **non-blocking** for the SVM merge cadence.
- **P10** gates on P05 (boot smoke) at minimum; full CI matrix needs P07+P08.
- ⚠️ **Law 1 (2× user confirmation):** **P04** adds `#[repr(C,u8)]` variants to the frozen `ViVmExit`
  (`libs/api/src/abi/hypervisor.rs:17`) + bumps `VERSION` 1→2. **P10** finalizes the multi-arch
  `ViHypervisor` trait shape (light). **No new syscalls, no manifest-flag bump** — 220-227 +
  `MANIFEST_FLAG_HYPERVISOR` already exist and are arch-generic.

## Key Cross-Cutting Invariants

- **Vendor-neutral trait boundary (Law 7):** one `ViHypervisor` impl per vendor (`X86Svm`, `X86Vmx`)
  selected at boot by CPUID; SVM ships first, VMX second. The kernel `registry.rs` and the cell run-loop
  personality dispatch on vendor only where mechanics genuinely differ (enablement, VMCS vs VMCB,
  world-switch instr, exit-reason decode). EPT vs NPT trees, guest memory, device models, boot protocol
  are vendor-agnostic.
- **SAS guest isolation (Law 4):** ViCell kernel/cell frames are NEVER mapped into guest EPT/NPT. Guest
  RAM = one contiguous carve; leave every MMIO GPA (any emulated device) **unmapped** so accesses fault
  out (EPT-violation reason 48 / NPF) — the x86 analog of the ARM Stage-2-unmapped-GICD trick. All
  GPA→HPA arithmetic uses `checked_add` overflow guards (C-x1, mirrors ARM C3).
- **No-LAPIC MVP (research #5):** boot the guest with `nolapic noapic acpi=off` and emulate **only**
  8259 PIC (ports 0x20/0x21/0xA0/0xA1) + 8253/8254 PIT (0x40-0x43, gate 0x61) via the port-I/O exit
  path. PIT is load-bearing even without a timer IRQ — Linux calibrates TSC via CPUID 0x15 with a
  **PIT fallback**. This defers all LAPIC/x2APIC/IOAPIC MMIO emulation to P09 (analog of ARM's
  trap-emulate-GICD-then-GICH-upgrade staging).
- **PVH direct-boot (research #4):** enter the guest at the kernel's 32-bit-protected-mode PHYS32_ENTRY
  (paging off, `%ebx`→`hvm_start_info`), NOT via BIOS/real mode. This avoids **unrestricted-guest**
  (SVM/VMX secondary control) and all real-mode emulation. The guest kernel does its own long-mode
  bring-up; the VMM only supplies a flat GDT + `CR0.PE=1, PG=0` + boot-info struct.
- **IA-32e-mode transition:** guest starts with entry-control "IA-32e mode guest"=0. Trap the guest's
  `CR0.PG` 0→1 write (CR0 guest/host mask); on that exit, set the entry control =1 before resuming.
  Do NOT let it free-run into a VM-entry consistency-check failure.
- **Law 2:** the cell copies guest buffers into `Box<[u8]>` before any `.await` IPC to VFS/Net.
- **run_vcpu preemption budget (C-x2, mirrors ARM C2):** the shipped `sys_run_vcpu(budget_ns)` ABI is
  reused. VMX arms the **VMX-preemption timer** (pin ctrl bit6, value scaled by `IA32_VMX_MISC[4:0]`,
  exit reason 52) → `Preempted`. **SVM has NO preemption timer** → arm a host one-shot timer + physical
  `INTR` intercept to force a `Preempted`-equivalent exit at budget expiry (vendor difference, P03).
- **Host IRQ composition:** enable external-interrupt exiting + "acknowledge interrupt on exit"; the
  VM-exit handler dispatches the acked vector through the existing x86 IDT path — host IDT is untouched
  by guest execution.
- **Capability gate:** only a manifest-declared `hypervisor=true` cell on a kernel that detected SVM or
  VMX gets `HypervisorCap` (`kernel/src/task/cap.rs:164`). Deny-by-default in dispatch.

## Validation Log

### 2026-07-12 — Research (3 parallel haily-researcher agents)

**#9 CI-accelerator decision — SVM first, VMX second (behind a vendor-neutral trait). Evidence:**
- QEMU TCG has **no** Intel VMX emulation. TCG's x86 decoder implements only SVM virtualization opcodes;
  `-cpu ...,+vmx -accel tcg` does not yield a usable VMXON path. The oft-cited "nested-VMX-under-TCG"
  work was never merged to mainline QEMU. A guest cannot `VMLAUNCH` under TCG.
- QEMU TCG **does** emulate AMD SVM (`target/i386/tcg/system/svm_helper.c`, actively maintained);
  `-cpu qemu64,+svm -accel tcg` lets a guest execute `VMRUN` and run an SVM guest in pure software.
- Windows WHPX accelerator does **not** expose nested virtualization to a guest.
- ⇒ On the Windows+QEMU+TCG dev host and x86 CI runners, **SVM is the only hardware-virt path that runs
  without real VT-x hardware.** This is the direct analog of the ARM track's "TCG EL2 works → cheap CI".
  Chose option (c) vendor-dispatch HAL, implement **SVM first** (green TCG CI lane), **VMX second** on a
  KVM/real-Intel-HW lane (P09, non-blocking). EPT/NPT + device models + boot protocol are shared.

**#4 Boot protocol — PVH (not bzImage). Evidence:**
- Industry convergence 3-for-3: Xen PVH, Firecracker (PVH merged 2025-03), cloud-hypervisor (bzImage
  deprecated). PVH enters 32-bit protected mode, paging off, `%ebx`→`hvm_start_info` — no real mode, no
  unrestricted-guest, no zero-page. Minimal VMM CPU-state contract at entry (used in P03/P05):
  `CR0=0x11, CR4=0, EFER=0, EFLAGS=0x2`, flat GDT {null, `0xc09b`, `0xc093`}, `RIP=PHYS32_ENTRY`,
  `RBX=hvm_start_info`. The guest kernel does its own long-mode bring-up.
- Alpine ships PVH: `CONFIG_XEN_PVH=y → select PVH` (aports `main/linux-lts/lts.x86_64.config:52`,
  `virt.x86_64.config:51`). **BUT** the `XEN_ELFNOTE_PHYS32_ENTRY` note (name "Xen", type 18) lives only
  in the uncompressed `vmlinux` ELF, NOT the shipped `vmlinuz-*` bzImage ⇒ **P05 must extract/build
  `vmlinux`** (documented + fallback in P05). `hvm_start_info` fields: magic `0x336ec578`, version 1,
  memmap (e820-style), modlist (initramfs), cmdline, rsdp.

**#5/#3 MVP simplifications adopted:**
- **No-LAPIC MVP:** boot with `acpi=off nolapic noapic`; emulate ONLY 8259 PIC + 8253 PIT via port-I/O.
  PIT is load-bearing (Linux TSC calibration PIT-fallback). Defers all LAPIC/x2APIC/IOAPIC to P09.
  Synergy: `acpi=off` ⇒ `rsdp_paddr=0`, no ACPI tables for M2.
- **PVH avoids unrestricted-guest** (32-bit protected entry is a legal VMX/SVM guest state); EPT/NPT
  mandatory; compute all VMX controls from "true" capability MSRs; trap `CR0.PG` 0→1 for the IA-32e flip.
- **SVM has no preemption timer** ⇒ `budget_ns` via host one-shot timer + INTR intercept (VMX uses the
  native preemption timer). Spiked in P03.

## Resolved Decisions (research-backed — see Validation Log)

- **Boot protocol = PVH** (research #4, 3-for-3 industry convergence). Alpine ships PVH via
  `CONFIG_XEN_PVH=y → select PVH` (aports `main/linux-lts/lts.x86_64.config:52`,
  `virt.x86_64.config:51`), so the PHYS32_ENTRY note **exists** — BUT only in the uncompressed
  `vmlinux` ELF, NOT the shipped `vmlinuz-*` bzImage. **P05 must extract/build `vmlinux`.** Detect note
  via `readelf -n` (name "Xen", type 18). Fallback if extraction fails: bzImage 64-bit protocol (VMM
  owns page tables + long-mode + zero-page) — heavier, documented in P05 Risk Assessment.
- **ACPI/RSDP not needed for M2:** the no-LAPIC MVP passes `acpi=off nolapic noapic`, so Linux skips
  ACPI/APIC probing entirely → `hvm_start_info.rsdp_paddr = 0`, no ACPI tables built. ACPI/RSDP-at-
  0xE0000 (Firecracker pattern) is required only if P09 drops `acpi=off`.

## Open Questions

- **SVM budget mechanism fidelity under TCG:** SVM has no preemption timer; confirm a host one-shot
  timer + physical-INTR intercept yields a clean synchronous `Preempted`-equivalent exit under TCG.
  Spike in P03.
- **`InjectIrq` semantics reuse:** ABI param `intid` is reinterpreted as an x86 interrupt vector (0-255,
  8259 line) — no ABI change, but document the semantic overload in P04.
