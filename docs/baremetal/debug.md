# Cellos Bare-Metal Debugging Guide

## 1. Required Equipment
- USB-to-TTL serial adapter cable
- JTAG Debugger hardware (e.g., FT2232H-based adapter, Segger J-Link, or similar)
- OpenOCD (Open On-Chip Debugger)
- `gdb-multiarch` (or `aarch64-none-elf-gdb`)

## 2. Debugging on QEMU
Instead of launching QEMU normally, you must start the emulator with the `-s` and `-S` flags.
- `-s` acts as shorthand for `-gdb tcp::1234` (starts a GDB server listening on TCP port 1234).
- `-S` freezes the CPU at startup (before executing the first boot instruction).

> [!TIP]
> **Project Workflow**: In this repository, you can launch QEMU in debugging mode automatically using the PowerShell test runner:
> ```powershell
> .\run-rpi3.ps1 -Gdb
> ```

Manual QEMU command equivalent:
```bash
qemu-system-aarch64 -machine raspi3b -m 1G -display none -serial null -serial stdio -kernel target/aarch64-unknown-none-softfloat/release/cellos-kernel -s -S
```

## 3. Hardware Debugging via JTAG (Raspberry Pi 3)
To debug directly on physical hardware, you configure the board to expose internal JTAG test access port (TAP) signals.

1. **GPIO Multiplexing**: In kernel boot setup, configure GPIO pins **22 through 27** to **Alternate Function 4 (Alt4)** to enable the JTAG interface (TRST, RTCK, TDO, TCK, TDI, TMS).
2. **Wiring**: Connect your JTAG debugger adapter leads to GPIO 22–27 and Ground.
3. **OpenOCD**: Launch OpenOCD on your host machine with the appropriate interface driver and target configuration (e.g., `bcm2837.cfg`). OpenOCD connects directly to the physical CPU cores via hardware TAP lines.

## 4. Attaching GDB and Inspecting State
Open a separate terminal window and launch GDB loaded with the compiled kernel ELF executable containing debug symbols.

> [!IMPORTANT]
> Ensure you specify the unstripped ELF binary (`target/aarch64-unknown-none-softfloat/release/cellos-kernel` or `kernel8.elf`), **not** the raw disk image (`kernel8.img`). Raw images lack DWARF symbol tables.

```bash
gdb-multiarch target/aarch64-unknown-none-softfloat/release/cellos-kernel
```

Inside the GDB console, connect to the target remote session:
- **For QEMU** (default port `1234`):
  ```gdb
  target remote localhost:1234
  ```
- **For OpenOCD** (default port `3333`):
  ```gdb
  target remote localhost:3333
  ```

Once connected, execution remains halted. You can inspect memory, registers, and step execution instruction-by-instruction:
- Set breakpoints: `b _start` or `b kernel_main`
- Step source lines: `n` (next), `s` (step)
- Step assembly instructions: `ni` (nexti), `si` (stepi)
- Inspect CPU registers: `info registers`

## 5. Roadmap Hardware Debug Matrix

Different target boards in the Cellos roadmap require distinct hardware debug interfaces, pin multiplexing setups, and OpenOCD target configurations.

| Roadmap Stage | Target Board / Chipset | CPU Arch | Debug Interface & Pin Mux Setup | OpenOCD Target Config | GDB Architecture (`gdb-multiarch`) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **G1** | **Raspberry Pi 3B / 4B**<br>*(BCM2837 / BCM2711)* | ARM64 | **ARM JTAG**: Mux GPIO 22–27 to Alternate Function 4 (`Alt4`). | `target/bcm2837.cfg`<br>`target/bcm2711.cfg` | `aarch64` |
| **G1** | **StarFive VisionFive 2**<br>*(JH7110)* | RV64 | **RISC-V JTAG**: Dedicated 10-pin JTAG header on board. | `target/jh7110.cfg` | `riscv64` |
| **G1 (Sub)** | **SiFive E21 / CHERIoT**<br>*(RV32 Core)* | RV32 | **4-wire JTAG / cJTAG**: Standard TAP pins. | `target/sifive-e21.cfg` | `riscv32` |
| **G2** | **Milk-V Pioneer**<br>*(SOPHON SG2042 / X60)* | RV64 | **Workstation JTAG**: Dedicated 20-pin ARM/RISC-V JTAG header. | `target/sophgo_sg2042.cfg` | `riscv64` |
| **G2** | **Alibaba C930**<br>*(T-Head Xuantie)* | RV64 | **T-Head CKLink**: Dedicated JTAG / T-Head debug probe. | `target/thead_c930.cfg` | `riscv64` |
| **G2** | **Generic x86_64 PC**<br>*(Intel / AMD PC)* | x86_64 | **USB RS-232/UART** | N/A *(Intel DCI OpenIPC or QEMU GDB stub)* | `i386:x86-64` |
| **G3** | **Radxa ROCK 5 / OPi 5+**<br>*(Rockchip RK3588)* | ARM64 | **ARM SWD / JTAG**: Dedicated SWD header or muxed SDMMC pins. | `target/rk3588.cfg` | `aarch64` |
| **G3** | **SiFive P870 / X390**<br>*(Next-Gen RISC-V)* | RV64 | **SiFive Debug TAP**: Standard 10-pin RISC-V Debug connector. | `target/sifive_p870.cfg` | `riscv64` |

