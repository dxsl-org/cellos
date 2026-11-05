# x86 GPU compute — vendor and platform evidence

**Date:** 2026-11-04. Input to [ADR-0033](../../../docs/decisions/0033-x86-intel-igpu-native-compute.md) and the [plan](../plan.md). Tags: `[verified: source]` read on this date; `[inference]` otherwise.

## 1. Why native Intel first, and no guest passthrough first

- **Passthrough in Cellos depends on Intel VMX P09.** It is `pending` and locked by CELL-CONTRACT-0024 (`.agents/260711-1917-tier3b-x86-vtx/plan.md:4-13`) `[verified: repo]`.
- **The Tier-3 guest is a PVH MVP** (`acpi=off nolapic noapic`, 8259 + PIT only). A Linux GPU driver needs PCI, ACPI, LAPIC and MSI in the guest `[verified: repo]`.
- **Device assignment is missing**: virtual config space, BAR→EPT, MSI-X forwarding, FLR. That is a separate program `[inference]`.
- **A native driver reuses work that is already planned:** x86 breadth 04a (PCIe function ABI, MSI-X), 04b (IRQ-driven PCIe Cells) and 12 (VT-d multi-unit, RMRR, interrupt remapping) `[verified: plan]`.
- **Decision (owner, 2026-11-04):** native, iGPU first. NVIDIA's route is decided later.

## 2. Intel stack — nothing on the path is closed except signed firmware

| Item | Fact | Source |
|---|---|---|
| Kernel driver `xe` | `SPDX-License-Identifier: MIT` (`xe_guc.c`) | `[verified: torvalds/linux master]` |
| uAPI `xe_drm.h` | MIT | `[verified]` |
| Officially supported by `xe` (no `require_force_probe`) | Lunar Lake (`lnl_desc`), Battlemage (`bmg_desc`), Panther Lake (`ptl_desc`), Wildcat Lake (`INTEL_WCL_IDS` → `ptl_desc`), Nova Lake-S | `[verified: xe_pci.c:399-541]` |
| Force-probe only in `xe` (Linux defaults to `i915`) | TGL, RKL, ADL-S/P/N (includes N100/N150), DG1, DG2/Alchemist, MTL/ARL | `[verified: xe_pci.c]` |
| Public PRMs | Up to Alchemist/ATS-M, DG1, TGL, RKL; no Xe2/Xe3 PRM found | `[verified: intel.com graphics-for-linux PRM index; community thread]` |
| Firmware | `xe/{lnl,ptl,bmg}_guc_70.bin` (GuC API 70.72.1), HuC, GSC; "Redistributable" | `[verified: linux-firmware WHENCE]` |
| Firmware licence (`LICENSES/LICENSE.xe`) | See below | `[verified]` |
| Vulkan userspace | Mesa `anv`, MIT (`anv_device.c`), compiler in-tree (no LLVM) | `[verified: anv_device.c header]` / `[inference: no-LLVM]` |

`LICENSE.xe` allows redistribution and use in binary form, unmodified, provided the copyright notice and disclaimer are reproduced. It **forbids reverse engineering, decompilation and disassembly**. Its patent grant covers use alone or "in combination with an operating system licensed under an approved Open Source license". Cellos's Apache-2.0 / MIT / MPL-2.0 licences are OSI-approved. The copyright licence has no Linux-only restriction.

**Consequence.** The documentation for Xe2/Xe3 is Intel's own MIT driver and Mesa source, maintained by Intel upstream. The remaining blocking risks:

- GuC ABI changes between firmware versions. Mitigation: pin one firmware version, as `xe` does with `API 70.x`.
- The firmware licence may change. The current text is recorded here.

## 3. Platform choice for "iGPU first"

- **Recommended:** a mini PC on an `xe`-official platform: Panther Lake, Wildcat Lake or Lunar Lake. It should also carry an admitted NIC (I219/I226, breadth phases 13/15), so one machine serves both the HCL row and the GPU work.
- **Allowed with recorded risk:** ADL-N (N100/N150), MTL, ARL. Their `xe` support is force-probe; the mature reference is `i915`, which is larger and older.
- The iGPU uses system memory: no VRAM/LMEM, no resizable BAR. Its stolen memory and the graphics DMAR unit are described by ACPI DMAR (dedicated DRHD + RMRR), so VT-d breadth phase 12 is a hard prerequisite `[inference: confirm on the machine's DMAR]`.
- Compute-only needs GuC. HuC/GSC serve media and content protection `[inference: confirm in G2 from xe init order]`.

## 4. No virtual test target

QEMU has no Intel GPU model, so driver evidence is host tests (fake MMIO, recorded traces) plus the physical machine. A faster loop to evaluate in G2 `[inference]`: Cellos as a KVM guest on the same machine, given an iGPU SR-IOV VF through VFIO, where `xe` supports VFs on that platform.

## 5. Other vendors (not in this plan)

- **NVIDIA:** CUDA userspace is closed, and GSP firmware ABI changes per release. The open alternative is NVK (Vulkan). The route is decided later by the owner: native NVK, passthrough after VMX P09, or a host model server.
- **AMD dGPU:** `amdgpu` is mostly MIT and RADV is MIT, but the PSP/SMU/MES firmware load is heavy. It stays parked; an AMD card on an Intel host is a separate decision from AMD CPUs.
