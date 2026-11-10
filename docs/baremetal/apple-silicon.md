# Apple Silicon Bare-Metal Bring-Up & Installation Guide

**Applies to:** Apple Silicon Macs (admitted chip order: Mac mini M1 `t8103, J274`, then M2 `t8112`, then M1/M2 Pro/Max/Ultra per ADR-0032).

---

## 1. Overview & Boot Architecture

Cellos runs on bare-metal Apple Silicon hardware without proprietary drivers, utilizing a clean-room permissive stack (OpenBSD ISC references and Linux MIT-licensed headers per ADR-0030/0032).

```
[iBoot (Apple signed)] 
         ↓
[m1n1 Stage 1 (Apple fuOS in 1TR)]
         ↓
[m1n1 Stage 2 on ESP]
         ↓
[U-Boot (EFI payload)]
         ↓
[Limine aarch64 EFI]
         ↓
[Cellos aarch64 Kernel Image (ARM64 Linux Image Header + FDT)]
```

### Supported Boot Regimes

1. **Installed / Distribution Path (Standard):**
   - Boot chain: `iBoot` → `m1n1 stage 1` → `m1n1 stage 2` → `U-Boot (EFI)` → `Limine aarch64` → `Cellos`.
   - Cellos ships no GPL components and boots via standard UEFI DeviceTree handover.
2. **Development / Diagnostic Path:**
   - Boot chain: `m1n1` stage 1 in proxy mode over USB-C CDC-ACM using `run_guest.py`.
   - Runs Cellos under the m1n1 hypervisor with virtual UART (vUART) and MMIO tracing for non-invasive debugging.

---

## 2. Partition Layout & Safety Rules

> **CRITICAL:** The internal NVMe drive (ANS2) contains macOS, the Apple recovery OS, system firmware (iBoot), and iSC partitions. Never overwrite or reformat these partitions!

### Recommended Partition Structure
- **Container 1 (APFS):** macOS system, recovery, and data volumes.
- **Partition 2 (FAT32, EFI System Partition - ESP):** `m1n1` stage 2, U-Boot, and Limine bootloader configuration.
- **Partition 3 (Cellos Root):** Cellos kernel image, initial ramdisk, and cell storage.

The storage driver enforces **`PartitionGuard`** (`libs/apple-rtkit`), which rejects any read or write operation whose Logical Block Addressing (LBA) falls outside Partition 3.

---

## 3. Preparation & Installation Steps

### Step 1: Permissive Security in 1TR (1-True-Recovery)
1. Shut down the Mac mini.
2. Press and hold the power button until **"Loading startup options"** appears.
3. Select **Options** → **Continue**.
4. Open **Terminal** from the Utilities menu.
5. Set the security posture for the target boot volume using the standard Asahi installer script or `bputil`:
   ```sh
   bputil -p -c
   ```
6. Follow the on-screen prompts to authenticate as an administrator and approve Permissive Security.

### Step 2: Provisioning ESP & Staging Cellos
1. Mount the ESP volume (`/Volumes/EFI` or disk slice).
2. Copy the Limine EFI binary (`BOOTAA64.EFI`) and `limine.cfg`:
   ```
   TIMEOUT=3
   :Cellos
   PROTOCOL=linux
   KERNEL_PATH=boot:///cellos-kernel
   DTB_PATH=boot:///apple-t8103-j274.dtb
   ```
3. Copy `cellos-kernel` (built with `target/aarch64-unknown-none-softfloat/release/cellos-kernel`).

---

## 4. Hardware Interfaces & Isolation

- **Interrupt Controller:** Apple AIC1. Core timers reach the CPU as FIQs; the kernel's cross-core IPI uses AIC1's IPI registers and arrives as an IRQ (the sysreg "fast IPI" path, also FIQ, is unused).
- **UART Console:** S5L UART (`apple,s5l-uart`) accessed via USB-C debug adapter or m1n1 vUART.
- **DMA Isolation:** Apple DART IOMMU (`DmaIsolation::Required`). All PCIe and USB bus masters must translate through 16 KiB DART page tables; bypass mode is strictly rejected by `AppleDartPt`.
- **Storage DMA:** Filtered by SART (Storage Address Resolution Table) hardware window registers.
- **CPU topology & SMP:** the m1n1-supplied DTB is the only source (`DtbPolicy::Required`). Boot reads `/cpus` once (`boot::firmware_cpus`, recorded by `firmware_smp::note`): the four Icestorm E-cores are MPIDR `0x0`–`0x3` (Aff1 = 0) and the four Firestorm P-cores `0x10100`–`0x10103` (Aff2 = Aff1 = 1). Secondaries start by spin table: m1n1 fills each node's `cpu-release-addr` (upstream Linux trees carry `<0 0>`), and a core whose word is still 0 is reported (`no cpu-release-addr in FDT`) and skipped. There is no QEMU model; the parser is pinned by host tests against the Linux v7.3 `t8103-j274` tree and an m1n1-filled copy (`tests/fixtures/dtb/`, `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu -- boot::firmware_tree_tests`). Bringing a P-core online on hardware is unverified.

---

## 5. Recovery & Rollback

If the machine fails to boot into Cellos:
1. Hold the power button to return to 1TR Startup Options.
2. Select your primary **Macintosh HD** volume to boot back into macOS.
3. Delete or repair the ESP entries without affecting macOS data.

---

## 6. Hosted Guest on macOS (no reinstall)

Cellos also runs as a guest on an Apple Silicon Mac without replacing macOS, through QEMU's `virt` machine with the HVF accelerator. This uses the `aarch64-vm` profile (`boards/vm/aarch64-vm`), not the bare-metal t8103 backend:

```sh
cargo build --release -p cellos-kernel --features board-aarch64-vm \
    --target aarch64-unknown-none-softfloat -Z build-std=core,alloc
bash scripts/build-aarch64-image.sh
bash tools/apple-host/run-hvf.sh   # -M virt,gic-version=3,highmem=off -accel hvf -cpu host
```

- **Interrupt controller:** GICv3; distributor and redistributors are read from QEMU's device tree.
- **Console:** the `virt` PL011 on stdio. **Devices:** virtio-mmio (`virtio-*-device`), not virtio-pci.
- **vCPUs:** up to 16, started through PSCI.
- **DMA isolation:** `Optional`; Cells inside the guest are not DMA-isolated from each other.
- **Virtualization.framework / vfkit is not supported:** its RAM base, console (virtio-console) and device transport (virtio-PCI) differ from the QEMU `virt` map this kernel is linked for. `tools/apple-host/run-vz.sh` exits with that explanation instead of launching.
- The same profile boots under QEMU TCG on any host: `LANE=virt-gicv3 bash scripts/qemu-aarch64-test.sh <kernel>.Image`.
