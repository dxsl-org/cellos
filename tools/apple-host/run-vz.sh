#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Virtualization.framework (VZ / vfkit) is NOT a supported Cellos host yet.
#
# This script refuses instead of launching a guest that cannot print a byte.
# The `board-aarch64-vm` kernel targets the QEMU `virt` memory map; VZ's
# platform differs in every fact that kernel relies on:
#
#   * Load address. The kernel is linked at 0x4008_0000 and its arm64 Image
#     header declares it non-relocatable. VZLinuxBootLoader places the Image at
#     the start of VZ guest RAM, which is not 0x4000_0000 (QEMU's model of the
#     VZ platform, hw/vmapple/vmapple.c, puts RAM at 0x7000_0000 and the PCIe
#     ECAM window at 0x4000_0000).
#   * Console. The kernel's console is a PL011 at 0x0900_0000. VZ gives a Linux
#     guest a virtio-console serial port (hvc0); Cellos has no virtio-console
#     driver on the kernel console path.
#   * Devices. VZ attaches block/network/entropy as virtio-PCI behind its own
#     ECAM; the AArch64 kernel's driver cells use QEMU virt's virtio-mmio slots.
#   * Interrupt controller. VZ's GICv3 lies outside the kernel's MMIO identity
#     maps; the backend refuses such a firmware description at boot.
#
# Supported hosted path on Apple Silicon: tools/apple-host/run-hvf.sh (QEMU
# with the HVF accelerator, `-M virt,gic-version=3`).
set -euo pipefail

cat >&2 <<'EOF'
ERROR: Cellos does not run under Virtualization.framework / vfkit yet.
  The aarch64-vm kernel assumes the QEMU virt map: RAM at 0x4000_0000 with the
  kernel linked at 0x4008_0000 (non-relocatable), a PL011 console at
  0x0900_0000 and virtio-mmio devices. VZ provides none of these (different RAM
  base, virtio-console instead of a PL011, virtio-PCI devices).
  Use tools/apple-host/run-hvf.sh (QEMU + HVF) instead.
EOF
exit 2
