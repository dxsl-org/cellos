#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
UBOOT_SOURCE=${UBOOT_SOURCE:-"$REPO_ROOT/.agents/debug/u-boot-v2026.07"}
UBOOT_BUILD=${UBOOT_BUILD:-"$REPO_ROOT/.agents/debug/u-boot-rpi3-embedded-static-build"}
UBOOT_DEPS=${UBOOT_DEPS:-"$REPO_ROOT/.agents/debug/u-boot-deps"}

export PATH="$UBOOT_DEPS/usr/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
export LD_LIBRARY_PATH="$UBOOT_DEPS/usr/lib/x86_64-linux-gnu"
export LIBRARY_PATH="$UBOOT_DEPS/usr/lib/x86_64-linux-gnu"
export CPATH="$UBOOT_DEPS/usr/include:$UBOOT_DEPS/usr/include/x86_64-linux-gnu"
export BISON_PKGDATADIR="$UBOOT_DEPS/usr/share/bison"
export M4="$UBOOT_DEPS/usr/bin/m4"

CELLOS_BOOTCOMMAND='usb start; setenv autoload no; setenv ipaddr 192.168.42.2; setenv serverip 192.168.42.1; setenv netmask 255.255.255.0; if tftpboot 0x01000000 cellos.uimg; then bootm 0x01000000 - ${fdt_addr}; else echo Cellos TFTP failed; fi'

grep -Eq '^VERSION = 2026$' "$UBOOT_SOURCE/Makefile"
grep -Eq '^PATCHLEVEL = 07$' "$UBOOT_SOURCE/Makefile"
make -C "$UBOOT_SOURCE" O="$UBOOT_BUILD" CROSS_COMPILE=aarch64-linux-gnu- rpi_3_defconfig
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --disable TOOLS_MKEFICAPSULE
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --disable OF_BOARD
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --disable OF_SEPARATE
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --disable OF_HAS_PRIOR_STAGE
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --enable OF_EMBED
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --disable BOOTSTD_DEFAULTS
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --disable CMD_BOOTI
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --set-val BOOTDELAY 0
"$UBOOT_SOURCE/scripts/config" --file "$UBOOT_BUILD/.config" --set-str BOOTCOMMAND "$CELLOS_BOOTCOMMAND"
make -C "$UBOOT_SOURCE" O="$UBOOT_BUILD" CROSS_COMPILE=aarch64-linux-gnu- olddefconfig
make -C "$UBOOT_SOURCE" O="$UBOOT_BUILD" CROSS_COMPILE=aarch64-linux-gnu- -j4

grep -F 'CONFIG_BOOTDELAY=0' "$UBOOT_BUILD/.config"
grep -F 'CONFIG_CMD_TFTPBOOT=y' "$UBOOT_BUILD/.config"
grep -F 'CONFIG_USB_ETHER_SMSC95XX=y' "$UBOOT_BUILD/.config"
grep -F 'CONFIG_OF_EMBED=y' "$UBOOT_BUILD/.config"
grep -F '# CONFIG_BOOTSTD_DEFAULTS is not set' "$UBOOT_BUILD/.config"
grep -F '# CONFIG_CMD_BOOTI is not set' "$UBOOT_BUILD/.config"
grep -F 'tftpboot 0x01000000 cellos.uimg' "$UBOOT_BUILD/.config"
sha256sum "$UBOOT_BUILD/u-boot.bin"
