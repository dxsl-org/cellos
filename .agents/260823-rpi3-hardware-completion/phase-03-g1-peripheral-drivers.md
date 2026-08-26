---
phase: 3
title: "G1 Peripheral Sensor Drivers"
status: in-progress
priority: P2
effort: "3d"
dependencies: []
tier: medium
---

# Phase 03: G1 Peripheral Sensor Drivers

## Overview

Create concrete sensor and display driver cells that consume the existing
BCM BSC1 (I2C) and SPI0 bus implementations on RPi3. This phase proves the
G1 Robot & Embedded peripheral pipeline works end-to-end with real hardware.

## Requirements

- At least one I2C sensor driver cell (MPU6050 or BNO055) reading real data.
- At least one SPI display driver cell (SSD1306 or ST7789) rendering pixels.
- Both use the established `sensor-demo` pattern: `declare_manifest!`,
  `BcmBscI2c::open()` / `BcmSpi0::open()`, in-process register protocol.
- Physical RPi3 test with wired sensors proves real data acquisition.

## Architecture

Existing infrastructure:
- `cells/drivers/i2c-bcm/`: BCM BSC1 polling I2C with `ViI2c` trait, repeated
  START, 7-bit addressing, NACK distinction. Ready for register-addressed sensors.
- `cells/drivers/spi-bcm/`: BCM SPI0 polling SPI, Mode 0, native CS0, full-duplex
  transfer and TX-only write. Ready for display command/data streams.
- `hal/traits/i2c/`: `I2cAdapter<T>` for embedded-hal 1.0 compatibility.
- Resource registry: BSC1 `0x3F804000/0x1000` under `DEV_I2C`, SPI0
  `0x3F204000/0x1000` under `DEV_SPI`, GPIO under `DEV_GPIO`.
- Manifest flags: `i2c = true`, `spi = true`, `gpio = true` are v2 supported.

Key constraint: MMIO windows are exclusive. One cell owns BSC1; one cell owns
SPI0. Multiple I2C sensors on the same bus must share a single owning cell.

### Sub-phase 3a: I2C Sensor Cell (MPU6050)

MPU6050 is a widely available 6-axis IMU at I2C address `0x68` (or `0x69`
with AD0 high). Its register protocol uses `write_read` for register reads
and `write` for register writes — exactly what `ViI2c` provides.

### Sub-phase 3b: SPI Display Cell (SSD1306)

SSD1306 is a 128×64 monochrome OLED controller. SPI variant uses:
- MOSI for data, SCK for clock, CS for chip select (native CS0 works).
- A D/C (data/command) GPIO pin to distinguish commands from pixel data.
- Optionally a RST GPIO pin.

The D/C GPIO requirement means the cell needs both `spi = true` and
`gpio = true` in its manifest.

## Assumptions

- **Claim:** `BcmBscI2c::open()` succeeds on RPi3 with a wired I2C sensor
  because BSC1 MMIO is already allowlisted.
  **Confidence:** high (GPIO/I2C loopback NACK test passed on RPi3 physical board)
  **How to verify:** Boot with MPU6050 wired, read WHO_AM_I register.

- **Claim:** SPI Mode 0 is correct for SSD1306.
  **Confidence:** high (SSD1306 datasheet specifies Mode 0 / CPOL=0 CPHA=0)
  **How to verify:** Check SSD1306 datasheet timing diagram.

- **Claim:** GPIO pins for D/C and RST can be controlled from a cell that
  also holds SPI0 MMIO, since GPIO window is a separate MMIO class.
  **Confidence:** high (resource registry allows concurrent `DEV_GPIO` + `DEV_SPI`)
  **How to verify:** Check `resource_registry.rs` for RPi3 allowlist overlap rules.

## Related Files

- Create: `cells/drivers/imu-mpu6050/` (new crate: `Cargo.toml`, `src/lib.rs`)
- Create: `cells/drivers/display-ssd1306/` (new crate: `Cargo.toml`, `src/lib.rs`)
- Modify: `Cargo.toml` (workspace members)
- Modify: `kernel/src/embedded-test-hooks/init` or `kernel/src/embedded/init`
  (if boot-spawned)
