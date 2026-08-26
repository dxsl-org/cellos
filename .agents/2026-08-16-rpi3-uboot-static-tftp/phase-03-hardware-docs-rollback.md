---
phase: 3
title: "Validate Hardware and Document Rollback"
status: pending
priority: P1
effort: "2h"
dependencies: [2]
tier: medium
---

# Phase 3: Validate Hardware and Document Rollback

## Overview

Run the static TFTP lane on RPi3B v1.2, prove kernel-only iteration, and update operator docs with rollback-first instructions.

## Requirements

- Functional: hardware boot must fetch only the Cellos image from TFTP and enter Cellos via the Phase 1-proven handoff.
- Functional: second boot must change only the TFTP Cellos image, not SD files.
- Functional: docs must preserve the SD rollback and state that OTP/DHCP are out of scope.
- Non-functional: no kernel NIC driver claims; this is a bootloader-host TFTP lane only.

## Architecture

Data flow:

`build Cellos` -> `deploy selected image to TFTP root` -> `U-Boot SD boot script static TFTP` -> `Cellos UART output` -> `logs/report/docs`.

## Assumptions

- Claim: U-Boot's SMSC LAN951x/LAN9500 Ethernet path works on RPi3B v1.2 with the selected artifact.
  Confidence: medium
  How to verify: U-Boot log shows Ethernet device and successful TFTP transfer.

## Related Files

- Modify: `docs/baremetal/load-cellos.md`
- Modify: `tools/rpi3-uboot-tftp/README.md`
- Create: `.agents/debug/<timestamp>-rpi3-uboot-tftp-report.md`
- Runtime logs: `tools/rpi3-uboot-tftp/logs/`

## Implementation Steps

1. Run server and boot with SD-local U-Boot.
2. Capture U-Boot serial, TFTP log, and Cellos UART boot output.
3. Rebuild or swap a marker Cellos image in TFTP root only; reboot without touching SD.
4. Save evidence report with U-Boot artifact hash, selected command, image hash, TFTP RRQ, UART proof, and rollback state path.
5. Update `docs/baremetal/load-cellos.md` with the SD-local U-Boot static TFTP lane.
6. Update `tools/rpi3-uboot-tftp/README.md` with exact operator steps, admin boundaries, and troubleshooting.

## Success Criteria

- [ ] U-Boot log shows static IP `192.168.42.2` and server `192.168.42.1`.
- [ ] TFTP log shows only the selected Cellos image RRQ.
- [ ] Cellos UART reaches the expected boot marker for the active kernel.
- [ ] Second boot uses a different Cellos image hash without SD writes.
- [ ] Docs include rollback before any SD-modifying command.

## Test Matrix

- Hardware: first boot from SD-local U-Boot.
- Hardware: second boot after TFTP-only image swap.
- Host: `Get-Service SharedAccess` remains running.
- Host: firewall/static-IP rollback verified.
- Docs: manual read-through of setup and rollback order.

## Backwards Compatibility

Existing full SD flashing and full SD-local boot remain available. This lane is additive and reversible.

## Risk Assessment

- Medium likelihood x High impact: U-Boot Ethernet driver does not work on this exact board/artifact. Mitigation: Phase 1 manual proof before automation; rollback to SD-local boot.
- Medium likelihood x Medium impact: docs lead user to modify SD before backup. Mitigation: rollback-first wording and script fail-closed backup manifest.
- Low likelihood x Medium impact: static IP conflicts with another host network. Mitigation: no gateway, direct cable, exact NIC alias/ifIndex preflight.
- Rollback: restore SD boot backup, remove firewall rule, restore NIC config, stop TFTP. Irreversible part: none.

## Security Considerations

Document that this is isolated-cable TFTP only. Do not leave the server listening after the boot test.

## Deviation Log

None.
