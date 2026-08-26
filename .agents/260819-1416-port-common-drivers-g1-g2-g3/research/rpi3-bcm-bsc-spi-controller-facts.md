## Current Cellos RPi3 DTS does not yet describe BSC1 or SPI0
**Verdict:** Phase 03 cannot claim a real BCM I2C/SPI port from current Cellos board data alone; DTS/board descriptor expansion is still required before driver code is credible.
- The in-tree RPi3 board only declares `compatible = "raspberrypi,3-model-b", "brcm,bcm2837"` and a single AUX mini-UART node at `serial@3f215040`.
- No `i2c@...` or `spi@...` nodes exist in the current Cellos RPi3 DTS, so MMIO/IRQ/pinctrl for BSC1/SPI0 are not yet first-party repo facts.
- The board metadata already matches the expected BCM2837 identity, so adding controller nodes is consistent with current board selection, not a new board family.
**Source:** [raspberry-pi-3-model-b.dts](/home/dmin/cellos/boards/raspberry-pi/3-model-b/raspberry-pi-3-model-b.dts:4), [raspberry-pi-3-model-b.dts](/home/dmin/cellos/boards/raspberry-pi/3-model-b/raspberry-pi-3-model-b.dts:23), [board.rs](/home/dmin/cellos/boards/raspberry-pi/3-model-b/board.rs:6)

## RPi3 reference DTS locks the target controllers and bus-to-physical mapping
**Verdict:** The target controllers for Cellos Phase 03 are `brcm,bcm2835-spi` at bus `0x7e204000` and `brcm,bcm2835-i2c` at bus `0x7e804000`; with the RPi3 peripheral window mapping, those become CPU physical `0x3f204000` and `0x3f804000`.
- The reference RPi3 DTS maps the peripheral bus window `0x7e000000..` onto CPU physical `0x3f000000..` through `ranges = <0x7e000000 0x3f000000 0x1000000 ...>`.
- The same DTS defines `spi@7e204000` with `compatible = "brcm,bcm2835-spi"` and `i2c@7e804000` with `compatible = "brcm,bcm2835-i2c"`.
- Their interrupts are `<0x02 0x16>` for SPI0 and `<0x02 0x15>` for BSC1.
- The Linux SPI binding example independently matches `compatible = "brcm,bcm2835-spi"`, `reg = <0x7e204000 0x1000>`, `interrupts = <2 22>`.
**Source:** `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:57`, `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:414`, `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:573`, https://github.com/torvalds/linux/blob/master/Documentation/devicetree/bindings/spi/spi-controller.yaml

## Default header pinmux is ALT0 on GPIO2/3 for BSC1 and GPIO7-11 for SPI0
**Verdict:** The fast path for G1 hardware access is the standard 40-pin header wiring, not alternate muxes.
- The reference RPi3 DTS pins `i2c1_gpio2` to GPIO2 and GPIO3 with `brcm,function = <0x04>`.
- The same DTS pins `spi0_gpio7` to GPIO7, GPIO8, GPIO9, GPIO10, GPIO11 with `brcm,function = <0x04>`.
- On Raspberry Pi convention this maps to BSC1 SCL/SDA on GPIO3/GPIO2 and SPI0 CS1/CS0/MISO/MOSI/SCLK on GPIO7/8/9/10/11.
- Current Cellos peripheral traits already expect synchronous master `write`/`read`/`write_read` for I2C and Mode 0 style `cs_select`/`cs_deselect`/`transfer`/`write` for SPI, so the existing trait surface fits this wiring.
**Source:** `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:200`, `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:279`, [13-peripherals.md](/home/dmin/cellos/docs/specs/13-peripherals.md:70), [13-peripherals.md](/home/dmin/cellos/docs/specs/13-peripherals.md:71)

