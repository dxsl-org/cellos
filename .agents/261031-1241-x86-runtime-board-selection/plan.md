# Plan — x86-64 runtime board selection (ADR-0029)

**Status:** registered in `.agents/plan-portfolio.md` (order 1). Lock handoff for phases 01–02 approved and **effective 2026-11-01** (Ginux closed; [record](lock-handoff-approval.md)). Phase 03 needs its own handoff.
**Authority:** [ADR-0029](../../docs/decisions/0029-platform-image-families-and-runtime-detection.md) §1–3, 5; [ADR-0022](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md) amendment 2026-10-31
**C2C link (ADR-0022 admission):** necessary prerequisite / build simplification. The Intel qualification machine and QEMU lanes boot one x86 image, and the HCL profile is selected and pinned at boot instead of by a cargo feature.
**Evidence ceiling:** QEMU (q35 with and without `intel-iommu`). Physical evidence comes from the HCL machine's qualification, not from this plan.
**Recon:** [scout-report.md](scout-report.md)

## Goal

One `x86_64` kernel image. Profile is chosen at boot from a compiled catalog:
- Operator selection comes from the Limine command line.
- Detected ACPI/PCI/SMBIOS facts only validate that selection or reject it.
- First boot pins a fingerprint. Later mismatches stop for re-confirmation.
- The `board-x86-pc` feature is removed everywhere.

## Phases

| # | Phase | Status | Depends | Checkpoint |
|---|---|---|---|---|
| 01 | [Identity facts and catalog API](phase-01-identity-facts-and-catalog.md) | done 2026-11-01 (uncommitted) | — | no |
| 02 | [Runtime profile selection, feature removal](phase-02-runtime-profile-selection.md) | pending | 01 | **yes** |
| 03 | [Fingerprint pinning and re-confirmation](phase-03-fingerprint-pinning.md) | pending | 02 | **yes** |

## Key design decisions

- **Two-stage selection.** Pre-ACPI code (COM1, the four legacy COM ports, BIOS/RSDP windows) uses `hal_soc_x86::X86_EARLY_FACTS`, a single constant shared by every catalog profile and enforced by a const assertion. The full profile is bound after `acpi::parse` and before the IOAPIC/HPET/MCFG `has_driver` gates (`kernel/src/main.rs:448-470`).
- **Policy is never inferred.** `Optional` DMA (the q35 profile) is reachable only through explicit `cellos.profile=qemu-q35-x86_64` on the command line, and only if CPUID reports a hypervisor. With no option, the generic PC profile (`Required`) applies. The DMAR fallback in `iommu_x86.rs:438-457` keys off the bound profile.
- **Fingerprint is checked late, before the trust boundary that matters.** The store is not reachable until VFS and block driver Cells run, which is after ACPI, PCI and IOMMU. Init verifies the record before spawning non-boot services. Boot-critical storage/console drivers run under catalog policy, which is unchanged by the fingerprint.

## Assumptions (unverified)

- A1 (**verified 2026-11-01**, phase 01): the vendored Limine 8.7.0 used for the ISO supports the executable-file request, whose `cmdline` field carries the command line, and the SMBIOS request (physical entry points under base revision 3). See [phase 01](phase-01-identity-facts-and-catalog.md#implementation).
- A2: QEMU q35 sets the CPUID hypervisor bit under both TCG and KVM.
- A3: A writable cellosfs path exists on the x86 boot disk images used by CI. If not, phase 03 adds a small config partition to `scripts/gen-disk*`.

## Rollback

Each phase is a single revertable commit set. Phase 02 removes a cargo feature, and reverting restores it. No on-disk format is irreversible until phase 03 ships the record schema. That schema is versioned (`v1`), and unknown versions are treated as "no record".
