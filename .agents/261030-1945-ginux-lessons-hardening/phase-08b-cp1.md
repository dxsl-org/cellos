# Phase 08B — CP1 (driver-death quiescence in the retirement funnel)

Status: PROPOSED, awaiting user decision. No source edited.

No wire/ABI change. Semantic change: retirement now writes PCI config space and
releases IRQ waiter slots, so it needs approval under Law-1.

## Current state (source-pinned)

- Retirement funnel: `kernel/src/task.rs:1424-1523` → `reap_retired_task_resources`
  (`:1872`) → `teardown_dma_authority` (`:1828`):
  1. `revoke_dma_for_cell`, which waits for IOMMU context/IOTLB invalidation;
  2. only then `release_bdfs_for` and `release_acked_frames`.
  On failure everything is retained and retried (`:1838-1870`). This part is
  sound and is kept.
- **Gap 1: bus master is never cleared.** `pcie_ecam.rs:691` has only
  `enable_bus_master`. A dead driver's device keeps `CMD_BUS_MASTER` set until
  the next driver resets it. The IOMMU fences translation, but on a machine
  without an IOMMU (a QEMU q35 lane without VT-d) nothing stops the device from
  writing memory that has already been released.
- **Gap 2: INTx is never disabled.** MSI/MSI-X is not a reachable path today:
  - no PCIe driver cell (ahci, nvme, e1000, igb, xhci) enables MSI or MSI-X;
  - Message Control lives in config space, which cells cannot write (only the
    kernel maps ECAM);
  - only virtio-net and dwc2 call `WaitIrq`.
  So the delivery path that actually exists is INTx, plus the IRQ waiter table.
- **Gap 3: IRQ waiter slots outlive their owner.** `irq_wait.rs` keys a slot by
  bare tid and clears it only when the scheduler wakes the waiter
  (`scheduler.rs:2141`).
  - A driver that dies inside `WaitIrq` leaves the slot occupied, so its
    replacement gets `TryAgain` forever (`syscall.rs:7932`).
  - The ISR keeps acking the dead owner's MMIO base.
  - A stale `IRQ_PENDING` hands the replacement a spurious first wake.
- MMIO grant release (`resource_registry::release_for`) is not gated on DMA
  teardown. It does not need to be: a replacement cannot drive the device
  until it can claim the BDF, and `release_bdfs_for` already runs only after
  the IOMMU acknowledges.

## Proposal (08B)

Done in `teardown_dma_authority`, in this order, for each BDF the dying task owns:

1. **Quiesce the function.**
   - Clear `CMD_BUS_MASTER`, set `CMD_INTX_DISABLE` (bit 10); `CMD_MEM_SPACE`
     is left alone.
   - Read back to confirm.
   - Use a fresh, bounded kernel capability walk (the existing 64-entry walker)
     to read MSI (0x05) and MSI-X (0x11) Message Control. If either is enabled,
     clear the enable bit and set the MSI-X function mask, then read back.
   - Driver-provided offsets are never trusted.
2. **Revoke DMA** through the IOMMU, unchanged.
3. Release the BDFs and acknowledged frames, unchanged.

Failure handling: if quiescing fails (BDF outside the ECAM range, or a
readback mismatch), the BDF and frames stay retained, the cleanup is queued for
retry exactly like an IOMMU timeout, and the device is logged as quarantined.

IRQ waiters: add `irq_wait::release_waiters_for(tid)`, called from
`reap_retired_task_resources`. For every line whose waiter is `tid` it clears
the MMIO base first, then the waiter, then the pending flag, so the ISR stops
acking the dead owner's device before the slot can be reused.
`signal_irq` stays lock-free. If an ISR read the old base just before the
clear, one extra ack reaches the same, still-present virtio device; this is
documented as benign.

Out of scope:
- device reset and a fresh lease on restart (08C);
- replacement readback (08C);
- any new interrupt routing.

## Witness

Kernel host tests:
- `irq_wait` release semantics: the slot is reusable after release, and
  another tid's slot is untouched.
- Config-word computation for quiescence: bus master cleared, INTx disable set,
  MSI/MSI-X enable cleared and function mask set.

QEMU x86 (q35 with the AHCI fault ISO lane): `kill` the AHCI driver from the
shell. The kernel logs
`[pcie] quiesced 00:1f.2 cmd=… bme=0 intx_disable=1 msi=off msix=off`
before `BDF released`. A second driver instance can then claim the BDF.

Residual: QEMU/TCG only. Clearing BME does not prove that writes already
posted have drained. On a machine with an IOMMU, the IOMMU fence covers that;
without one, the reclaimed frames are still exposed until the device has
actually stopped, and only 08C's reset can close that window.
