# Phase03 RPi3 BCM hardware gates — 2026-08-20

## Result

PASS on Raspberry Pi 3 Model B (`boardrev a22082`) using the TFTP kernel paired
with its embedded VIFS1 development cells. No SD/OTP write was performed.

## Payload

- Raw kernel SHA256: `4BD5A180CF69173DA9E8B24C81681771D1FAA99FF7B6EB26CCE7D5AF40F1232D`
- Legacy uImage SHA256: `9993D5C3658264C4B4E69AA8B2D2A6762C7B7958C3EE32EDCD615629A8D3A492`
- UART transcript: `/home/dmin/cellos-worktrees/common-drivers-g1-g2-g3/phase03-rpi3-vifs1-first-uart.log`
- TFTP transferred 9,572,416 bytes and U-Boot checksum verification passed.
- Cellos reached the interactive shell after SD/FAT and policy initialization.

## Physical wiring

- GPIO gate: physical pin 11 (GPIO17 output) to pin 13 (GPIO27 input).
- SPI gate: physical pin 19 (GPIO10/MOSI) to pin 21 (GPIO9/MISO).
- USB-UART TX remained disconnected during autoboot and was reconnected only
  after `Cellos >` appeared.

## Decisive markers

- UART line 270: `=== Cellos shell ready ... ===`
- UART line 278: `[phase03-gpio] negative precheck PASS`
- UART line 279: `[phase03-gpio] PASS: GPIO17->GPIO27 rising edge detected`
- UART line 288: `[phase03-i2c] PASS: explicit data NACK from BCM BSC1`
- UART line 289: `[phase03-gpio-actuator] PASS: GPIO17 high/low readback`
- UART line 299: `[phase03-spi] PASS: BCM SPI0 MOSI-MISO loopback AA55`
- No panic or Cell fault marker matched the retained transcript.

The explicit I2C data NACK is the accepted no-sensor gate: it proves the BCM
BSC1 controller completed a real transaction and surfaced the hardware error
class rather than returning a synthetic reading.

## Blockers removed during the gate

1. `exec /bin/<demo>` uses the caller-owned ELF route and correctly cannot mint
   MMIO authority. The reviewed path route remains the capability-safe launcher.
2. Non-bootstrap paths previously preferred the stale removable-media cell
   table. The three embedded Phase03 demos now resolve VIFS1-first while keeping
   the boot-critical list separate.
3. `periph-demo` could recursively self-spawn through its pinned edge. A staged
   argv sentinel now gives the pinned child a bounded worker path and prevents
   another task storm.

## Scope note

This closes the BCM2837/RPi3 GPIO, BSC1 I2C, and SPI0 physical gates. It does not
claim DesignWare I2C/SPI hardware evidence; those remain conditional on a board
with an observed compatible/controller instance.
