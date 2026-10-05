---
phase: 5
title: "ACPI DMAR discovery → real IOMMU"
status: completed
priority: P2
effort: M (kernel ACPI + IOMMU plumbing)
dependencies: [2, 4]  # sub-phases 02b and 04b (the DMA clients this gates)
tier: thinking
ceiling: qemu
---

# Phase 05 — ACPI DMAR discovery → real IOMMU

## Evidence (2026-10-05, `qemu` ceiling)

- **The base now comes from firmware.** `kernel/src/acpi.rs` parses the DMAR
  table next to MADT/MCFG/HPET and exposes `dmar_base`, `dmar_units` and
  `dmar_include_pci_all` on `AcpiInfo` (zero = nothing validated). Selection
  prefers the first DRHD carrying `INCLUDE_PCI_ALL` (it covers devices that
  declare no scope) and otherwise takes the first segment-0 DRHD, recording that
  the unit is scope-limited. RMRR/ATSR/SAT records are skipped by their own
  length; a malformed record stops the walk instead of reading past the table.
- **`iommu_x86::init_hw` no longer hardcodes the page it programs.** It takes the
  discovered base, and every register access goes through `VTD_REG_BASE`
  (`reg_base()`), so the discovered unit is authoritative in probing, activation
  and both invalidation paths. The q35 constant survives only as the
  board-declared fallback for `SocId::QemuX86Q35`, and the chosen path is logged:
  `[vtd] register base 0x… from ACPI DMAR (units=… include_pci_all=…)` versus
  `[vtd] no ACPI DMAR unit; using the board-declared q35 register base 0x… (fallback)`
  versus `[vtd] no DMAR-discovered register base; refusing q35 fallback`. The
  ACTIVE line names the programmed unit, and each per-Cell mapping line reports
  the live domain count (`domains=N`), so "per-Cell domains" is checkable rather
  than asserted.
- **The PC profile can use VT-d for the first time** (RUN 3): before this phase
  `x86_64-pc` refused the q35 fallback, so its only possible source of a base was
  firmware data that nothing parsed. RUN 3 shows the PC image discovering
  `0xfed90000` from DMAR and activating per-Cell domains.
- **Absent DMAR is fail-closed by profile, and never silent.** `BoardDescriptor`
  gained `dma_isolation: DmaIsolation` (`Required` / `Optional`) — a machine
  contract, not a hardware probe. `x86_64-pc` is `Required`: with no remapper,
  `map_dma_for_cell` refuses with a named error per requester (RUN 4: four
  refusals, zero `[driver_cell] … registered` lines, and the machine still reaches
  its shell) instead of quietly running untranslated DMA. The QEMU model and the
  ARM/RISC-V boards are `Optional`: they log the fallback base and the identity
  contract once (`[iommu] board … declares no DMA remapper; DMA is untranslated`),
  which is what removes the silence without breaking the lanes that boot without
  an IOMMU. The decision itself is a pure function (`dma_without_remapper`) with
  its own unit test, so the policy cannot drift from the log.
- Gates: new `iommu-dmar-x86` **3/3** (DMAR discovery with the enforcing unit
  equal to the discovered base; PC fail-closed with the shell alive and no driver
  registered; q35 named fallback + identity). Regressions: `igb-x86` 2/2,
  `nic-x86` 2/2, `nvme-x86` 3/3, `x86_64-boot` 9/9, `pcie-multibus-x86` 2/2,
  `ahci-x86` 5/5, `xhci-x86` 4/4, `driver-registration-contract` 3/3,
  `cellos-kernel` 193/193 (3 DMAR parser cases + the decision table),
  `cellos-boards` 13/13, HAL boundaries, F1/F5; `board-rpi3` and `riscv64gc`
  build with zero errors. The PC-profile lanes are wired into CI with their own
  image build.
- Evidence log: `evidence/phase-05-dmar-iommu.log`.

## Not claimed

- QEMU's vIOMMU tables are QEMU-generated, so the parser runs against
  firmware-shaped data, not a real BIOS — the physical firmware path is phase 07.
- AMD-Vi (`IVRS`) is a different table and unit model; interrupt remapping and
  ATS/PASID are untouched. RMRR is skipped (no exercised device needs it), which
  stays a recorded limitation rather than a silent assumption.
- No machine is qualified: `docs/hardware-compatibility-list.md` still has no
  machine rows, and the PC profile's `Required` contract is a policy this project
  chose, not a measurement of any particular board.


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
