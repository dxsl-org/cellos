# Intel x86-64 Hardware Compatibility List (HCL) & Procurement Survey

**Date**: 2026-11-09  
**Status**: Proposal & Procurement Recommendation for Machine A (Mini PC)  
**Authority**: [ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md) (Intel x86-64 C2C Anywhere Direction), [HCL](../hardware-compatibility-list.md), [.agents/261103-2218-x86-platform-breadth/phase-20-physical-qualification-hcl.md](../../.agents/261103-2218-x86-platform-breadth/phase-20-physical-qualification-hcl.md)

---

## 1. Executive Summary & Strategy Alignment

Under [ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md), **Cell-to-Cell (C2C) Anywhere on Intel x86-64 is the sole active program**. All kernel, driver, and platform engineering serves this target.

Before purchasing or deploying physical hardware, the platform breadth plan (Phase 20) mandates:
1. **Virtual Verification Gate ($S_1$)**: Complete qualification across three virtualization environments:
   - **QEMU-TCG** (Software architecture baseline, no host acceleration dependencies).
   - **QEMU-KVM** (Hardware acceleration, Invariant TSC, real CPUID, real instruction timing).
   - **VMware Workstation / ESXi** (Enterprise hypervisor compatibility, ACPI, APIC routing, PVSCSI/VMXNET3).
   *Status*: **All three virtualization environments verified ($S_1$ PASS)**.
2. **First-Machine Acquisition Gate (Machine A)**: Select and purchase **one exact Intel mini PC / desktop unit**, complete bare-metal bring-up and log captures ($S_2$ physical development evidence), and obtain first-machine qualification.
3. **Second-Node Acquisition Gate (Machine B)**: Only after Machine A passes all mandatory requirements and is ratified, acquire a second matching node to qualify cross-machine LAN C2C (Noise KKpsk0, SwarmBeacon discovery, and relay failover).

---

## 2. Mandatory HCL Requirements (R1–R9) Compliance Matrix

Every physical candidate must satisfy all nine criteria from [HCL §2](../hardware-compatibility-list.md):

| # | Requirement | Specification | Cellos Driver / Subsystem | Verification Method |
|---|---|---|---|---|
| **R1** | **Console** | 16550 UART (COM1 `0x3F8`/IRQ 4, optional SPCR) OR GOP linear framebuffer + USB HID keyboard | `driver-serial-x86`, `driver-xhci` + `service-input` | Boot to `Cellos >` prompt; interactive input/output |
| **R2** | **Clocksource** | Invariant TSC (Intel 10th-14th Gen) + TSC-Deadline, ACPI HPET, or FADT PM timer | `hal::clock::tsc`, `hal::clock::hpet`, `hal::clock::pm_timer` | `[x86-gate] timer: clock=... clockevent=...` |
| **R3** | **Secure Boot** | Must be switchable to **Disabled** in firmware (no signed x86 EFI path yet) | Limine bootloader | BIOS Setup menu toggle |
| **R4** | **Storage** | M.2 NVMe SSD or SATA AHCI; **Intel VMD MUST be disabled** in BIOS | `driver-nvme`, `driver-ahci` | Mount → write → reboot → readback cycle |
| **R5** | **Bootloader** | UEFI or Legacy BIOS boot from USB media | Limine BIOS/UEFI dual-mode ISO (`make-iso-ci.sh`) | Cold boot from USB drive |
| **R6** | **Network** | Supported Ethernet controller with exact admitted PCIe vendor/device ID | `driver-e1000e` (I219), `driver-igc` (I225/I226), or `driver-igb` (i210/I350) | `lspci -nn` + DHCP / Link-Local RFC 3927 assignment |
| **R7** | **Machine Identity** | Full recording of vendor, model, motherboard, PCH, CPU family, BIOS version & date | `boards/pc/x86_64-pc` descriptor | Machine pin digest (`Init: machine pin: ...`) |
| **R8** | **VT-d & IR** | Intel VT-d (IOMMU) with Interrupt Remapping (INTR_REMAP) active | `kernel::task::drivers::iommu_x86` (VT-d unit, DMAR) | `[vtd] Intel VT-d: DMA isolation ACTIVE` |
| **R9** | **Intel VT-x / EPT** | Hardware virtualization (VMX root) enabled in firmware | `hal::arch::x86_64::vmx` | CPUID feature flags + VMXON initialization |

