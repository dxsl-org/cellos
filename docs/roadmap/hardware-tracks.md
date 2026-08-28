# Hardware Tracks

**Last updated**: 2026-08-28

This page collects the hardware qualification lanes that matter for roadmap
reading. For the architecture split between board descriptors, SoC facts, and
shared drivers, see [system-architecture.md](../system-architecture.md).

## Board and SoC Ownership

- Root `boards/` owns board descriptors and audited fallback assets.
- `hal/soc/*` owns immutable SoC facts.
- Shared drivers stay single-copy in `cells/drivers/` or the relevant kernel
  integration path; boards do not fork UART, SDHCI, GIC/PLIC, PCIe, or similar
  mechanism code.

## Current Qualification Lanes

- RPi3 physical smoke is merged and should be treated as real hardware evidence.
- VF2, Pioneer, and RPi4 remain physical-only qualification lanes unless a log
  explicitly records PASS/FAIL/BLOCKED evidence.
- QEMU and compile-only checks are regression evidence, not board qualification.

## Available RPi3 Inventory

- **RPi3-A — identity and condition pending.** No revision, serial, or runtime
  evidence is currently bound to this physical board.
- **RPi3-B — identified; boot, peripheral, and HDMI path exercised.** Firmware
  and U-Boot report board revision `a22082`, `RPI 3 Model B`, 948 MiB DRAM, and
  unique serial `000000003d042795`. The reviewed 2026-08-28 HDMI image has
  SHA-256 `566b73a2e4b3499d564a8e40da41ff895cc1ff61f7388d1894881522c8e8e202`.
  The repository TFTP log independently records the final 9,642,048-byte
  transfer at 2026-08-28 11:14:54. Separately,
  `.agents/debug/rpi3-b-hdmi-reviewed-20260828.raw` contains an earlier boot at
  lines 37–210 and a later reviewed-image boot beginning around line 253. The
  later block records one 4,096-byte mailbox page, accepted cache begin/exact
  completion, framebuffer base `0x3e876000`, size 3,686,400, 1280x720, pitch
  5,120, BCM registration, fb-console damage, and a completed first scanout
  flush without a cell fault. The UART file has no host timestamp or image hash,
  so it does not itself prove the TFTP event's 11:14:54 timestamp. The user
  separately observed the cold-connected display remain lit for more than 10
  minutes with fb-console and cursor movement. This closes the HDMI visual gate
  at exact-device development evidence only; it does not establish production
  qualification. Connecting HDMI after firmware startup previously produced
  black / `No Signal`; that remains a reproduction condition, not an isolated
  root cause.
- **Historical capture — exact board unassigned.**
  `.agents/debug/rpi3-hdmi-data-path-long-capture.raw` reports revision
  `a22082`, reaches the Cellos shell, mounts FAT16/FAT32/littlefs, registers the
  BCM display driver, and completes its first scanout flush. It contains no
  unique serial, so it cannot be attributed to RPi3-A or RPi3-B.
- **Current access state.** COM4 and the direct 100-Mbps Ethernet link were
  usable for the RPi3-B run. The COM4 recorder and verified repository TFTP
  process were stopped after the final capture.
- **Available peripherals.** One HDMI cable is retained for regression testing.
  A camera is available but its model/interface is still unrecorded and sensor
  integration is deferred in the current session order.

## Placeholder-Only Board Entries

- `q35-x86_32`
- `virt-riscv32`
- `virt-aarch32`

These entries exist for documentation and future expansion, not as active
hardware claims.

## Shared-Lane Rule

If a hardware change affects a common peripheral mechanism, update the shared
driver or HAL layer once rather than cloning the behavior per board.