## BSC1 can support bounded polling, but Linux evidence says to keep the first pass narrow
**Verdict:** Implement BSC1 as a bounded polling master with `write`, `read`, and exactly one `write_read` repeated-start path first; do not start with a general multi-message engine.
- The BCM2835 BSC register surface is small and stable: `C`, `S`, `DLEN`, `A`, `FIFO`, `DIV`, `DEL`, `CLKT`, with control bits for `I2CEN`, `ST`, `CLEAR`, `READ` and status bits for `CLKT`, `ERR`, `RXD`, `TXD`, `DONE`.
- Linux's `i2c-bcm2835` explicitly supports only one trailing read message and requires it to be last; unsupported message patterns return `-EOPNOTSUPP`.
- Linux maps timeout to `-ETIMEDOUT`, ACK failure to `-EREMOTEIO`, and clears `C`/`S` completion bits on exit.
- Linux also disables the hardware clock-stretch timeout (`CLKT = 0`) and marks the adapter `NO_CLK_STRETCH`, which is a warning against ambitious early support claims on BCM2835/BMC2837 I2C.
**Source:** https://sources.debian.org/src/bcm2835/1.71%2Bds-1/src/bcm2835.h/, https://github.com/torvalds/linux/blob/master/drivers/i2c/busses/i2c-bcm2835.c

## SPI0 register semantics are sufficient for a polling-only Mode 0 master
**Verdict:** Cellos should start SPI0 with polling-only master transfers, native CS0/CS1, and FIFO-driven loops keyed off `TXD`, `RXD`, `DONE`, and `TA`.
- The SPI0 register block exposes `CS`, `FIFO`, `CLK`, `DLEN`, `LTOH`, `DC`.
- The critical `CS` bits for a minimal driver are `TA`, `DONE`, `TXD`, `RXD`, `CLEAR_RX`, `CLEAR_TX`, `CPOL`, `CPHA`, and low bits `CS` for chip-select selection.
- Linux's DT binding example for this controller uses a standard SPI controller node with child devices and optional `cs-gpios`; this means native CS can be used first, while GPIO-backed or active-high quirks can stay out of the initial slice.
- Cellos already has a generic `ViSpi` trait and existing bit-bang SPI demos in Mode 0, so the software contract is already aligned with a conservative first hardware implementation.
**Source:** https://sources.debian.org/src/bcm2835/1.71%2Bds-1/src/bcm2835.h/, https://github.com/torvalds/linux/blob/master/Documentation/devicetree/bindings/spi/spi-controller.yaml, [13-peripherals.md](/home/dmin/cellos/docs/specs/13-peripherals.md:71), [13-peripherals.md](/home/dmin/cellos/docs/specs/13-peripherals.md:168)

## Provenance is mixed: use permissive sources for patterns, copyleft sources for facts only
**Verdict:** Phase 03 should be a clean-room rewrite from DTS/manual facts; do not port Linux or seL4 driver code verbatim.
- The local seL4 tree states kernel-level code is generally GPLv2 and its RPi3 DTS is explicitly derived from a Linux intermediate build stage, so it is a fact source, not a code-copy source.
- The Linux `bcm2835` I2C driver is GPLv2 and is therefore concept-only for Cellos.
- Tock is dual Apache-2.0/MIT and is safe for permissive pattern reuse, but this checkout does not contain a BCM283x I2C/SPI implementation to port directly.
- The local Redox checkout is its build system, not the driver repository, so it is not useful as a BCM controller source of truth.
**Source:** `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:1`, `/mnt/d/Cellos/.references/seL4/LICENSE.md:1`, `/mnt/d/Cellos/.references/Tock/LICENSE-APACHE:1`, `/mnt/d/Cellos/.references/Tock/LICENSE-MIT:1`, `/mnt/d/Cellos/.references/Redox/README.md:4`

## Hardware-gated items remain open and should not be silently assumed
**Verdict:** MMIO base, compatible strings, GPIO mux, and nominal IRQ numbers are verified; actual IRQ delivery, clock behavior, and repeated-start behavior on the target board remain hardware-gated.
- Current verified facts stop at DTS/manual/reference-driver evidence; they do not prove Cellos's BCM2836-local IRQ path will deliver SPI0/BSC1 interrupts correctly on the real board.
- The reference DTS sets `clock-frequency = <0x186a0>` for BSC1, which is 100 kHz; that is the safe default for first bring-up.
- No current Cellos repo evidence proves SPI0 native CS polarity against the exact attached device set, so start with standard active-low CS0/CS1 assumptions and test on hardware before widening.
- Repeated-start compatibility for actual sensors is still a board-lab question because Linux's own driver narrows supported message shapes and warns about clock stretching.
**Source:** `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:417`, `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:576`, `/mnt/d/Cellos/.references/seL4/tools/dts/rpi3.dts:583`, https://github.com/torvalds/linux/blob/master/drivers/i2c/busses/i2c-bcm2835.c