## 6. Board Support Policy

Cellos should admit new hardware by architecture first, then by SoC family, then by a single reference board. Do not try to support every market SBC with a unique board port.

The rule of thumb is:

- architecture gives the shared execution model;
- SoC family gives the shared low-level IP blocks, clocks, resets, interrupt controller, timers, and storage/network controllers;
- the board only captures wiring, power, boot straps, and peripheral placement.

Device Tree helps describe topology and MMIO addresses, but it does not replace drivers. If two boards use the same SoC, most of the work should stay in the SoC layer and the DTB, not in duplicated per-board driver code.

Recommended progression:

| Priority | Board / SoC | Why it belongs |
| :--- | :--- | :--- |
| Current / near-term | Raspberry Pi 3 / BCM2837 | Existing ARM64 regression target; keep it as the small, known-good hardware baseline. |
| Current / near-term | StarFive VisionFive 2 / JH7110 | RV64 G1 board already wired into the repo: `board-vf2`, JH7110 MMIO fallback, JH7110 SDHCI, and `scripts/vf2-flash.sh` all show active support. Prefer the 4 GB board, ideally revision 1.3B. |
| Next / G1 sub-track | ESP32-C6 DevKitC-1 | First real Cellos-Nano bring-up target: official, inexpensive MCU-class RV32 hardware with a high-performance RV32 core up to 160 MHz, plus LP core, Wi-Fi 6, BLE, and IEEE 802.15.4. The existing G1 roadmap already names ESP32-C3/C6 as the RV32 Nano sub-track in `docs/project-roadmap.md`. |
| Later / G1 sub-track | ESP32-P4 + ESP32-C6 board/module | Advanced Cellos-Nano target for HMI or robot-controller work. ESP32-P4 brings dual HP RV32 cores up to 400 MHz plus an LP RV32 core; the C6 acts as the managed wireless coprocessor, typically over SDIO with `esp-hosted`. M3-class modules commonly advertise 32 MB PSRAM and 16 MB flash, but treat `JC-ESP32P4-M3-DEV` or an equivalent as revision-sensitive until memory population, schematic, board revision, P4↔C6 interconnect, and C6 firmware evidence are verified. |
| Later priority | Raspberry Pi 5 / BCM2712 | A modern, long-lived ARM64 reference once G1 is stable; useful for validating a newer ARM64 platform without changing the support model. |
| G2 edge AI | One RK3588 board | Pick one reference board only, such as Radxa ROCK 5 or Orange Pi 5 Plus, and share the SoC drivers. Do not duplicate the same RK3588 work per board. |
| Conditional | ASUS Tinker Board / RK3288 | Original Tinker Board is ARM32 and not a priority for the current roadmap. It only makes sense if the project deliberately needs ARM32 coverage. |
| Conditional | ASUS Tinker V / Renesas RZ/Five | RV64, but only worth adding for a concrete industrial target such as CAN, RS232, or dual-LAN validation. |
| Conditional | Newer ASUS/Rockchip Tinker-class boards | Add only when the SoC matches the roadmap or a real customer need; novelty alone is not enough. |
| G2/server | Milk-V Pioneer / SG2042 | Keep this for server-scale or large-SMP work. It is not the first RV64 board to qualify. |

Admission criteria before promoting a board:

- The board adds architectural value, not just a newer sticker or a faster benchmark.
- Firmware, boot chain, and upstream documentation are sufficient to bootstrap and debug it.
- The board has a realistic lifecycle and availability window.
- The SoC exposes reusable IP blocks or drivers that help more than one board.
- The board maps to an actual milestone or customer need.

Minimum qualification gates should be practical and explicit: boot, UART, timer, MMU where applicable, SMP where relevant, storage, network, GPIO, I2C, and reboot. If a board cannot clear those gates, it is not ready for first-class support.

ESP32-C6 and ESP32-P4 belong in the Cellos-Nano path, not the RV64 SBC path. The current RV32 code is still QEMU `virt` / OpenSBI / S-mode oriented in `hal/arch/riscv/src/rv32.rs`, while Espressif bring-up will need direct MCU M-mode boot, SoC-specific interrupt/timer/boot/flash/PSRAM support, and PMP/APM-style isolation. That is why the C6 comes first as the simpler official devkit, and the P4+C6 board comes later after the C6 port proves the Nano baseline. `esp-hal` lowers peripheral bring-up risk, but it does not remove the need for a separate SoC port. For memory layout work, keep the RV32 Nano path aligned with the existing paging model in `kernel/src/memory/paging.rs`.

The existing repository already points in this direction: `kernel/Cargo.toml` carries both `board-rpi3` and `board-vf2` features, `kernel/src/boot.rs` has a VF2 memory-map fallback, `kernel/src/task/drivers/mmc.rs` carries the JH7110 SDHCI path, `scripts/vf2-flash.sh` automates VF2 image creation, and `docs/specs/04-hardware.md` already treats VisionFive 2 as the RV64 G1 real-board target.
