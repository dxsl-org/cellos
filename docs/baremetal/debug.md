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
qemu-system-aarch64 -machine raspi3b -m 1G -display none -serial null -serial stdio -kernel target/aarch64-unknown-none-softfloat/release/vicell-kernel -s -S
```

## 3. Hardware Debugging via JTAG (Raspberry Pi 3)
To debug directly on physical hardware, you configure the board to expose internal JTAG test access port (TAP) signals.

1. **GPIO Multiplexing**: In kernel boot setup, configure GPIO pins **22 through 27** to **Alternate Function 4 (Alt4)** to enable the JTAG interface (TRST, RTCK, TDO, TCK, TDI, TMS).
2. **Wiring**: Connect your JTAG debugger adapter leads to GPIO 22–27 and Ground.
3. **OpenOCD**: Launch OpenOCD on your host machine with the appropriate interface driver and target configuration (e.g., `bcm2837.cfg`). OpenOCD connects directly to the physical CPU cores via hardware TAP lines.

## 4. Attaching GDB and Inspecting State
Open a separate terminal window and launch GDB loaded with the compiled kernel ELF executable containing debug symbols.

> [!IMPORTANT]
> Ensure you specify the unstripped ELF binary (`target/aarch64-unknown-none-softfloat/release/vicell-kernel` or `kernel8.elf`), **not** the raw disk image (`kernel8.img`). Raw images lack DWARF symbol tables.

```bash
gdb-multiarch target/aarch64-unknown-none-softfloat/release/vicell-kernel
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
| **G2** | **Generic x86_64 PC**<br>*(Intel / AMD PC)* | x86_64 | **Intel DCI / USB3 Debug**: Direct Connect Interface via USB 3.0 / UART. | N/A *(Intel DCI OpenIPC or QEMU GDB stub)* | `i386:x86-64` |
| **G3** | **Radxa ROCK 5 / OPi 5+**<br>*(Rockchip RK3588)* | ARM64 | **ARM SWD / JTAG**: Dedicated SWD header or muxed SDMMC pins. | `target/rk3588.cfg` | `aarch64` |
| **G3** | **SiFive P870 / X390**<br>*(Next-Gen RISC-V)* | RV64 | **SiFive Debug TAP**: Standard 10-pin RISC-V Debug connector. | `target/sifive_p870.cfg` | `riscv64` |