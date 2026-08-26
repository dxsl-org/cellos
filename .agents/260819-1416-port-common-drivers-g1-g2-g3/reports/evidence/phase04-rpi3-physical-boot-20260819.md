# Phase 04 RPi3 physical boot evidence — 2026-08-19

Status: PASS for TFTP payload delivery, SDHCI/SD discovery, partition reads,
filesystem mounts, kernel-push input service startup, shell readiness, and
interactive UART RX.

## Environment

- Board: Raspberry Pi 3 Model B, revision `0xa22082`.
- Host link: `Ethernet`, 100 Mbps, host `192.168.42.1/24`, Pi `192.168.42.2`.
- UART: CP210x on COM4, 115200 8N1, receive-only during boot.
- Payload SHA-256: `ecc52d8764ef2ef3e4ced3a4b472fbe68c14e53198d9290a5c30953698706f34`.
- TFTP log: `/home/dmin/cellos/tools/rpi3-netboot/logs/server-20260819-212021.log`.

## Decisive evidence

- TFTP RRQ at `21:24:48`, DONE at `21:24:51`, 9,572,416 bytes.
- U-Boot checksum: `Verifying Checksum ... OK`.
- `[sd] SD card probed: 30318592 sectors (~14804 MiB), block_addr=true`.
- `[mmc] SD card probed at 0x3f300000`.
- MBR P1-P4 all reported `ok`.
- `Filesystem: FAT16 mounted successfully.`
- `[vfs] FAT32 /mnt/sd volume mounted`.
- `[input] Input Service v0.3` and `No VirtIO input device; relying on kernel push`.
- `=== Cellos shell ready` followed by `Cellos >`.
- Sending `help` over COM4 returned `Cellos Shell v0.2.1` and a fresh prompt.
- A numbered 100-command `echo UARTRX001` through `UARTRX100` burst returned
  100 unique responses with no missing IDs and returned to the prompt.
- No panic or Cell fault was observed in the 150-second UART capture.

## Safety

No SD write, OTP programming, or network configuration mutation was performed.
The adapter TX lead remained disconnected during autoboot and was connected
only after the shell prompt appeared for the interactive RX checks.
