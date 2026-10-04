---
phase: 4
sub: b
title: "igb part B — DHCP data plane and VT-d variant"
status: pending
priority: P2
effort: M (sub-phase of the igb family)
dependencies: [4, 1]
tier: thinking
ceiling: qemu
---

# Phase 04b — igb part B: DHCP data plane and VT-d variant

## Target

Roadmap gate **X86-PC-3**, second half: prove the igb driver carries real
traffic end to end, with and without translation — the same two shapes the e1000
DHCP gate proved (`.agents/260903-x86-e1000-dhcp/plan.md`, status completed).

## Change

- Wire the igb registration into the net service path so DHCP frames flow
  through the driver cell (no driver-side DHCP logic — the net service owns it).
- Fix only defects reproduced by the strict oracle; the oracle is the deliverable,
  not a source rewrite.
- Keep `SLIRP restrict=on` and the existing bounded runner startup.

## QEMU-first gate

- Extend `tests/integration/tests/igb-x86.rs` with two bounded variants:
  - ordinary: driver registration → first bridge Tx `accepted=true` → first Rx →
    DHCP address acquired;
  - VT-d: `-device intel-iommu` placed **before** the NIC (existing
    `boot_x86_bios_with_vtd` shape) → require isolation active before any DMA →
    then the same DHCP sequence.
- Regression: `nic-x86` 2/2, `nvme-x86` 3/3, `x86_64-boot` 7/7.
- State that i210/i211 PHY/NVM behaviour is not validated by the model (`A-01`).

## Acceptance

- Both variants pass on QEMU with the exact success markers, or the failing step
  is reported with the observed marker (no partial credit).
- No change to the DHCP state machine or net IPC unless the oracle proves it
  broken; if changed, the change is listed in the phase evidence.

## Out of scope

- Physical LAN (phase 07), ACPI DMAR discovery (phase 05 — this sub-phase only
  requires the *existing* VT-d path to keep working), e1000e/igc/Realtek.

## Risk assessment

- **Undo:** remove the two variants; 04a's driver registration stays valid and
  e1000 remains the shipped path.
- **Not undoable:** claiming DHCP on real hardware — phase 07.
