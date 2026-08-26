---
phase: 1
title: "Build Recoverable Netboot Tooling"
status: pending
priority: P1
effort: "4h"
dependencies: []
tier: medium
---

# Phase 1: Build Recoverable Netboot Tooling

## Overview

Create Windows-native tooling that prepares a TFTP root, preserves a full local-boot backup from the current SD card, and runs a minimal DHCP/TFTP server bound to the direct Ethernet NIC.

## Requirements

- Functional: create a deploy script that copies current SD boot files before changing anything, writes a recoverable backup, then stages TFTP root files.
- Functional: TFTP root must contain `bootcode.bin`, `start.elf`, `fixup.dat`, `config.txt`, and `kernel8.img`.
- Functional: create a Windows Python 3.14-compatible minimal DHCP/TFTP server with request/response logs.
- Functional: provide a PowerShell wrapper that verifies NIC alias/ifIndex, port availability, firewall/admin status, and starts the server.
- Non-functional: no WSL server path; WSL may still build kernel/image, but DHCP/TFTP serving stays Windows-native.
- Non-functional: no OTP changes, no kernel NIC driver, no permanent system services.

## Architecture

Data flow:

1. Inputs: current SD drive letter, `tools/rpi3-firmware/*`, release `vicell-kernel`, current `tools/rpi3-firmware/config.txt`, NIC alias/ifIndex.
2. Prepare transform: copy SD boot files to timestamped backup, build/convert `vicell-kernel` to `kernel8.img`, stage all boot files into `tools/rpi3-netboot/root/`.
3. Server transform: PowerShell wrapper sets/validates NIC static IP, opens scoped firewall rules if explicitly run as admin, then launches Python DHCP/TFTP bound to that NIC IP.
4. Outputs: staged TFTP root, `server-*.log`, `dhcp-*.log`, `tftp-*.log`, and backup manifest with SHA-256.

Dependency graph:

`firmware/kernel present` -> `backup SD boot files` -> `stage TFTP root` -> `bind static NIC` -> `start DHCP/TFTP` -> `hardware boot`.

## Assumptions

- Claim: Windows Python 3.14 can bind UDP 67/69 with admin privileges and receive direct-link broadcasts on the chosen NIC.
  Confidence: medium
  How to verify: run a dry-start that binds sockets and emits the selected local address before connecting RPi3.
- Claim: Pi3B v1.2 requests the standard boot filenames from TFTP after `bootcode.bin`.
  Confidence: high
  How to verify: server log must show RRQ sequence for firmware/config/kernel files.

## Related Files

- Create: `tools/rpi3-netboot/README.md`
- Create: `tools/rpi3-netboot/prepare-rpi3-netboot.ps1`
- Create: `tools/rpi3-netboot/serve-rpi3-netboot.ps1`
- Create: `tools/rpi3-netboot/rpi3-dhcp-tftp.py`
- Create: `tools/rpi3-netboot/.gitignore`
- Runtime ignored: `tools/rpi3-netboot/root/`
- Runtime ignored: `tools/rpi3-netboot/backups/`
- Runtime ignored: `tools/rpi3-netboot/logs/`
- Runtime ignored: `tools/rpi3-netboot/state/`

## Implementation Steps

1. Add `.gitignore` for generated root, backups, logs, state, and copied binary firmware/kernel artifacts.
2. Implement `prepare-rpi3-netboot.ps1` with explicit parameters for SD drive letter, output root, backup directory, and optional existing `kernel8.img`.
3. Make preparation fail closed if the SD drive is missing, lacks current boot files, or backup manifest cannot be written.
4. Stage `bootcode.bin`, `start.elf`, `fixup.dat`, `config.txt`, and `kernel8.img`; record SHA-256 and source path for each.
5. Implement `rpi3-dhcp-tftp.py` as a bounded single-board server: DHCP offer/ack on the selected NIC subnet plus TFTP RRQ read-only serving from root.
6. Implement `serve-rpi3-netboot.ps1` to re-check NIC alias `Ethernet`/ifIndex 14, static IP, UDP 67/69, Python availability, and Windows admin state.
7. Gate firewall/static-IP changes behind explicit PowerShell switches; print exact changes before applying.

## Success Criteria

- [ ] `tools/rpi3-netboot/root/` contains exactly the five required boot artifacts before first server start.
- [ ] Backup manifest includes original SD boot files and SHA-256 hashes.
- [ ] Server logs every DHCP DISCOVER/OFFER/REQUEST/ACK and every TFTP RRQ/ACK/error.
- [ ] Server binds only to the configured physical NIC IP, not Wi-Fi, WSL, or wildcard unless explicitly requested.

## Test Matrix

- Static: `powershell -NoProfile -File <script> -WhatIf` style dry run for prepare/serve wrappers.
- Static: `python -m py_compile tools/rpi3-netboot/rpi3-dhcp-tftp.py`.
- Unit/manual: run server with UDP 69 free check and simulated TFTP RRQ from localhost or selected NIC IP.
- Integration: connect RPi3 direct LAN and verify DHCP/TFTP logs.

## Backwards Compatibility

Existing `gen_disk_rpi3.ps1`, `run-rpi3.ps1`, and full SD-image workflow remain unchanged. The current SD boot partition is backed up before converting the card to `bootcode.bin`-only bootstrap.

## Risk Assessment

- Medium likelihood x High impact: wrong NIC/static-IP change disrupts host networking. Mitigation: bind to alias/ifIndex, require admin switch, preserve previous NIC IP config in `state/`, rollback script path.
- Medium likelihood x High impact: SD local-boot files are lost. Mitigation: copy and hash backup before modifying SD; do not delete until user confirms hardware netboot works.
- Medium likelihood x Medium impact: Windows firewall blocks DHCP/TFTP. Mitigation: explicit scoped firewall rules for UDP 67/69 on `Ethernet` only, plus log preflight.
- Rollback: stop server, remove firewall rules created by name, restore previous NIC DHCP/static config from state, copy backup files back to SD boot partition. Irreversible part: none if backup manifest exists before SD change.

## Security Considerations

DHCP/TFTP server must be bound to the isolated physical NIC and serve read-only files from the configured root. Do not expose TFTP on Wi-Fi or wildcard interfaces.

## Deviation Log

None.
