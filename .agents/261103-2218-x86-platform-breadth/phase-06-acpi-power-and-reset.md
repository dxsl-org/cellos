---
phase: 6
title: "ACPI reset, S5 poweroff, power button"
status: pending
priority: P2
effort: "3d"
dependencies: [1, 3]
tier: medium
checkpoint: false
---

# Phase 06: ACPI reset, poweroff, power button

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- Phase 01 `FadtInfo`; ACPI 6.5 §4.8 (PM1 registers), §7.4.2 (`\_S5`), §4.8.3.6 (reset register)
- `cells/services/power/src/lib.rs` (35-line trait stub); `cells/services/supervisor`, `cells/services/vfs`
- Phase 00 CP1 item `SystemPower = 436`

## Overview

Cellos can reboot and power off cleanly, and reacts to the ACPI power button. Needed for `virsh shutdown`/`reboot`, VMware "Shut Down Guest" via ACPI, and BMC soft-off on servers.

## Key Insights

- There is no reset or poweroff path today (scout report).
- S5 needs `SLP_TYPa/b` from the `\_S5_` package in DSDT AML. A full AML interpreter is a large kernel surface. A bounded bytecode scan for `NameOp "_S5_" PackageOp` covers firmware that defines `\_S5` statically (QEMU, VMware, most Intel boards). A computed `\_S5` is refused with a named log rather than guessed.
- Power button: PM1 `PWRBTN_STS`, delivered by SCI (level, active-low, GSI from FADT). It needs ACPI mode enabled (write `ACPI_ENABLE` to `SMI_CMD` when `SCI_EN` is clear).

## Requirements

- Functional:
  - Reset, in order: FADT reset register (IO/MMIO/PCI config space) when `RESET_REG_SUP`; then port `0xCF9` (0x02 then 0x06); then 8042 pulse (`0xFE` to 0x64); then triple fault. Each step logged before it is attempted.
  - Poweroff:
    1. Parse DSDT (and SSDTs) for a static `\_S5_` package.
    2. Write `SLP_TYPa | SLP_EN` to PM1a_CNT (and PM1b if present).
    3. If HW_REDUCED, use `SLEEP_CONTROL_REG`.
    4. If `\_S5_` is absent or not static, return `NotSupported`.
  - SCI: enable ACPI mode, route the SCI GSI via `route_gsi`, enable `PWRBTN_EN`, clear status on interrupt. Power-button events go to the supervisor through the existing service-notification path.
  - `SystemPower = 436` (`a0 = 0 reboot | 1 poweroff`), allowed only for the init/supervisor capability. The supervisor runs an orderly shutdown: stop cells, flush VFS, then call 436.
  - Shell `reboot` and `poweroff` commands call the supervisor, not the syscall directly.
  - Shutdown is bounded: if the orderly path has not finished within 30 s (power button) or the operator-supplied timeout, the supervisor logs which cells did not stop and calls 436 anyway. A second power-button press during shutdown forces 436 immediately.
- Non-functional: shutdown of an idle system completes within 5 s on QEMU.

## Architecture

The kernel holds `kernel/src/acpi/power.rs` (register writes and the `\_S5_` scan) and `kernel/src/acpi/sci.rs` (ISR → event flag). The supervisor owns policy. `cells/services/power` is either filled in as the policy owner or removed, whichever its existing consumers require — decided in step 1 and logged.

## Assumptions

- **Claim:** the supervisor has a shutdown/stop-all path that flushes VFS. **Confidence:** low. **How to verify:** read `cells/services/supervisor/src` for stop/terminate-all handling and the VFS flush API.
- **Claim:** `QemuRunner` exposes QMP for `system_powerdown`. **Confidence:** high (qmp_port in `tests/integration/src/lib.rs:893-895`). **How to verify:** read the QMP helper.

## Related Code Files

- Create: `kernel/src/acpi/{power,sci}.rs`, `tests/integration/tests/power-x86.rs`
- Modify: `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs`, supervisor cell, the shell command table (`cells/tools` or `cells/apps` shell — locate in step 1), `cells/services/power/src/lib.rs`

## Implementation Steps

1. Locate the shell command table and supervisor shutdown path; decide the power-cell question; log it.
2. Reset chain + syscall 436 (reboot only); lane: `reboot` → second boot banner observed.
3. `\_S5_` scanner with host unit tests on captured DSDT blobs (QEMU q35 DSDT dumped via `-acpitable`/`acpidump` from a Linux guest; store under `tests/fixtures/acpi/`).
4. Poweroff via 436; lane: `poweroff` → QEMU process exits with status 0.
5. SCI + power button; lane: QMP `system_powerdown` → orderly shutdown → QEMU exits.

## Todo List

- [ ] Shutdown path located
- [ ] Reset + 436
- [ ] `\_S5_` scanner + fixtures
- [ ] Poweroff
- [ ] SCI power button

## Success Criteria

- [ ] `power-x86` lanes (TCG and KVM): `reboot` reboots; `poweroff` exits QEMU; QMP `system_powerdown` exits QEMU after the `[supervisor] shutdown: vfs flushed` marker.
- [ ] Unit tests: `\_S5_` extraction from the q35 fixture and a fixture with `\_S5` inside a method (must return NotSupported).
- [ ] An unauthorized cell calling 436 gets `PermissionDenied` (unit test on dispatch).

## Risk Assessment

- Firmware that needs `\_PTS` before S5. Not evaluated; physical qualification (phase 20) records whether poweroff works per machine.

## Security Considerations

436 is a denial-of-service primitive. Capability-gated to init/supervisor only, with a test.

## Next Steps

Phase 09 uses ACPI shutdown in the VMware lane; phase 20 records the per-machine result.

## Deviation Log

None.
