# Cellos Bare-Metal Installation Guide

## 1. Prerequisites
- USB-to-TTL serial adapter cable
- MicroSD card (Class 10, 8 GB – 16 GB recommended)
- MicroSD card reader
- BalenaEtcher flashing software
- PuTTY terminal emulator

## 2. Choose the Boot Workflow

Use a full SD image only for first bootstrap or recovery. For repeated kernel
development on Raspberry Pi 3, keep firmware and U-Boot on the SD card and load
only the current Cellos kernel over a direct static TFTP link. This is the fast
iteration path: no card rewrite after each code change and no Raspberry Pi OTP
programming.

The tested development topology is:

- Windows Ethernet: `192.168.42.1/24`
- Raspberry Pi 3: `192.168.42.2/24`
- Direct LAN cable; Cat 5e or Cat 6 is sufficient
- SD card: Raspberry Pi firmware plus the Cellos U-Boot bootstrap
- TFTP payload: `cellos.uimg`
- Serial console: `COM4`, 115200 baud on the tested host

DHCP and Internet Connection Sharing are not used. WSL may remain running during
board reboots.

## 3. Flashing the OS Image

Treat this path as bootstrap or recovery only. For normal code iteration on
Raspberry Pi 3, keep the SD bootstrap fixed and use the TFTP lane in Section 7.
- Insert the MicroSD card into the card reader and connect it to your computer.
- Launch BalenaEtcher.
- Click **Flash from file** and select your Cellos OS `.img` file.
- Click **Select target** and choose the MicroSD card drive. (Ensure you select the correct target drive to avoid overwriting your system disk).
- Click **Flash!** and wait for the process to complete with a "Success" notification.

> [!CAUTION]
> Once flashing is complete, Windows may display a prompt stating the disk is unreadable and asking to format it. Click **Cancel** immediately—do not format the drive. Safely remove the MicroSD card.

## 4. Hardware Setup
- Insert the flashed MicroSD card into the slot on the underside of the Raspberry Pi 3.
- Connect the three leads (Ground/Black, RX/White, TX/Green) of the USB-to-TTL cable to the Raspberry Pi GPIO header according to the serial pinout diagram. Do not connect the external power supply to the Raspberry Pi yet.
- Plug the USB end of the TTL adapter cable into your computer.

## 5. Connecting to the Serial Console via PuTTY
Before launching PuTTY, you must identify which COM port number Windows assigned to your USB-to-TTL adapter.
- On Windows, right-click the **Start** button and select **Device Manager**.
- Expand the **Ports (COM & LPT)** section. Locate your serial adapter (e.g., "Silicon Labs CP210x..." or "USB-SERIAL CH340..."). Note the assigned port identifier in parentheses (e.g., `COM3` or `COM4`).
- Launch PuTTY.
- Under **Connection type**, select **Serial**.
- In the **Serial line** field, enter the identified COM port (e.g., `COM3`).
- In the **Speed** field, enter `115200` (the standard default baud rate for the Raspberry Pi serial console).
- Click **Open**. A blank terminal window will appear. It remains blank because the Raspberry Pi is currently powered off.

## 6. Booting a Fully Flashed Image
- Connect the power supply to the Raspberry Pi to power on the board.
- Observe the PuTTY terminal window. If the OS image is valid, the kernel boot log (`dmesg`) will begin outputting to the screen.
- Once the boot sequence completes, the terminal will display a login prompt (e.g., `Cellos>`).

## 7. Fast Raspberry Pi 3 Iteration over TFTP

Prepare the bootstrap SD once by following
[`tools/rpi3-netboot/README.md`](../../tools/rpi3-netboot/README.md). The U-Boot
build must keep `CONFIG_BOOTSTD_DEFAULTS` and `CONFIG_CMD_BOOTI` disabled because
Cellos is a raw kernel wrapped in a legacy uImage, not a Linux ARM64 Image.
Do not reflash the card for ordinary Cellos changes; keep the bootstrap fixed
and redeploy the kernel image over TFTP.

After building a new raw `kernel8.img`, wrap and publish it from PowerShell:

```powershell
.\scripts\build-aarch64-cells.ps1 -BoardRpi3
cargo build --release --features board-rpi3 `
  -p cellos-kernel --target aarch64-unknown-none-softfloat
aarch64-linux-gnu-objcopy -O binary `
  .\target\aarch64-unknown-none-softfloat\release\cellos-kernel `
  .\.agents\debug\rpi3-kernel8.img
pwsh -File .\tools\rpi3-netboot\deploy-rpi3-kernel.ps1 `
  -KernelImage .\.agents\debug\rpi3-kernel8.img
```

After each Windows reboot, restore the host's ActiveStore address from
Administrator PowerShell:

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1 `
  -ApplyNetworkConfig -ApplyFirewall -PreflightOnly
```

Before powering the Pi for each test, start the server:

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1
```

This host requires official Python 3.12. Its Laragon Python 3.14 installation
was verified not to receive UDP packets from the physical Ethernet adapter. A
successful transfer logs `TFTP RRQ cellos.uimg` followed by `TFTP DONE
cellos.uimg`; U-Boot then enters Cellos without another SD-card write.

Add `uart_2ndstage=1` to `config.txt` only when firmware-stage UART diagnostics
are needed. It is not required for normal netboot.

The current real-board gate reaches the Cellos scheduler and init services. The
RPi3 cell build disables the QEMU VirtIO input probe, so `/bin/input` relies on
the kernel UART push path instead of dereferencing `0x0A000000`. The verified
boot emitted `No VirtIO input device; relying on kernel push`, reached init
service supervision, and produced no `EC=0x24`, `FAR=0x0A000000`, input-service
death, or restart.

RPi3 console RX now enables AUX legacy IRQ 29 only after the kernel RX buffer
is initialized. That fixes the mini UART's 8-byte FIFO overrun under 115200-baud
bursts: the IRQ handler drains RX immediately, and direct polling stays as the
early-boot / lost-IRQ fallback. The real-board lane accepted raw
`echo 123456789\r`, returned `1 1 11` for `echo board-rpi3 \| wc\r`, and passed
100/100 unpaced burst commands in 1658 ms.

For unattended autoboot capture, connect Raspberry Pi TXD0 (physical pin 8) to
the adapter RX and connect ground, but leave the adapter TX lead disconnected
from Raspberry Pi RXD0 (physical pin 10). The tested adapter injected characters
that stopped U-Boot at its prompt when that return lead was connected. Reconnect
it only when an interactive U-Boot console is required.

This lane is an ARM64 boot/runtime regression lane, not yet proof that every G1
device feature works on Raspberry Pi 3.

RPi3 console input uses the BCM mini UART, while generic AArch64/QEMU keeps the
PL011 receiver. The real-board input gate connected adapter TX to RPi RXD0 only
after U-Boot had entered Cellos, sent `help` at 115200 baud, received the full
shell command listing, and returned to `ViCell >`.

The production RPi3 lane does not emit the old per-event `T<EC>`, timer `M`,
scheduler `N`, or context-switch `A` bring-up markers. A real-board boot and
interactive `help` gate reduced `T15` from `14,596` to `0` and `ANM` to `0`,
while retaining fault-only `FS0`-`FS3` diagnostics and bounded one-shot boot
markers.