- Reference: `cells/demos/sensor-demo/src/main.rs` (pattern template)
- Reference: `cells/drivers/i2c-bcm/src/controller.rs`
- Reference: `cells/drivers/spi-bcm/src/controller.rs`

## Implementation Steps

### 3a: MPU6050 I2C Sensor Cell

1. Create `cells/drivers/imu-mpu6050/` with standard cell structure:
   `Cargo.toml` depending on `ostd`, `hal-i2c`, `i2c-bcm`.

2. Implement `src/lib.rs`:
   - `declare_manifest!` with `i2c = true`.
   - `cell_main!` entry point.
   - `BcmBscI2c::open()` to acquire BSC1.
   - Read WHO_AM_I register (`0x75`) to verify device presence.
   - Configure: wake from sleep (PWR_MGMT_1 `0x6B`), set gyro/accel ranges.
   - Periodic read loop: read 14 bytes from register `0x3B` (accel XYZ,
     temp, gyro XYZ), convert to scaled values, print via UART.

3. Add to workspace `Cargo.toml` members.

4. Build for AArch64 RPi3, deploy, wire MPU6050 to BSC1 (SDA=GPIO2, SCL=GPIO3).

5. Boot, verify WHO_AM_I reads `0x68` and accelerometer shows gravity vector.

### 3b: SSD1306 SPI Display Cell

1. Create `cells/drivers/display-ssd1306/` with standard cell structure:
   `Cargo.toml` depending on `ostd`, `hal-spi`, `spi-bcm`, `hal-gpio`.

2. Implement `src/lib.rs`:
   - `declare_manifest!` with `spi = true, gpio = true`.
   - `cell_main!` entry point.
   - `BcmSpi0::open()` for SPI bus; GPIO for D/C pin control.
   - SSD1306 init sequence: display off, set MUX ratio, display offset,
     start line, segment remap, COM scan direction, COM pins config,
     contrast, entire display on (follow RAM), normal display, clock
     divide/osc freq, charge pump enable, display on.
   - Write a test pattern (checkerboard or "Cellos" text using `FONT8X8`).

3. Add to workspace `Cargo.toml` members.

4. Build for AArch64 RPi3, deploy, wire SSD1306:
   - MOSI → SDA (GPIO10), SCK → SCL (GPIO11), CS → CE0 (GPIO8),
     D/C → GPIO24 (or another free pin), RST → GPIO25.

5. Boot, verify display shows the test pattern.

## Success Criteria

- [ ] MPU6050 WHO_AM_I register reads `0x68` on physical RPi3 (pending physical sensor wiring).
- [ ] Accelerometer Z-axis shows ~1g when board is level (pending physical sensor wiring).
- [ ] SSD1306 displays a visible test pattern on physical RPi3 (pending physical display wiring).
- [x] Both cells compile without warnings for AArch64 target.
- [x] Existing `i2c-bcm` and `spi-bcm` register-model tests still pass (12/12).

## Security Considerations

- Sensor cells run as unprivileged user cells with only the declared
  manifest capabilities. No kernel privilege escalation.
- MMIO exclusivity prevents two cells from fighting over the same bus.
- GPIO pin selection must not conflict with console UART (GPIO14/15) or
  other critical pins.

## Risk Notes

- Physical sensor availability: if MPU6050 is unavailable, BNO055 or any
  I2C device with a known register map can substitute.
- SPI clock speed: `BcmSpi0` uses a fixed divider. If the default clock
  is too fast for SSD1306 (max 10 MHz SPI), the divider may need adjustment.
  The current controller has no speed configuration API — may need a minimal
  divider-set extension.
- GPIO D/C timing: SSD1306 requires D/C to be stable before CS assertion.
  Verify timing with the polling SPI implementation.

## Deviation Log

None.
