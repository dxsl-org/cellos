---
phase: 5
title: "ACPI DMAR discovery → real IOMMU"
status: pending
priority: P2
effort: M (kernel ACPI + IOMMU plumbing)
dependencies: [2, 4]  # sub-phases 02b and 04b (the DMA clients this gates)
tier: thinking
ceiling: qemu
---

# Phase 05 — ACPI DMAR discovery → real IOMMU

## Target

Roadmap gate **X86-PC-4**. The x86 IOMMU base is hardcoded to the q35 model
(`kernel/src/task/drivers/iommu_x86.rs:59-60`), so per-Cell DMA isolation cannot
be programmed from firmware data on a real board, and the risk
`CELLOS-X86-DMAR-002` stays open. Board VT-d is a hardware property: present on
Whiskey-Lake-class parts, absent on Haswell-ULT U-series.

## Change

- Parse ACPI **DMAR** (DRHD units, and the RMRR/ATSR/SAT entries only as far as
  they affect correctness) in the existing kernel ACPI path, next to the MADT /
  HPET / MCFG handling in `kernel/src/main.rs`.
- Replace the hardcoded base with the discovered unit(s); keep the board-declared
  q35 value as a fallback **only** while both the QEMU and hardware gates pass,
  and make the fallback log which path was taken.
- Fail closed when DMAR is absent but a DMA-capable driver requests isolation:
  the driver must not fall back to untranslated DMA silently (that is the SAS
  invariant the VT-d work exists to protect).
- Expose a status line equivalent to today's `VT-d ACTIVE` naming the discovered
  unit and the active domain count.

## QEMU-first gate

- Reuse the existing VT-d constructors (`boot_x86_bios_with_vtd`,
  `boot_x86_root_port(with_vtd)`) which already place `-device intel-iommu`
  before endpoint devices, and run `nic-x86` plus the phase-04 `igb-x86` VT-d
  variant with the new parser.
- Add a negative lane: boot **without** `intel-iommu` and assert fail-closed
  behaviour (a DMA-capable driver either does not activate or logs the named
  refusal) rather than quietly using untranslated DMA.
- Regression set that must stay green: `nvme-x86` 3/3, `nic-x86` 2/2,
  `pcie-multibus-x86` 2/2, `x86_64-boot` 7/7.
- State in the evidence that QEMU's vIOMMU tables are QEMU-generated, not
  firmware DMAR, so the parser's firmware path is only fully exercised in
  phase 07.

## Acceptance

- `iommu_x86.rs` no longer hardcodes a base: the address comes from parsed
  firmware data, with an explicit named fallback.
- Both QEMU variants pass and every listed regression lane stays green.
- Absent-DMAR behaviour is fail-closed and witnessed by the negative lane.

## Out of scope

- AMD-Vi (`IVRS`) — a different table and unit model; a separate decision.
- Interrupt remapping (IR) and ATS/PASID unless a later phase proves it is
  required; RMRR correctness beyond what the exercised devices need.

## Risk assessment

- **Undo:** restore the hardcoded base and delete the parser; QEMU lanes return
  to today's behaviour. Keep this revert trivial by isolating the new code
  behind the discovery function.
- **Not undoable:** a machine that ran with untranslated DMA — which is why the
  absent-DMAR path is fail-closed and gated by its own negative lane.
- **Containment:** this phase touches the path every DMA client depends on
  (NVMe, e1000, igb). Any red regression lane blocks the phase, not just the
  new test.
