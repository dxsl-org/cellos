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

### x86_64 BIOS/UEFI smoke and serial capture

Build the kernel, then create the repository-relative BIOS+UEFI El Torito ISO:

```bash
cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc
bash scripts/x86/make-iso-ci.sh build/vicell-x86.iso
BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso
```

On Windows, the bounded UEFI/OVMF smoke is:

```powershell
pwsh -File build/boot-x86-uefi.ps1 -Iso build/vicell-x86.iso -Ovmf C:\path\to\OVMF_CODE.fd
```

The real-hardware diagnostic console is a PC-compatible 16550 COM1 at I/O
`0x3F8`, IRQ4, configured as `115200 8N1`, no flow control. Use a real RS-232
adapter for a DB9 port; a 3.3 V TTL adapter is not electrically compatible.

The x86 hardware gate order is strict:

1. Firmware loads Cellos and the boot banner appears.
2. COM1 transmit and polled receive work without ACPI; keep this gate open
   until IRQ4 receive is confirmed after MADT supplies the route.
3. RSDP and checksummed MADT, MCFG, and HPET tables are reported. MADT is a
   dependency of the pending COM1 IRQ4 witness, not permission to skip it.
4. HPET and LAPIC timer initialization succeeds.
5. x86 SMP startup and cross-core scheduling succeeds.
6. PCIe topology is enumerated from MCFG.
7. NVMe is discovered and performs bounded I/O.

Ethernet is not an early bring-up gate. The e1000 Driver Cell accepts only
Intel `8086:100e`; an `e1000e` endpoint can exercise the negative gate with
`X86_NIC_MODEL=e1000e bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso`.

Evidence status as of 2026-08-17: QEMU q35 BIOS and OVMF/UEFI reached the
scheduler; ACPI-derived timer and bus-0 ECAM initialization passed; an emulated
`8086:10d3` e1000e endpoint was rejected. Dell OptiPlex, N100/N5105, physical
COM1 IRQ4, x86 SMP, PCIe behind bridges, NVMe, and DMAR/VT-d remain
hardware-gated; this document does not claim they have run on physical hardware.

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
