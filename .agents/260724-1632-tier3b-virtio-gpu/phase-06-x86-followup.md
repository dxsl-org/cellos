# Phase 06 — x86 Follow-up (gated on external effort HV-X86-MMIO)

> **Tracking ref:** the external x86-MMIO-exit work is referenced throughout as **`HV-X86-MMIO`**
> (renamed from the earlier "task P06" to avoid colliding with this "phase 06").

## Context Links
- Plan: [plan.md](plan.md)
- x86 run loop (MMIO unwired): `cells/services/hypervisor/src/run_loop_x86.rs:111-164` (port-I/O only; MMIO dispatch absent, awaiting HV-X86-MMIO).
- ARM64 device model (reuse target): `virtio_gpu.rs` + `virtio_gpu/` (phases 02-04).
- x86 VMM progress (project memory): SVM boots Alpine to busybox; VMX deferred (separate effort).

## Overview
- **Priority:** P3 — **DEFERRED**, not part of the MVP.
- **Status:** deferred
- **Description:** Bring the same 2D virtio-gpu device model to the x86 (SVM) personality once the
  x86 run loop models MMIO exits (HV-X86-MMIO). The device model (`virtio_gpu*`) is arch-generic;
  only the transport wiring and the guest device-advertisement differ.

## Key Insights
- **Hard blocker:** `run_loop_x86.rs` handles PortIn/PortOut/Hlt/Msr but has no MMIO-exit dispatch
  (run_loop_x86.rs:111-164). virtio-mmio requires MMIO read/write exits → nothing to hook the GPU
  device onto until HV-X86-MMIO wires x86 MMIO. This phase cannot start before that lands.
- The device model itself (`virtio_gpu.rs`, `command.rs`, `resource.rs`, `scanout.rs`, `cursor.rs`)
  is `#[cfg(target_arch="aarch64")]`-gated today (main.rs:52-77) but has **no arch-specific code** —
  it uses only `vmm::read/write_guest_memory` + `inject_irq`, both of which have x86 syscall paths
  (vmm.rs:28-35). Promote these modules to arch-generic and gate only the transport glue.
- Alternative x86 transport: virtio-**pci** instead of virtio-mmio (x86 guests conventionally use
  PCI). Decide during HV-X86-MMIO whether x86 gets an MMIO hole (like ARM) or a virtio-pci BAR model
  — the latter is more work but more idiomatic for x86 Linux. This choice is out of scope until
  HV-X86-MMIO lands.
- x86 IRQ delivery differs (no GICv2): the GPU device's `inject_irq` must map to the x86 interrupt
  model HV-X86-MMIO establishes (LAPIC/PIC/MSI). Reuse whatever blk/net use on x86 once they exist there.

## Requirements
1. Device model modules made arch-generic (remove aarch64-only cfg where no asm is involved).
2. x86 transport wiring (MMIO slot or virtio-pci) added to `run_loop_x86.rs` after HV-X86-MMIO.
3. Guest advertisement: DTB is ARM-only; x86 uses ACPI/PCI — advertise the GPU via the x86 guest's
   device-discovery mechanism HV-X86-MMIO defines.
4. Re-run the phase-05 matrix on x86.

## Related Code Files
- **Modify:** `main.rs` (cfg gates), `run_loop_x86.rs` (GPU dispatch), x86 device-advertisement
  (ACPI/PCI equivalent of dtb.rs).
- **Reuse:** all `virtio_gpu*` modules from phases 02-04.

## Implementation Steps (post-HV-X86-MMIO)
1. Un-gate arch-generic device modules; confirm ARM build unaffected.
2. Add x86 MMIO/PCI transport for the GPU slot.
3. Advertise GPU to the x86 guest.
4. Run phase-05 matrix on x86 SVM.

## Todo List
- [ ] (blocked on HV-X86-MMIO) device modules arch-generic
- [ ] (blocked on HV-X86-MMIO) x86 transport wiring
- [ ] (blocked on HV-X86-MMIO) x86 guest device advertisement
- [ ] (blocked on HV-X86-MMIO) phase-05 matrix on x86

## Success Criteria
- x86 Alpine guest gets /dev/dri/card0 and renders into the compositor, same matrix as ARM64.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| HV-X86-MMIO chooses virtio-pci → device model needs a PCI transport shim | M×M | Device model is transport-agnostic; only add a pci-config/BAR adapter, not a rewrite. |
| x86 IRQ model diverges from ARM inject_irq semantics | M×M | Follow the blk/net x86 IRQ pattern HV-X86-MMIO sets; do not invent a GPU-specific path. |

## Security Considerations
Identical hostile-guest boundary as ARM64 (the copy validation is transport-independent).

## Next Steps
Revisit when HV-X86-MMIO (x86 MMIO-exit dispatch) completes. No other phase depends on this.
