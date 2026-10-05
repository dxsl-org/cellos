---
phase: 4
sub: b
title: "igb part B — DHCP data plane and VT-d variant"
status: completed
priority: P2
effort: M (sub-phase of the igb family)
dependencies: [4, 1]
tier: thinking
ceiling: qemu
---

# Phase 04b — igb part B: DHCP data plane and VT-d variant

## Evidence (2026-10-05, `qemu` ceiling)

- `tests/integration/tests/igb-x86.rs` now carries the two bounded variants the
  gate asked for, both untraced, both against the production q35 image:
  * **ordinary** `igb_x86_dhcp` — bind `8086:10c9` → `[driver_cell] NIC driver
    registered` → `[net-bridge] first e1000 TX len=304 accepted=true` → first Rx →
    `[net] DHCP acquired` → `[net] IP address: 10.0.2.15`;
  * **VT-d** `igb_x86_vtd_dhcp` — `-device intel-iommu` before the igb controller,
    so `[vtd] Intel VT-d: DMA isolation ACTIVE` must precede the igb bind line,
    and the same DHCP sequence must complete through the IOMMU.
  The shared order assertion also keeps 04a's identity checks: `link_up=true`, a
  non-zero MAC, no `[e1000]` line at all (the sibling cell stays idle), no denied
  syscall, no panic/fault. Result: **`igb-x86` 2/2**.
- **No product defect was reproduced**, so per the phase's acceptance no DHCP
  state machine or net IPC code changed: the net service already owns DHCP and the
  igb cell only carries frames. The one defect found was in the *test*: the DHCP
  completion marker was written with the full line's em dash, which the
  byte-at-a-time serial reader never reassembles — the ASCII prefix
  `[net] DHCP acquired` is used instead (the same shape `nic-x86` uses).
- Regressions all green on the same image: `nic-x86` 2/2, `nvme-x86` 3/3,
  `x86_64-boot` 9/9 (the phase text says 7/7; the lane has grown since it was
  written), `pcie-multibus-x86` 2/2, plus `ahci-x86` 5/5, `xhci-x86` 4/4 and
  `driver-registration-contract` 3/3.
- Runner gate `scripts/qemu-x86_64-test.sh` extended for 04b: with
  `X86_NIC_MODEL=igb` it now attaches SLIRP (`restrict=on`) as well as the device,
  so the standalone gate can assert the DHCP address actually arrives — a bare
  `-device igb` has no link to observe. Verified: `X86_NIC_MODEL=igb` PASS (igb
  bound, `[net] IP address: 10.0.2.15`, no `[e1000]` line), `X86_NIC_MODEL=e1000e`
  still PASS with its fail-closed refusal.
- `tests/integration/src/lib.rs`: the VT-d boot helper is now one private body
  (`boot_x86_bios_with_vtd_nic`) with `boot_x86_bios_with_vtd` (e1000) and the new
  `boot_x86_bios_with_vtd_igb_nic` (igb) as thin wrappers, so the
  `intel-iommu`-precedes-endpoints invariant lives in one place.
- **Scope note (`A-01`)**: the VT-d variant is a **q35** gate
  (`VICELL_IGB_VTD_ISO`, defaulting to the production image). The `x86_64-pc`
  descriptor refuses the q35 register-base fallback by design, so a `GenericX86Pc`
  lane image cannot serve it until phase 05 discovers the base from ACPI DMAR —
  that is a phase-05 property, not a phase-04b defect. QEMU's vIOMMU tables are
  QEMU-generated, so the firmware DMAR path is only exercised in phase 07.
- Evidence log: `evidence/phase-04b-igb-dhcp.log`.


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
