# 2026-08-17 — RPi3 static netboot and UART input

## What happened

Cellos booted repeatedly on a real Raspberry Pi 3 Model B v1.2 through a fixed
SD firmware/U-Boot bootstrap and static TFTP. Two board-only input failures were
diagnosed and verified with exact before/after hardware reproductions.

## Decisions

- Keep the SD bootstrap fixed and publish only `cellos.uimg` over direct static
  TFTP; avoid OTP, DHCP, ICS, WSL shutdown, and repeated SD writes.
- Build `/bin/input` without QEMU VirtIO MMIO only for the isolated RPi3
  artifact; preserve the generic QEMU input binary.
- Select BCM mini-UART RX for `board-rpi3`; preserve PL011 RX for generic
  AArch64/QEMU.

## Lessons

- Disconnect adapter TX from Pi RX during U-Boot autoboot; reconnect it only
  after Cellos reaches the shell to avoid injected characters stopping U-Boot.
- A successful UART TX log does not prove RX: the final gate must send a command
  and observe its semantic shell response.
- Reset the USB-TTL adapter after Windows reports that the device is not
  functioning; a visible COM port alone did not guarantee incoming bytes.

## Next steps

- Reduce temporary `T15`/`ANM` trace-marker noise in a separate verified slice.
- Treat RPi3 as the ARM64 boot/runtime regression lane, not proof of complete G1
  peripheral or real-time coverage.
