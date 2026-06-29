# Cellos on Milk-V Pioneer (SG2042) — Board Bring-Up Guide

Boot Cellos on a Milk-V Pioneer Box (StarFive SG2042, 64-core T-Head C910 RISC-V) and reach
an interactive `Cellos>` shell via UART serial. All Cells run from the embedded VIFS1 ramdisk —
no NVMe driver required at this stage.

---

## Hardware Required

| Item | Notes |
|------|-------|
| Milk-V Pioneer Box or Pioneer v1.3 | 64 GB DDR4 RAM typical |
| USB drive or SD card, ≥ 1 GB | For the Limine UEFI boot image |
| USB-to-UART adapter (3.3 V TTL) | CP2102, CH340G, or FTDI FT232R |
| Linux host machine | For flashing (`dd`); WSL2 works with caveats |

### UART Connection

Pioneer exposes a debug UART (NS16550 via the front-panel 40-pin or a dedicated debug port).
Check the Pioneer schematic for exact pin assignments; typical is a 3-pin header (GND/TX/RX)
on the board edge.

Host terminal settings: **115200 8N1**

```bash
minicom -D /dev/ttyUSB0 -b 115200
# or
screen /dev/ttyUSB0 115200
```

---

## Architecture Notes (SG2042 vs QEMU virt)

| Component | QEMU virt | Pioneer SG2042 | Compatible |
|-----------|-----------|----------------|------------|
| DRAM base | 0x80000000 | 0x80000000 | ✅ identical |
| PLIC | 0x0C000000 (`sifive,plic-1.0.0`) | 0x0C000000 (`thead,c900-plic`) | ✅ same address |
| CLINT | 0x02000000 (`sifive,clint0`) | 0x02000000 (`thead,c900-clint`) | ✅ same address |
| UART | NS16550 @ 0x10000000 | DW APB UART @ 0x7040000000 | ⚠ addr inaccessible in sv39 → **SBI DBCN** |
| VirtIO | 0x10001000+ | absent | ✅ VIFS1 ramdisk fallback |
| RTC | Goldfish @ 0x00101000 | DW APB RTC @ 0x7020000000 | ⚠ inaccessible → epoch = 0 |
| Boot | Limine (direct) | Limine UEFI via U-Boot | ✅ |

**Console I/O path on Pioneer:** The SG2042 UART is at physical address `0x7040000000`,
which exceeds the sv39 virtual address limit and cannot be identity-mapped. Cellos (when built
with `--features board-pioneer`) forces `uart_base = 0` and routes all console I/O through
the SBI Debug Console extension (DBCN). OpenSBI on Pioneer provides DBCN, so the interactive
shell still works fully via UART serial.

---

## Prerequisites

```bash
# Rust RISC-V target
rustup target add riscv64gc-unknown-none-elf

# Image-creation tools (Debian/Ubuntu)
sudo apt install parted util-linux dosfstools curl
```

---

## Build and Flash

```bash
# 1. Build kernel + create pioneer-boot.img (256 MB GPT + FAT32)
./scripts/pioneer-flash.sh

# 2. Write to a USB drive (replace /dev/sdX — verify with lsblk!)
sudo ./scripts/pioneer-flash.sh /dev/sdX

# Windows (PowerShell + WSL2):
.\scripts\pioneer-build.ps1
```

The script:
1. Downloads `BOOTRISCV64.EFI` (Limine v12) via `scripts/download-limine.sh` if not cached
2. Compiles the kernel with `--features board-pioneer --release`
3. Creates a 256 MB GPT image with a 200 MB EFI System Partition
4. Populates: `EFI/BOOT/BOOTRISCV64.EFI`, `limine.conf` (KASLR=no), `vicell-kernel`

Insert the USB drive into the Pioneer, set UEFI boot order to USB, and power on.

---

## Boot Sequence

Expected serial output at 115200 baud:

```
[U-Boot SPL]
U-Boot SPL 2024.x (Pioneer SG2042)
...

[Limine]
Limine 12.x.x
Loading /vicell-kernel ...
Booting ...

[Cellos kernel]
Cellos v0.2.x — RISC-V 64 — Cellular SAS
[boot] Limine memory map: N entries
[boot] Usable RAM: 0x84200000 – ...
[platform] Pioneer SG2042: SBI-console PLIC=0xc000000+0x4000000 CLINT=0x2000000
[uart] RX/TX base = 0x0   (SBI DBCN)
[fs] VIFS1 embedded ramdisk: 8 cells
[init] Starting Cellos Orchestrator...
[init] cell not found — skipping: /bin/compositor
[init] cell not found — skipping: /bin/input
[init] cell not found — skipping: /bin/net
[vfs] RamFS ready
[shell] Cellos shell ready

Cellos>
```

> **Note**: "cell not found — skipping" messages are **expected** on Pioneer. VirtIO net/input/compositor
> are absent on real hardware; only the VIFS1-embedded Cells (shell, vfs, cat, ls, echo) start.

---

## Known Limitations (Current Bring-Up)

| Feature | Status | Notes |
|---------|--------|-------|
| Interactive shell (`cat`, `ls`, `echo`) | ✅ | Via SBI DBCN console |
| VFS RamFS | ✅ | Embedded in kernel |
| RTC / wall clock | ⚠ | Epoch = 0 (SG2042 RTC at sv39-inaccessible addr) |
| NVMe / PCIe storage | ❌ | G2 driver work — NVMe Cell planned |
| Network (TCP/MQTT) | ❌ | No VirtIO; Pioneer NIC driver future work |
| SMP (64 cores) | ❌ | Secondary hart start tested in QEMU; Pioneer SMP requires HSM firmware support |
| GPIO / peripherals | ❌ | SG2042 GPIO at high addresses; Driver Cell planned |

---

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| No serial output | UART adapter wiring | Check GND/TX/RX; try swapping TX↔RX |
| Limine not found | U-Boot UEFI support off | Upgrade Pioneer firmware; or `load usb 0:1 0x84000000 vicell-kernel; bootefi 0x84000000` at U-Boot prompt |
| `[uart] RX/TX base = 0x10000000` | Wrong build — missing `board-pioneer` feature | Rebuild with `--features board-pioneer` |
| Kernel stuck after `[boot]` | Wrong DRAM base in fallback | Ensure Limine UEFI boot is used (not bare SBI direct load) |
| Input not echoed | SBI DBCN read returns -1 | OpenSBI version too old (< 1.2); upgrade firmware |

### Manual U-Boot Boot (if Limine EFI fails)

At the U-Boot `=>` prompt:

```
# Assumes USB drive is device 0, partition 1
load usb 0:1 0x84000000 vicell-kernel
bootefi 0x84000000
```

---

## Hardware Compatibility Notes

The Milk-V Pioneer SG2042 uses T-Head C910 cores with a PLIC (`thead,c900-plic`) and CLINT
(`thead,c900-clint`) at the same base addresses as QEMU virt (`0x0C000000` / `0x02000000`).
Cellos discovers these from the DTB and falls back to the same defaults, so no board-specific
PLIC/CLINT code is needed.

The only real deviation is the UART: `snps,dw-apb-uart` at `0x7040000000` is unreachable in
sv39 (`uart_base` is forced to `0` by the `board-pioneer` Cargo feature), so all console I/O
goes through OpenSBI's DBCN extension — effectively identical to a firmware console, with no
loss of interactivity.
