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
- **RPi3-B — identified and boot/peripheral path exercised.** Firmware and
  U-Boot report board revision `a22082`, `RPI 3 Model B`, 948 MiB DRAM, and
  unique serial `000000003d042795`. After the host reboot and elevated network
  setup, TFTP transferred and checksum-verified the 9,637,952-byte `cellos.uimg`.
  `.agents/debug/rpi3-b-post-reboot-boot.raw` records SD discovery, all four MBR
  partitions, FAT16/FAT32/littlefs/RedoxFS mounts, policy and kernel self-test
  passes, BCM display registration, a completed first scanout flush, shell
  readiness, and `89 PASS, 0 FAIL` from the VFS integration suite. This is
  exact-device development/hardware-integration evidence only. Connecting HDMI
  after firmware startup produced black / `No Signal`. Repeating from power-off
  with HDMI connected and the display active before firmware startup removed
  the EDID errors; the display showed U-Boot, the Cellos boot log, and the
  `Cellos >` prompt. This confirms only the reproduction condition: late
  connection failed while cold-connected boot displayed output. It does not
  isolate firmware EDID sampling, display handshake/input behavior, or driver
  behavior as the root cause. Because no named reviewer approval is recorded
  for the mailbox unsafe DMA-page copies, the observation is non-qualifying and
  the HDMI visual hardware gate remains `governance-gated`.
- **Historical capture — exact board unassigned.**
  `.agents/debug/rpi3-hdmi-data-path-long-capture.raw` reports revision
  `a22082`, reaches the Cellos shell, mounts FAT16/FAT32/littlefs, registers the
  BCM display driver, and completes its first scanout flush. It contains no
  unique serial, so it cannot be attributed to RPi3-A or RPi3-B.
- **Current access state.** COM4 and the direct 100-Mbps Ethernet link were
  usable for the RPi3-B run. The repository TFTP server was stopped immediately
  after the payload transfer and boot capture.
- **Available peripherals.** One HDMI cable is available for external-display
  work. A camera is available but its model/interface is still unrecorded and
  sensor integration is deferred in the current session order.

## Placeholder-Only Board Entries

- `q35-x86_32`
- `virt-riscv32`
- `virt-aarch32`

These entries exist for documentation and future expansion, not as active
hardware claims.

## Shared-Lane Rule

If a hardware change affects a common peripheral mechanism, update the shared
driver or HAL layer once rather than cloning the behavior per board.