---

## 3. Comparative Evaluation of Candidate Machines (Desktop / Mini PC)

We evaluated the four most accessible and reliable Intel enterprise mini PC platforms:

| Metric / Requirement | Candidate 1: **Dell OptiPlex 7080 / 7090 Micro** (Recommended) | Candidate 2: **Intel NUC 11 / 12 Pro** | Candidate 3: **HP ProDesk / EliteDesk 600/800 G6 Mini** | Candidate 4: **Lenovo ThinkCentre M70q / M90q Tiny** |
|---|---|---|---|---|
| **Form Factor** | 1-Litre Ultra Small Form Factor (USFF) | NUC Mini PC kit | 1-Litre Desktop Mini (DM) | 1-Litre Tiny Desktop |
| **Processor Generation** | Intel 10th / 11th Gen Core (Comet Lake / Rocket Lake) | Intel 11th / 12th Gen Core (Tiger Lake / Alder Lake) | Intel 10th Gen Core (Comet Lake) | Intel 10th / 11th Gen Core |
| **Recommended CPU** | **Intel Core i5-10500T** (6C/12T, 2.3–3.8 GHz, 35W TDP) | Intel Core i5-1135G7 (4C/8T, 28W) | Intel Core i5-10500T (6C/12T, 35W) | Intel Core i5-10500T (6C/12T, 35W) |
| **Onboard NIC (R6)** | **Intel I219-LM** Gigabit Ethernet (`8086:0d4e` / `8086:15b7`) | Intel I225-V / I226-V 2.5 GbE (`8086:15f3` / `8086:125c`) | Intel I219-LM (`8086:0d4e`) | Intel I219-V / I219-LM (`8086:0d4e`) |
| **Driver Status** | **`driver-e1000e` (Shipped & Qualified)** | `driver-igc` (Shipped & Qualified) | `driver-e1000e` (Shipped & Qualified) | `driver-e1000e` (Shipped & Qualified) |
| **Storage (R4)** | M.2 2280 PCIe NVMe Gen3 x4 (pure AHCI/NVMe mode, VMD disableable) | M.2 PCIe Gen4 NVMe (VMD disableable) | M.2 PCIe Gen3 NVMe (VMD disableable) | M.2 PCIe Gen3 NVMe (VMD disableable) |
| **Firmware / BIOS (R3)** | **Dell UEFI**: Clean disable of Secure Boot & VMD; full VT-x & VT-d control | Intel Visual BIOS / Aptio: full control | HP SureStart BIOS: Secure Boot toggleable | Lenovo BIOS: Secure Boot toggleable |
| **Console Ports (R1)** | Optional rear DB9 COM port punch-out; standard Framebuffer + USB | Front/Rear USB 3.0 / Type-C (Framebuffer only) | Optional serial port punch-out | Optional serial port punch-out |
| **Intel VT-x / EPT (R9)** | Fully supported & enabled in BIOS | Fully supported & enabled in BIOS | Fully supported & enabled in BIOS | Fully supported & enabled in BIOS |
| **Intel VT-d (R8)** | Fully supported (DMAR table present) | Fully supported | Fully supported | Fully supported |
| **Thermal / Noise** | 35W low-power desktop T-series CPU, whisper-quiet fan | Mobile U-series CPU, compact blower fan | 35W T-series CPU | 35W T-series CPU |
| **Estimated Cost** | **$180 – $240 USD** (Refurbished / Enterprise Surplus) | $320 – $450 USD (Kit without RAM/SSD) | $200 – $260 USD | $210 – $270 USD |

---

## 4. Primary Recommendation for Machine A: Dell OptiPlex 7080 Micro

