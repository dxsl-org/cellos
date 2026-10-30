---
phase: 8
title: "Device-specific quiescence without SAS availability regression"
status: pending
priority: P1
dependencies: [1]
tier: thinking
---

# Phase 08: Device-specific quiescence

## Overview

Item 2 is valuable for SAS: Rust ownership does not stop a device writing freed DMA memory. But PCIe FLR is only one reset mechanism, not the definition of safe restart. Keep admitted Tier-1 driver Cells and their direct authorized MMIO/DMA path; do not move drivers to private domains or proxy every I/O through the kernel.

## Requirements and architecture

- State machine at the actual device publication boundary: prepared/not-issued → issued/notified → completion acknowledged, or ambiguous completion requiring reconciliation/quiescence. Use AHCI PxCI or an actually supported NVMe doorbell; existing virtio-MMIO Cells are not assumed to be q35 PCI drivers.
- A timeout after hardware issue does not undo effects. Do not retry a write/command blindly, regardless of descriptor ownership. A pre-issue retry must preserve reservation/buffer lifetime and prove no notification occurred.
- Carry issue certainty end-to-end through the block-driver reply, VFS router and block stream, not only kernel markers. AHCI currently returns generic IO after PxCI timeout (`controller.rs:482-501`), dispatch collapses errors to one byte (`dispatch.rs:10-13,100-107`), and VFS retries every failed write (`block_stream.rs:139-149`). Define/migrate NotIssued, Indeterminate and Completed outcomes; automatic write retry requires proven NotIssued or explicitly authorized request-bound reconciliation. Approve any affected wire/error semantic delta through Law-1 before implementation.
- Admission cut closes software submission, then quiescence fences all relevant users: running/switching CPU contexts, delayed workers, borrowed SAS slices, grant/ring users, IRQ callbacks, TLB mappings and DMA. No Rust Drop assumption on fatal faults.
- Stop/mask every enabled interrupt delivery path actually supported (INTx/MSI/MSI-X). Synchronize in-flight handlers and generation-bound routes before reuse; an AP that cannot acknowledge means quarantine, not a fabricated completed drain.
- A bounded validated kernel capability walk supplies current PCI controls even for Platform Cell metadata stubs. Do not trust driver-provided cached offsets or expand registration ABI merely for this metadata.
- Select a reviewed reset/quiesce strategy per controller and ownership/reset domain: FLR where supported; documented AHCI port/controller stop/reset or NVMe disable/reset where sufficient. Verify DMA/queue/interrupt inactivity with the hardware-specific contract. Reset affecting sibling ports/functions requires authority over all affected owners, never blanket bus reset.
- Disable bus mastering where part of that strategy, revoke translation, wait for required IOMMU/cache/TLB completion and acknowledge pins only after actual quiescence. Exact ordering and posted-write drain requirements are device-specific; do not mistake a config bit readback, 100 ms delay or vendor-ID readability for proof of reset completion.
- Device re-grant and DMA-frame reclamation have separate proof obligations. If translation fencing proves DMA isolation but reset state is unknown, frames may be reclaimed only with that independent proof; the device stays quarantined. If either proof is absent, retain the affected resources and charge them.
- Reuse the existing retirement funnel and source-owned per-device state, not a second lifecycle manager. Deferred/retry work is bounded and never waits under scheduler locks.

## Related Files

`kernel/src/task.rs` retirement funnel, `kernel/src/task/drivers/{pcie_ecam,iommu_x86,iommu}.rs`, IRQ route ownership, `kernel/src/memory/pin.rs`, resource registry and the selected AHCI/NVMe Cell. Include the real block-driver reply producers, `cells/services/vfs/src/{blk_router,block_stream}.rs` and every affected consumer found by reference census. Inventory the shared driver wire contract before selecting an exact delta; no legacy error fallback may collapse Indeterminate into retryable refusal. Public/wire/error semantics require separate approval, even with unchanged opcode or byte count.

## Implementation Steps

1. Establish a source-pinned AHCI q35 fixture and actual IRQ/flush/DMA support. Write a per-controller table of issue point, drain acknowledgement, reset scope, completion evidence and unavailable outcome.
   Include a driver-to-VFS ambiguous-write witness: issue then suppress completion; observe Indeterminate at the caller and prove no second hardware submission. A proven NotIssued control may retry; a completed control retains success.
2. Implement ordered retirement by that contract, preserving existing pin/quarantine semantics. Record each verified stage and outstanding obligations.
3. Inject timeout before issue, after issue and during quiescence; retain a second unaffected Cell and, when applicable, a sibling device owner to detect overbroad reset.
4. Restart the selected controller through its supported strategy with a fresh incarnation/lease; read back a known committed record. Also inject failed reset/IOTLB/CPU acknowledgement and prove non-reuse.
5. Reuse an adequate isolated lifecycle runner; the Phase 07 private image is optional fixture reuse, not a dependency on its authority work. Any missing fixture belongs to this admitted slice. Entry hardening (04) is required only for a subcase using its new NMI/MCE hooks, not for ordinary device-timeout testing.

## Success Criteria

- [ ] Real AHCI prepared vs issued timeout paths differ as specified; uncertain writes are never implicitly replayed.
- [ ] Supported controller quiesce/reset yields a fresh driver/lease and successful readback, without requiring FLR where an equivalent documented reset works.
- [ ] Failed reset or CPU/IRQ/IOMMU acknowledgement retains the unsafe device/resources; no premature pin release, route reuse or new BDF grant.
- [ ] Independent Cell/device owner survives replacement; direct admitted SAS MMIO/DMA path remains intact.
- [ ] Exact wire/error semantics are approved before cutover and migrated across all affected callers; no production injection bypass.

## Assumptions

AHCI/NVMe reset/IRQ/FLR capabilities are established by actual fixture and hardware contract before implementation. QEMU success qualifies only the exact software/device model, never physical hardware.

## Security and risk assessment

LBI cannot substitute for DMA/IRQ/TLB fencing. No-FLR no longer means automatic permanent lockout; missing safe reset/isolation evidence still fails closed. Revert source plus cold boot a known-good image; device effects already issued cannot be undone by binary revert.

## Deviation Log

SAS review: replaced blanket FLR-only quarantine with controller-specific proof; removed arbitrary 04 dependency and preserved driver hot path. No completion claimed for absent hardware evidence.
