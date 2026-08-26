# Scout Report: RPi3 U-Boot Static TFTP

## Verified Codebase Facts

- `kernel/linker-rpi3.ld:2` sets `ENTRY(_start)`.
- `kernel/linker-rpi3.ld:11` links board-rpi3 at `0x80000`.
- `kernel/build.rs:14` selects `kernel/linker-rpi3.ld` for `board-rpi3`.
- `gen_disk_rpi3.ps1:117` creates a raw binary with `aarch64-linux-gnu-objcopy -O binary`; `gen_disk_rpi3.ps1:158` names it `kernel8.img`.
- `hal/arch/arm/src/aarch64/boot.rs:37` documents/stashes the DTB pointer from `x0`.
- `docs/hardware-dev-guide.md:230` has a generic U-Boot/TFTP example using `0x40000000`; it must not be copied into the RPi3 plan because the RPi3 linker uses `0x80000`.

## Official References

- U-Boot Raspberry Pi docs: `rpi_3_defconfig` is the 64-bit Raspberry Pi 3B target; `rpi_arm64_defconfig` is a generic multi-board option using firmware DTB. https://docs.u-boot.org/en/v2026.04/board/broadcom/raspberrypi.html
- Debian `u-boot-rpi` arm64 file list includes `/usr/lib/u-boot/rpi_3/u-boot.bin`. https://packages.debian.org/sid/arm64/u-boot-rpi/filelist
- U-Boot `booti` docs describe flat/compressed Linux `Image`, which must be proven before applying to freestanding Cellos. https://docs.u-boot.org/en/v2026.04/usage/cmd/booti.html

## Session Recon Inputs

- Host static IP: `192.168.42.1`.
- Pi static IP: `192.168.42.2`.
- Keep WSL and Windows `SharedAccess` running.
- Fetch only the Cellos image from TFTP.
- Preserve/reuse current SD rollback.

## Constraints

- No OTP.
- No DHCP.
- No kernel NIC driver.
- No source/code edits in the planning turn.
