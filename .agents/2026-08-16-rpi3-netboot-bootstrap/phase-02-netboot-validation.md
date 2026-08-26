---
phase: 2
title: "Validate Direct Ethernet Boot Lane"
status: pending
priority: P1
effort: "2h"
dependencies: [1]
tier: medium
---

# Phase 2: Validate Direct Ethernet Boot Lane

## Overview

Exercise the direct PC-to-RPi3 boot path and prove that kernel iteration no longer requires rewriting the SD image every time.

## Requirements

- Functional: boot RPi3 with SD containing only `bootcode.bin` and LAN connected to PC NIC.
- Functional: capture DHCP/TFTP server logs and UART logs for each attempt.
- Functional: prove fast iteration by updating only `kernel8.img` in TFTP root and rebooting.
- Non-functional: do not enable OTP; do not require internet on Ethernet; Wi-Fi remains host internet.

## Architecture

Data flow:

`kernel build` -> `kernel8.img in TFTP root` -> `RPi3 boot ROM/bootcode DHCP` -> `TFTP firmware/config/kernel fetch` -> `UART Cellos boot evidence` -> `logs/report`.

## Assumptions

- Claim: bootcode-only SD triggers network boot without OTP on this Pi3B v1.2 workflow.
  Confidence: medium
  How to verify: hardware log shows DHCP/TFTP requests after power-on using bootcode-only SD.

## Related Files

- Create: `tools/rpi3-netboot/logs/<timestamp>-server.log`
- Create: `tools/rpi3-netboot/logs/<timestamp>-uart.raw`
- Create: `.agents/debug/<timestamp>-rpi3-netboot-report.md`

## Implementation Steps

1. Prepare SD local-boot backup and convert SD root to bootcode-only only after backup hash verification.
2. Configure `Ethernet` ifIndex 14 to the chosen static subnet, for example `192.168.42.1/24`, with no gateway/DNS.
3. Start DHCP/TFTP server and verify UDP 67/69 binds on the selected IP.
4. Power RPi3 and capture server logs until `kernel8.img` RRQ completes or times out.
5. Capture UART for at least 30 seconds after kernel transfer.
6. Replace only `tools/rpi3-netboot/root/kernel8.img`, reboot RPi3, and confirm the new kernel hash is logged.
7. Save evidence report with NIC config, firewall rule names, log paths, and hashes.

## Success Criteria

- [ ] DHCP log shows DISCOVER/OFFER/REQUEST/ACK for the RPi3 MAC on the direct Ethernet subnet.
- [ ] TFTP log shows RRQ success for `bootcode.bin`, `start.elf`, `fixup.dat`, `config.txt`, and `kernel8.img`.
- [ ] UART shows the current Cellos board-rpi3 boot marker expected for the active kernel.
- [ ] A second boot uses a changed `kernel8.img` hash without rewriting the SD card.

## Test Matrix

- Network preflight: `Get-NetAdapter`, `Get-NetIPAddress`, `Get-NetUDPEndpoint`, and firewall rule inspection.
- Server dry run: bind UDP 67/69 and exit before hardware.
- Hardware e2e: bootcode-only SD plus direct LAN boot.
- Iteration proof: replace only TFTP `kernel8.img` and reboot.

## Backwards Compatibility

The original SD boot backup remains recoverable. If netboot fails, restore full local boot files to SD and return to current flash workflow.

## Risk Assessment

- High likelihood x Medium impact: RPi3 boot firmware retries silently and gives little UART before `start.elf`. Mitigation: DHCP/TFTP logs are primary evidence before ARM kernel starts.
- Medium likelihood x Medium impact: APIPA/disconnected state changes when cable is inserted. Mitigation: re-check ifIndex and IP immediately before binding.
- Low likelihood x High impact: serving DHCP on the wrong network. Mitigation: refuse to start unless selected NIC matches alias/ifIndex and Wi-Fi is not the bind target.
- Rollback: stop server, restore NIC config, restore SD boot backup. Irreversible part: none.

## Security Considerations

DHCP lease range must be a tiny isolated pool for the direct cable only. TFTP is unauthenticated, so keep root minimal and bind-scoped.

## Deviation Log

None.
