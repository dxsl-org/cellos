# Test Report — 2026-08-26 — RPi3 Physical Hardware

## Test Results Overview

- Host driver library tests: 15 passed, 0 failed.
- AArch64 device cells: build passed.
- `cellos-kernel` with `board-rpi3`: build passed with two existing dead-code warnings.
- Physical GPIO and SPI gates passed.
- Physical SD read/probe passed; filesystem write failed.
- I2C controller transaction completed with an explicit NACK because no sensor
  is attached.
- HDMI gate was not runnable because the RPi3 image omitted both compositor and
  BCM display cells.

## Preflight Evidence

- Windows exposes the CP210x USB-UART as `COM4`.
- Physical `Ethernet` adapter is up at 100 Mbps.
- UART command/response is confirmed at 115200 baud; board is at `U-Boot>`.
- Current payload transferred by TFTP after `usb reset`; uImage checksum passed.
- Cellos reached its interactive shell on the physical board.
- No Windows TFTP Python process is running.
- RPi3 target checks passed for `driver-imu-mpu6050`, `driver-display-ssd1306`,
  `driver-bcm-display`, `sensor-demo`, and `spi-demo`.
- Host library tests passed: BCM I2C 6, BCM SPI 6, BCM display mailbox 3.

## Physical Results

1. GPIO: PASS — physical pin 11 GPIO17 to pin 13 GPIO27 produced the expected
   rising edge.
2. SPI: PASS — physical pin 19 MOSI to pin 21 MISO returned `AA55`.
3. I2C: PARTIAL — BCM BSC1 opened and returned explicit data NACK at SHT3x
   address `0x44`; a sensor is required for real-data evidence.
4. SD read: PASS — SDHC card reported 30,318,592 sectors (~14.8 GiB); P1-P4
   partition metadata and FAT boot volume were readable.
5. SD write: FAIL — VFS mounted `/mnt/sd`, but create/write returned failure.
   RAMFS and littlefs control writes succeeded, isolating failure to FAT/SD
   block write. P5 is absent, so `/srv` RedoxFS could not mount.
6. HDMI: BLOCKED — `/bin/compositor` and `/bin/bcm-display` are absent from the
   packaged RPi3 image; no framebuffer transaction ran.

## Required Setup

- RPi3 Ethernet and USB-UART are connected and detected.
- Connect an HDMI display before the next power-on.
- Attach an I2C sensor and state its model/address for a real-data gate.
- Package and boot the BCM display driver plus compositor before HDMI testing.
- Diagnose the physical CMD24/FAT write path and provision P5 before the
  persistence power-cycle gate.

## Build Status

- AArch64 peripheral cells: PASS.
- RPi3 kernel: PASS.
- Network cleanup: Windows Ethernet restored to DHCP, firewall rule removed,
  and TFTP process stopped.
- Physical release evidence: PARTIAL — GPIO/SPI/read gates pass; SD write,
  sensor-backed I2C, and HDMI remain open.
