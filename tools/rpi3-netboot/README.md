# Raspberry Pi 3 static TFTP boot

This lane keeps Raspberry Pi OTP unchanged. The SD card loads firmware and
U-Boot locally. U-Boot then uses fixed addresses on the direct Ethernet link:

- Windows host: `192.168.42.1/24`
- Raspberry Pi 3: `192.168.42.2/24`
- served file: `cellos.uimg` over read-only TFTP/UDP 69

DHCP is not used, so Windows Internet Connection Sharing and WSL can remain
running during every board reboot.

## Safety model

- Python 3.12 binds the TFTP socket to `0.0.0.0` because that is the listener
  configuration proven to receive packets on this Windows host. The lane is
  pinned to the Pi address `192.168.42.2`.
- The firewall rule opens only UDP 69 on `192.168.42.1` and the selected
  Ethernet interface.
- TFTP is unauthenticated; keep this NIC on the direct Pi-to-PC cable.
- SD preparation checks the physical disk number and size, then creates a host
  backup before writing the bootstrap.
- No command writes `program_usb_boot_mode`; OTP is never programmed.

## Boot artifacts

The tested bootstrap uses U-Boot `v2026.07`, `rpi_3_defconfig`, with the Pi 3
control DTB embedded so U-Boot does not depend on the firmware handoff. Required config
features are `CONFIG_LEGACY_IMAGE_FORMAT`, `CONFIG_CMD_BOOTM`,
`CONFIG_CMD_TFTPBOOT`, `CONFIG_USB_HOST_ETHER`, and
`CONFIG_USB_ETHER_SMSC95XX`. `CONFIG_BOOTSTD_DEFAULTS` and `CONFIG_CMD_BOOTI`
must remain disabled: Cellos is a raw payload inside a legacy uImage, not a
Linux ARM64 Image-header payload.

Create the SD boot script with the matching U-Boot `mkimage`:

```bash
mkimage -A arm64 -O linux -T script -C none \
  -n "Cellos static TFTP" \
  -d tools/rpi3-netboot/boot.cmd tools/rpi3-netboot/root/boot.scr
```

With the SD mounted as `F:` and the Pi powered off, install the bootstrap from
Administrator PowerShell:

```powershell
pwsh -File .\tools\rpi3-netboot\prepare-rpi3-netboot.ps1 `
  -SdDriveLetter F -ExpectedDiskNumber 2 `
  -UbootImage .\.agents\debug\u-boot-rpi3-embedded-static-build\u-boot.bin `
  -BootScript .\tools\rpi3-netboot\root\boot.scr `
  -DeviceTree .\.agents\debug\rpi-firmware\bcm2710-rpi-3-b.dtb `
  -Confirm:$false
```

## Deploy and serve

Wrap a raw Cellos kernel in the legacy ARM64 uImage expected by `bootm`:

```powershell
pwsh -File .\tools\rpi3-netboot\deploy-rpi3-kernel.ps1 `
  -KernelImage .\path\to\kernel8.img
```

Apply the fixed host address after each Windows reboot, and apply the narrow
firewall rule when installing or updating this lane, from Administrator PowerShell:

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1 `
  -ApplyNetworkConfig -ApplyFirewall -PreflightOnly
```

For each boot, start the server before powering the Pi:

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1
```

For unattended autoboot logging, connect only Pi TXD0 (physical pin 8) to the
USB-to-TTL adapter RX plus ground. Leave adapter TX disconnected from Pi RXD0
(physical pin 10); the tested adapter injected input that interrupted U-Boot's
countdown. Reconnect that lead only for an interactive U-Boot session.

Use official Python 3.12 on this host. The Laragon Python 3.14 installation was
confirmed not to receive UDP packets from the physical Ethernet adapter.

U-Boot loads `cellos.uimg` at `0x01000000`. `bootm` validates it, relocates the
raw payload to `0x00080000`, and enters Cellos at EL2 with the firmware Device
Tree address in `x0`.

The verified RPi3 cell build disables the QEMU-only VirtIO input MMIO probe.
On the real board, `/bin/input` registered, selected the kernel-push fallback,
and remained alive through init service supervision without an abort at
`0x0A000000`.

After Cellos reaches the shell, reconnecting adapter TX to Pi RXD0 and sending
`help` at 115200 baud must print `ViCell Shell v0.2.1` and return to `ViCell >`.
This verifies BCM mini-UART RX through the kernel EV_ASCII relay and Input
Service; generic AArch64/QEMU continues to use PL011 RX.

The verified production payload also removes the legacy raw `T<EC>`, `M`, `A`,
and `N` hot-path bring-up probes. Static guards reject their return while keeping
the fault-only `FS0`-`FS3` diagnostics. The real-board 30-second boot and `help`
gate contained zero `T15` or `ANM` sequences.

## Restore the host network

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1 -RestoreNetwork
```
