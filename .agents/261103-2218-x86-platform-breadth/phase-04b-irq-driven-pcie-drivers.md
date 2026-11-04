---
phase: 4b
title: "IRQ-driven PCIe driver cells"
status: pending
priority: P1
effort: "3d"
dependencies: ["4a"]
tier: medium
checkpoint: false
---

# Phase 04b: IRQ-driven PCIe driver cells

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `cells/drivers/nvme/src/{main,controller,dispatch}.rs`, `cells/drivers/ahci/src/{main,controller}.rs`, `cells/drivers/igb/src/main.rs`, `cells/drivers/e1000/src/controller.rs`, `cells/drivers/xhci/src/controller.rs:303`
- Phase 04a syscalls 434/435

## Overview

Move the five existing PCIe driver cells from busy polling to MSI/MSI-X waits. They stop burning CPU, which matters once several drivers share a few vCPUs.

## Key Insights

- Every PCIe cell polls today (scout report). Under KVM each poll loop costs host CPU.
- Timeouts stay. Each wait is `WaitIrq` with the existing timeout logic around it, so a lost interrupt degrades to a logged timeout instead of a hang.

## Requirements

- Functional:
  - nvme: MSI-X vector 0 = admin CQ, vector 1 = I/O CQ (`IV` in Create I/O CQ, `IEN=1`); completion handler drains CQ until phase flips, then rings the CQ head doorbell.
  - ahci: one MSI vector; `PxIE` for D2H/PIO/DMA-setup/SDB/error; clear `PxIS` then `IS`.
  - igb/e1000: MSI-X (igb: rx/tx/other vectors via IVAR) or MSI (e1000 82540EM: MSI only, IMS RXT0/TXDW/LSC).
  - xhci: interrupter 0 `IMAN.IE=1`, `IMOD` 4000 (1 ms) [assumption-tuned], `USBCMD.INTE=1`; drain event ring on IRQ.
  - A driver that gets `count == 0` from 435 keeps today's polling loop and logs `irq=polled reason=…`. This is not a fallback for errors: it is the documented contract for platforms that refuse MSI (FADT flag).
- Non-functional: idle CPU of driver cells near zero; throughput not lower than polling on the existing lanes.

## Architecture

Each cell gets a small `irq.rs` helper: `setup(bdf, n) -> IrqSet`, `wait(idx, timeout)`. Rx/tx paths in NIC cells are restructured as wait → drain → re-arm.

## Assumptions

- **Claim:** `cpu_accounting` exposes per-cell CPU time readable from the shell (e.g. `cellbox` or a telemetry command). **Confidence:** medium. **How to verify:** read `kernel/src/task/telemetry.rs` and the `TelemetrySnapshot = 430` consumers.

## Related Code Files

- Create: `cells/drivers/{nvme,ahci,igb,e1000,xhci}/src/irq.rs`
- Modify: controller/dispatch files listed in Context Links; each cell's `declare_syscalls!` list (add 434/435/WaitIrq)

## Implementation Steps

1. nvme first (cleanest MSI-X), then ahci, e1000, igb, xhci, one commit each.
2. For each: setup vectors at init, convert the completion paths, keep timeouts, and update the syscall declaration.
3. Measure idle CPU per driver cell before and after with the telemetry snapshot; record it in the phase evidence.

## Todo List

- [ ] nvme
- [ ] ahci
- [ ] e1000
- [ ] igb
- [ ] xhci
- [ ] Idle-CPU evidence

## Success Criteria

- [ ] `nvme-x86`, `ahci-x86` (two-boot persistence), `nic-x86`, `igb-x86` (incl. VT-d variant), `xhci-x86`, `iommu-dmar-x86` pass under TCG and KVM.
- [ ] Logs show `irq=msix` (or `msi`) for each driver; no `irq=polled` on QEMU q35.
- [ ] Idle driver-cell CPU after boot drops versus the recorded baseline (numbers in evidence).

## Risk Assessment

- Lost-interrupt races between drain and re-arm. Mitigation: always drain after re-enable (level-like semantics), and keep the `take_pending` lost-wakeup guard in `WaitIrq`.

## Security Considerations

None beyond phase 04a. Cells only use their own BDF.

## Next Steps

Phase 05 builds virtio-pci on the same helper.

## Deviation Log

None.
