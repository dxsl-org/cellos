# Phase 03 — Fingerprint pinning and re-confirmation  (checkpoint)

**Status:** implemented 2026-11-02 (uncommitted), awaiting CP2. Design: [CP1](phase-03-cp1.md), approved with all defaults ([record](phase-03-cp1-approval.md)). Evidence and deviations: [CP2 dossier](reports/phase-03-cp2-dossier.md). Superseded requirement details: the record lives at `/srv/.cellos-machine/pin`, not `/etc/cellos/machine-pin`; an unknown version or unreadable record is safe mode, not "no record"; without a store, the PC profile enters safe mode and q35 warns (criterion 5 reads "DMA refusal + store unavailable → safe mode").

## Context Links
[plan](plan.md) · [phase 02](phase-02-runtime-profile-selection.md) · ADR-0029 §3

## Overview
Record the first-boot fingerprint, re-check it on every boot, and stop for operator re-confirmation on mismatch.

## Key Insights
- The record carries no authority. It only detects change. Policy remains with the bound catalog profile.
- Storage is reachable only after the boot-critical driver Cells start. The check gates the spawning of non-boot services, not kernel boot.

## Requirements
- `MachineFingerprint v1`: profile slug; ACPI OEM ID / table ID / revision; SMBIOS manufacturer, product and UUID when present; sorted PCI `(vendor, device, class)` set; DMAR present and VT-d active flags; schema version.
  - Hash: SHA-256 over a canonical encoding, using the existing crypto library in the tree.
- The kernel exposes the fingerprint read-only to init through the existing boot-info path. No new syscall without the ABI checkpoint. If none fits, stop and escalate (bounded escalation).
- Init (or its supervisor):
  - Reads `/etc/cellos/machine-pin` from writable cellosfs.
  - No record: logs `first boot`, writes the record and continues.
  - Match: continues.
  - Mismatch: prints the field-level diff and enters safe mode (console and storage only, no network or user services) until the operator runs `cellbox machine pin --confirm`.
- Unknown schema version is treated as no record and logs a warning.

## Architecture
Kernel computes the fingerprint. Init compares it and gates. The `cellbox` command rewrites the record. The safe-mode service set is a static list in init.

## Related Code Files
`kernel/src/main.rs` (compute after the IOMMU activates), the boot-info hand-off to init, `cells/` init or supervisor, the `cellbox` CLI (ADR-0028), `scripts/gen-disk*` if A3 fails.

## Implementation Steps
1. Canonical encoding and hash, with host tests (order independence of the PCI set, version field).
2. Hand-off to init through the existing mechanism (ABI check first).
3. Init compare/gate and safe-mode service set.
4. The `cellbox machine pin --confirm` command.

## Todo List
- [x] fingerprint encode + tests
- [x] kernel→init hand-off (ABI gate: syscall 433, CP1)
- [x] init gate + safe mode
- [x] cellbox confirm command

## Success Criteria (checkpoint)
- Host tests pass.
- QEMU sequence on one disk image:
  1. First boot writes the record.
  2. Second boot matches.
  3. Booting with an added `-device e1000` gives a mismatch, a diff naming the PCI set, safe mode, and no network service.
  4. `cellbox machine pin --confirm` followed by a reboot matches again.
  5. Removing `intel-iommu` under the PC profile triggers both the DMA refusal and the fingerprint mismatch.

## Risk Assessment
- The ABI checkpoint may block the hand-off. The plan stops there rather than adding an unreviewed syscall.
- Firmware updates change the OEM revision, which forces re-confirmation. That is intended.
- Rollback: remove the init gate. The record file is inert data.

## Security Considerations
- The record is integrity-relevant but not authority. A tampered record can only cause a false match. Policy is unaffected, because DMA isolation is still enforced from the profile.
- Write access to `/etc/cellos/machine-pin` is limited to the confirm path's capability.

## Next Steps
Update the HCL to describe profile and pin per machine. Then `/skill:hc-adr` amends ADR-0029 consequences if the hand-off design changes.
