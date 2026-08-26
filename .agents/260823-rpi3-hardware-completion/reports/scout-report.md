# Scout Report: RPi3 Hardware Completion

## Summary

Five items investigated. All five are feasible but vary dramatically in effort:

| Item | Effort | Blocking prerequisites |
|------|--------|----------------------|
| AArch64 semihosting exit | 4h | None; code may already compile |
| HDMI framebuffer | 5d | BCM mailbox MMIO facts, display capability generalization |
| Persistent SD write | 2d | RPi3 image provisioning, flush semantics |
| G1 peripheral drivers | 3d | Physical sensors required; bus APIs ready |
| LAN9514 Ethernet | 15d+ | DWC2 USB host stack from scratch; cache/DMA primitives |

## Key Findings

### 1. AArch64 Semihosting
- `qemu_exit::AArch64` is already the correct API (not the historical `AArch64Semihosting`).
- Current `kernel/src/main.rs:77-80` already uses the right type.
- No caller exists; scripts rely on timeout + serial markers.
- QEMU runner lacks `-semihosting` flag.
- Blocker `B-AARCH64-SEMHOSTING` in ledger is stale.

### 2. HDMI Framebuffer
- Zero BCM VideoCore mailbox support exists.
- BCM2837 MMIO profile lacks mailbox base `0x3F00_B880`.
- Compositor is backend-neutral except for VirtIO-specific registration (`PcieDriverCap`).
- `GpuGetResolution` returns hardcoded 1280x800.
- GPU cursor is VirtIO-specific; RPi must use software cursor fallback.

### 3. Persistent SD Write
- CMD24 write is implemented in SDHCI driver.
- FAT32 writes work. RedoxFS P5 at `/srv` is the accepted persistent FS.
- `/srv/cellos` is KMS-only namespace; general persistence uses `/srv/<name>`.
- `MmcBlock::flush()` is a no-op.
- RPi3 image tooling doesn't provision P5 RedoxFS.

### 4. G1 Peripherals
- BCM BSC1 I2C and SPI0 drivers are ready (`ViI2c`, `ViSpi` traits).
- `sensor-demo` and `robot-demo` show the consumer pattern.
- MMIO allowlist already covers BSC1, SPI0, GPIO, AUX.
- Kernel owns AUX mini-UART; user UART coprocessor needs ownership decision.
- SPI display needs D/C GPIO + Mode 0 only constraint.

### 5. LAN9514 Ethernet
- No USB support whatsoever in the codebase.
- No DWC2 MMIO facts, IRQ topology, or DMA cache primitives.
- Requires full USB host stack: DWC2 core + hub + LAN95xx protocol.
- Net service has QEMU-hardcoded MAC and e1000-only NIC lookup.
- Estimated 15+ days minimum; physical hardware mandatory.
