---
phase: 2
title: "Build SD and Static TFTP Tooling"
status: pending
priority: P1
effort: "3h"
dependencies: [1]
tier: medium
---

# Phase 2: Build SD and Static TFTP Tooling

## Overview

Create the recoverable SD-local U-Boot bootstrap and Windows static TFTP deployment tooling using the single boot command proven in Phase 1.

## Requirements

- Functional: preserve/reuse current SD rollback before writing U-Boot files.
- Functional: firmware config must load U-Boot from SD, not network: `arm_64bit=1`, `kernel=u-boot.bin`, `enable_uart=1`, `core_freq=250`.
- Functional: boot script sets static `serverip=192.168.42.1`, `ipaddr=192.168.42.2`, `netmask=255.255.255.0`, then TFTP-fetches only the Cellos image.
- Functional: TFTP root contains only the selected Cellos image and logs; firmware, U-Boot, config, and boot script stay on SD.
- Non-functional: keep WSL and `SharedAccess` running; no DHCP service; bind TFTP to `192.168.42.1`.

## Architecture

Data flow:

1. SD input: current recoverable boot partition.
2. Prepare transform: backup SD -> copy `u-boot.bin` -> write U-Boot `config.txt` -> compile `boot.cmd` into `boot.scr`.
3. Deploy transform: build Cellos -> generate selected image -> copy to TFTP root -> start static TFTP server bound to `192.168.42.1`.
4. Boot output: U-Boot reads boot script from SD, TFTP downloads only Cellos image, then jumps via proven command.

Dependency graph:

`Phase 1 selected image/command` -> `SD backup` -> `U-Boot SD files` -> `TFTP root image` -> `static boot`.

## Assumptions

- Claim: Windows can keep `SharedAccess` running while TFTP binds `192.168.42.1:69`.
  Confidence: medium
  How to verify: preflight `Get-NetUDPEndpoint`, `Get-Service SharedAccess`, and socket bind dry run.

## Related Files

- Create: `tools/rpi3-uboot-tftp/.gitignore`
- Create: `tools/rpi3-uboot-tftp/README.md`
- Create: `tools/rpi3-uboot-tftp/prepare-rpi3-uboot-sd.ps1`
- Create: `tools/rpi3-uboot-tftp/deploy-cellos-tftp.ps1`
- Create: `tools/rpi3-uboot-tftp/serve-static-tftp.ps1`
- Create: `tools/rpi3-uboot-tftp/rpi3-static-tftp.py`
- Create: `tools/rpi3-uboot-tftp/boot.cmd.template`
- Runtime ignored: `tools/rpi3-uboot-tftp/root/`
- Runtime ignored: `tools/rpi3-uboot-tftp/backups/`
- Runtime ignored: `tools/rpi3-uboot-tftp/logs/`
- Runtime ignored: `tools/rpi3-uboot-tftp/state/`

## Implementation Steps

1. Add ignored runtime folders for TFTP root, backups, logs, and NIC state.
2. Implement SD prepare script with explicit SD drive parameter, backup manifest, U-Boot artifact path, and generated `config.txt`/`boot.scr`.
3. Generate `boot.scr` from `boot.cmd.template` with `mkimage -A arm64 -T script -C none`; fail closed if `mkimage` is missing.
4. Implement deploy script that builds board-rpi3, converts/packages the selected Cellos image, writes SHA-256, and copies only that image to TFTP root.
5. Implement static read-only TFTP server or wrapper bound to `192.168.42.1`; no DHCP code.
6. Implement preflight checks: NIC alias/ifIndex, IP `192.168.42.1/24`, UDP69 free on that IP, firewall rule presence, `SharedAccess` still running.
7. Gate static-IP/firewall changes behind admin switches and record prior NIC state for rollback.

## Success Criteria

- [ ] SD backup manifest exists before SD writes.
- [ ] SD boot root contains firmware, `u-boot.bin`, `config.txt`, and `boot.scr`.
- [ ] TFTP root contains only the selected Cellos image and no firmware/U-Boot files.
- [ ] TFTP server logs RRQ for exactly the selected Cellos image.
- [ ] `SharedAccess` service remains running after preflight/start.

## Test Matrix

- Static: PowerShell parser check for scripts.
- Static: `python -m py_compile tools/rpi3-uboot-tftp/rpi3-static-tftp.py`.
- Static: `mkimage -l boot.scr`.
- Network: bind UDP69 to `192.168.42.1` with no DHCP listener.
- SD: verify backup hash and new SD file list.

## Backwards Compatibility

The old full SD-local boot files are recoverable from backup. Existing `gen_disk_rpi3.ps1` and full image flash lane remain unchanged.

## Risk Assessment

- Medium likelihood x High impact: SD U-Boot config breaks local boot. Mitigation: backup first, never delete backup, document restore before validation.
- Medium likelihood x Medium impact: TFTP root accidentally includes firmware/boot scripts. Mitigation: deploy script writes only selected image and cleans/validates root.
- Medium likelihood x Medium impact: firewall/static IP disrupts host networking. Mitigation: physical NIC only, no gateway, record state, rollback script instructions.
- Rollback: stop TFTP, restore NIC state, copy backup SD boot files back. Irreversible part: none.

## Security Considerations

TFTP is unauthenticated. Bind only to `192.168.42.1` on direct cable and serve a minimal root.

## Deviation Log

None.
