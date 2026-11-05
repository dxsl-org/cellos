---
phase: 4b
title: "IRQ-driven PCIe driver cells"
status: completed (2026-11-05; CP2 approved, record cp2-04b-waitcompletion-irq-approval.md)
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

- [x] nvme (MSI-X: vector 0 admin CQ, vector 1 I/O CQ)
- [x] ahci (MSI)
- [ ] e1000 — re-scoped out by CP1 §7 (`irq=polled reason=request-driven`, NIC CP)
- [ ] igb — re-scoped out by CP1 §7 (NIC CP)
- [x] xhci (MSI-X, bring-up waits; steady-state input stays on the IPC receive timeout)
- [ ] Idle-CPU evidence — replaced by the CP1 §9 MSI-to-wake latency criterion (see evidence)

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

- **2026-11-04 Surprise → owner decision: deferred.** Recon before any edit found two facts that break this file's premises:
  - `WaitIrq` (234) has no timeout: a `WaitIrq` task wakes only on its pending flag (`kernel/src/task/syscall.rs` WaitIrq arm, `scheduler.rs` sweep). Replacing the drivers' poll+`Deadline` loops with `WaitIrq` would drop the timeouts the Key Insights require, so one lost interrupt would hang the driver and its block route.
  - Idle driver cells already block in `sys_recv` (`ostd::app`); they poll only while a command is outstanding. NIC receive is request-driven (net service → `sys_recv_timeout`), so "idle CPU drops" has little to measure, and IRQ-driven RX also needs a push path to the net service.
  - Options given to the owner: (A) add a timeout to `WaitIrq` (CP1 plus a `scheduler.rs` lock extension); (B) add an IRQ source to `WaitCompletion` (larger CP1); (C) defer. **Owner chose C on 2026-11-04.** Phase 05 proceeds with polled virtio-pci drivers. 04b will be re-planned with A or B before any driver conversion.
- **2026-11-04 Re-plan → owner decision.** The owner chose option B. The [CP1](cp1-04b-waitcompletion-irq.md) was approved on 2026-11-04 (D-B-1…D-B-10):
  - an `IRQ` source for `WaitCompletion` that covers the calling task's 435 lines, with deadlines through `TIMER`;
  - an x86 MSI wake hint, because the sweep otherwise runs only on tick advance (`scheduler.rs:2223-2226`);
  - scope reduced to nvme, ahci and xhci;
  - NIC RX left to a separate `MAILBOX`-source CP.
  
  Option A ([draft](cp1-04b-waitirq-timeout.md)) is superseded. Implementation waits for the owner to schedule it. That package's §7 and §9 replace this file's Requirements and Success Criteria for the re-scoped phase.
- **2026-11-05 Implementation (Intel WIP lane, record `lane-04b-schedule-approval.md`).** Evidence: [reports/phase-04b-evidence.md](reports/phase-04b-evidence.md); CP2: [cp2-04b-waitcompletion-irq.md](cp2-04b-waitcompletion-irq.md).
- **2026-11-05 Deviation — file set.** `kernel/src/task/ipc_pending_selftest.rs:911` is outside CP1 §6 but its boot self-test asserted `1 << 3` invalid. Main granted a one-token edit (`1 << 3` → `1 << 4`); recorded for ratification as CP2 deviation 1.
- **2026-11-05 Deviation — `select_locked` signature kept.** `ipc_pending_selftest.rs` also calls `select_locked` and `WaiterRegistration::new` with their old arguments. Instead of the CP1 §5 `irq_ready` parameter, `select_locked` peeks the task's own IRQ snapshot (the same lock-free loads, under `SCHEDULER`), and `WaiterRegistration::with_irq_lines` was added beside the unchanged `new`. CP2 deviation 2.
- **2026-11-05 Surprise — NVMe MSI-X order.** With 435 called after `CC.EN`, every admin wait expired (no line-0x30 wake in an instrumented KVM run): QEMU binds the admin CQ vector when the controller is enabled and drops notifications for a vector bound while MSI-X was off. The cell now allocates before `CC.EN`. CP2 deviation 3.
- **2026-11-05 Closed.** Owner approved CP2 (deviations 1–4; `IRQ = 8` frozen). The `async_lifecycle` bench and the no-hint latency baseline are follow-up items in `.agents/TODO.md` (CP2-04b-3). e1000/igb and idle-CPU rows above stay re-scoped out by CP1 §7/§9.
