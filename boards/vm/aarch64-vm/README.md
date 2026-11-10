# aarch64-vm — Generic AArch64 Virtual Machine Profile

## Purpose

The `aarch64-vm` descriptor is the hosted AArch64 VM profile: QEMU's `virt`
machine with a GICv3, under TCG, the Apple HVF accelerator, or KVM. It is bound
at build time by the kernel feature `board-aarch64-vm`, which selects the HAL
backend `hal/arch/arm/src/aarch64/backend/aarch64_vm.rs`. The default AArch64
build (QEMU virt, GICv2) is unchanged.

## Hardware Contract

- **Memory map:** QEMU virt (RAM at `0x4000_0000`, kernel linked at
  `0x4008_0000`, not relocatable). RAM size comes from the firmware tree's
  memory node; without a tree the 256 MiB fallback in `board.rs` applies.
- **Interrupt controller:** GICv3 with affinity routing and the ICC system
  registers. Distributor and the first redistributor region come from the
  firmware tree's `arm,gic-v3` node and must lie inside the kernel's MMIO
  identity maps; a direct ELF boot (no tree) uses QEMU virt's fixed GICv3 map
  (`aarch64-vm.dts`). A tree without a GICv3 stops boot with a message.
- **Timer:** ARM generic timer (CNTP PPI 30 at EL1, CNTHP PPI 26 for an EL2 VHE
  host).
- **Secondary cores:** PSCI `CPU_ON` over the conduit the tree declares (`hvc`
  on QEMU virt without EL3). Up to 16 vCPUs: the kernel's hart index is
  MPIDR.Aff0.
- **Console:** PL011 at `0x0900_0000`.
- **Devices:** QEMU virt's virtio-mmio slots (block, net, input, GPU, RNG),
  PL031 RTC, PL061 GPIO. The PCIe ECAM bus 0 is scanned only when the tree
  places it inside the kernel's MMIO maps (`highmem=off`).
- **DMA isolation:** `Optional`. The host hypervisor owns guest-physical
  translation; Cells inside this VM are not DMA-isolated from each other.
- **Not supported:** the EL2 host's guest vGIC (GICv3 virtualization uses
  `ICH_*` system registers, which the vGIC does not drive), and Apple
  Virtualization.framework, whose memory map, console and device transport
  differ (see `tools/apple-host/run-vz.sh`).

## Running

```sh
cargo build --release -p cellos-kernel --features board-aarch64-vm \
    --target aarch64-unknown-none-softfloat -Z build-std=core,alloc
bash scripts/build-aarch64-image.sh
# QEMU TCG, the CI-style lane:
LANE=virt-gicv3 bash scripts/qemu-aarch64-test.sh \
    target/aarch64-unknown-none-softfloat/release/cellos-kernel.Image
# Apple Silicon macOS:
bash tools/apple-host/run-hvf.sh
```