### 4.1. Hardware Specification
- **System**: Dell OptiPlex 7080 Micro (or 7090 Micro)
- **Chipset**: Intel Q470 PCH
- **Processor**: Intel Core i5-10500T (6 Cores, 12 Threads, 12 MB Cache, Invariant TSC, VT-x, VT-d, EPT)
- **Memory**: 16 GB DDR4-2666 / 2933 MHz SODIMM (1x 16 GB or 2x 8 GB)
- **Storage**: 256 GB or 512 GB M.2 NVMe SSD (PCIe Gen3 x4, non-RAID, pure NVMe controller)
- **Networking**: Intel Ethernet Connection I219-LM (PCIe ID `8086:0d4e`, supported by `driver-e1000e`)
- **Graphics**: Integrated Intel UHD Graphics 630 (GOP linear framebuffer compatible)
- **Ports**: 1x RJ-45 GbE, 5x USB 3.2 Gen 1/2 Type-A, 1x USB 3.2 Gen 2 Type-C, 2x DisplayPort 1.4

### 4.2. Firmware (BIOS) Configuration Checklist

Before booting Cellos from the prepared USB installation drive, enter Dell BIOS Setup (press **F2** on power-up) and set:

1. **System Configuration $\rightarrow$ SATA Operation**:
   - Change from `RAID On` to **`AHCI/NVMe`** (*CRITICAL: Disables Intel VMD / RST so NVMe SSD is exposed directly to Cellos `driver-nvme`*).
2. **Security $\rightarrow$ Secure Boot $\rightarrow$ Secure Boot Enable**:
   - Set to **`Disabled`** (*Mandatory for R3*).
3. **Virtualization Support**:
   - **Intel Virtualization Technology (VT-x)**: **`Enabled`** (*Mandatory for R9*).
   - **VT for Direct I/O (VT-d)**: **`Enabled`** (*Mandatory for R8*).
4. **Power Management**:
   - **AC Recovery**: `Power On` (Convenient for remote/bench automation).
5. **General $\rightarrow$ Boot Sequence**:
   - Ensure UEFI Boot is selected, and boot from USB Flash Drive.

---

## 5. Machine A Bring-Up & Capture Verification Plan

Once the physical hardware is available, execute the following capture protocol:

1. **Image Preparation**:
   - Format a USB drive (FAT32) using `scripts/x86/make-iso-ci.sh` to generate the bootable UEFI image `build/cellos-x86_64.iso`.
2. **Boot & Console Capture (R1, R5)**:
   - Boot via USB; verify Limine boots the kernel.
   - Record boot log lines up to `Cellos >` shell prompt.
3. **Clocksource & Timer Capture (R2)**:
   - Verify `[x86-gate] timer: clock=tsc clockevent=tsc-deadline` (or HPET).
4. **Storage Persistence Cycle (R4)**:
   - Execute `vwrite /mnt/sd/test_persist.bin` $\rightarrow$ reboot $\rightarrow$ `vcat /mnt/sd/test_persist.bin` to prove persistent NVMe I/O across restarts.
5. **Wired Networking Capture (R6)**:
   - Verify `driver-e1000e` binds the physical Intel I219-LM NIC.
   - Verify Link-Local RFC 3927 address assignment (`ifconfig` returns `169.254.x.y netmask 255.255.0.0`).
6. **IOMMU & DMA Isolation Capture (R8)**:
   - Verify `[vtd] Intel VT-d: DMA isolation ACTIVE` before any network or disk DMA transaction occurs.
7. **Clean Power Lifecycle**:
   - Execute `/bin/shutdown` to prove ACPI S5 poweroff.

---

## 6. Server Node (Machine B) Qualification Roadmap

Following the physical qualification of Machine A:
- **Target Platform**: 1U/2U Rackmount Server, e.g., **Dell PowerEdge R240 / R340** (Intel Xeon E-2200 series) or **R440 / R640** (Intel Xeon Scalable 2nd Gen).
- **Console Interface**: Integrated Dell Remote Access Controller (**iDRAC9**) with Serial-Over-LAN (SOL) on COM2 (`0x2F8` / IRQ 3), providing headless out-of-band capture.
- **Server NICs**: Broadcom BCM5720 Dual-Port 1GbE LOM (`14e4:165f`, supported by `driver-bcm57xx`) or Intel I350 Quad-Port Gigabit (`8086:1521`, supported by `driver-igb`).
- **Storage**: Dell BOSS-S1 (Boot Optimized Storage Solution) running dual M.2 SATA SSDs in AHCI mode, or PERC H330 in non-RAID HBA mode.
