# Scout Report: RPi3 Netboot Bootstrap

## Verified Codebase Facts

- `gen_disk_rpi3.ps1:153` copies VideoCore firmware and `kernel8.img` into the SD boot partition.
- `gen_disk_rpi3.ps1:37` expects a board-rpi3 release kernel before image generation.
- `tools/rpi3-firmware/README.txt:4` documents fetching `bootcode.bin`, `start.elf`, and `fixup.dat`.
- `tools/rpi3-firmware/config.txt:10` sets `arm_64bit=1`; `config.txt:11` names `kernel8.img`; `config.txt:12` enables UART.
- `docs/baremetal/load-cellos.md:10` currently starts with image flashing, so netboot should be added without removing existing recovery flow.
- Git precedent for RPi3 bring-up exists at `9b4aeead`; recent history for docs/probe cleanup was provided by session recon and should be rechecked before implementation if commit-specific claims matter.

## Official References

- Raspberry Pi docs, "Special bootcode.bin-only boot mode": https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#special-bootcodebin-only-boot-mode
- Raspberry Pi 2016 Ethernet article: https://www.raspberrypi.com/news/pi-3-booting-part-ii-ethernet-all-the-awesome/

## Session Recon Inputs

- Windows NIC target: `Ethernet`, ifIndex 14, Intel I226-V, disconnected/APIPA before cable.
- Wi-Fi remains internet.
- UDP 67 occupied only on `172.23.96.1`; UDP 69 free.
- Windows Python 3.14 available.
- WSL mirrored `eth1` exists but is intentionally not the serving path.

## Constraints

- No OTP.
- No kernel NIC driver.
- No permanent DHCP/TFTP service.
- Copy firmware from current SD before changing boot root; preserve recoverable local-boot backup.
- Plan artifacts only in this turn; no implementation files were created.
