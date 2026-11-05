# ADR-0033 — Native GPU compute on Intel x86-64, integrated GPU first

**Date:** 2026-11-04
**Status:** Accepted for direction. Owner decisions in the planning conversation of 2026-11-04:
- Pay the cost of a native driver rather than starting with guest passthrough.
- Integrated GPU first ("Đồng ý. Chọn iGPU trước.").
- Decide NVIDIA's route later ("NVIDIA sẽ chọn hướng đi sau.").

It amends [ADR-0022](0022-intel-x86-64-c2c-only-direction.md) §Paused scope and [ADR-0031](0031-x86-platform-breadth-kvm-vmware-intel.md) §2.
**Decider:** Cellos maintainer (owner).
**Implementation authority:** direction and scheduling only. Each slice still needs its CELL-CONTRACT-0024 lock handoff, and Law-1 CP1/CP2 for any ABI change. No hardware purchase is approved.
**Plan:** [`.agents/261104-0801-x86-igpu-compute/plan.md`](../../.agents/261104-0801-x86-igpu-compute/plan.md). **Evidence:** [vendor and platform evidence](../../.agents/261104-0801-x86-igpu-compute/research/vendor-and-platform-evidence.md).

## Context

- ADR-0022 parks AI/NPU/GPU programs, and ADR-0031 lists GPU drivers as out of scope. Inference today is CPU-only (`libs/ai-engine`, `cells/services/ai`). Haily, the planned flagship consumer, needs local models.
- Guest passthrough would let a Linux guest drive any GPU. In Cellos it waits on Intel VMX P09 (pending, locked), on a guest platform beyond the current PVH MVP, and on device assignment. It would also teach nothing about driving the GPU from Cellos.
- Intel's stack is open (verified 2026-11-04):
  - the `xe` kernel driver and the `xe_drm.h` uAPI are MIT;
  - Mesa `anv` is MIT;
  - `xe` officially supports Lunar Lake, Battlemage, Panther Lake and Wildcat Lake;
  - GuC/HuC/GSC firmware is redistributable in binary form, with a no-reverse-engineering clause.

  No Xe2/Xe3 PRM is published, so the MIT driver and Mesa sources are the documentation. NVIDIA's compute userspace (CUDA) is closed.

## Decision

1. **Native, compute-only Intel GPU support is admitted under the Intel direction.**
   - Consumer: the `ai` service and Haily's local inference on the Intel node.
   - The work is headless: no display, media, power/frequency tuning or 3D beyond what compute needs.
2. **Integrated GPU first.**
   - One exact machine: an `xe`-official platform (Panther Lake, Wildcat Lake or Lunar Lake) is recommended, with an admitted NIC so the same unit can be the HCL machine.
   - A force-probe platform (ADL-N/N100, MTL, ARL) needs the owner to accept that risk at procurement.
   - Discrete Intel Arc (Battlemage) is a later phase. It adds VRAM, BAR and LMEM management.
3. **Architecture.**
   - Driver: a GPU driver Cell modelled on `xe` (MIT; attribution in `NOTICE` where code is adapted).
   - **No new GPU API at any layer** (owner, 2026-11-04: "Không nên sinh ra api mới"):
     - Applications, including `ai` and Haily, use **Vulkan (compute)** only.
     - The driver Cell serves the **existing Linux `xe` uAPI** (`xe_drm.h`, MIT): the same requests and structures, carried over Cellos IPC instead of `ioctl`.
     - Mesa `anv` runs as a Tier-2 C Cell with a transport shim only; llama.cpp's Vulkan backend (or the `ai` engine) sits on top.
   - The only additions are kernel authority plumbing for Law-1 review (mapping client buffers into the device IOMMU domain, revocation on client death, completion waits), plus a service registry entry.
   - A later vendor brings its own existing uAPI with its driver Cell and Mesa Vulkan driver (RADV, NVK, Honeykrisp). Vulkan applications do not change.
4. **DMA isolation stays `Required`.** The iGPU runs behind its VT-d unit, with RMRR handled by x86 breadth phase 12. A missing or failing remapper refuses the device.
5. **Firmware.**
   - Pin one GuC version, and record its hash and the `LICENSE.xe` text.
   - Ship the binary unmodified with its notice.
   - Nobody disassembles it; this matches the licence and the ADR-0032 provenance rule.
6. **Scheduling.**
   - Intel single-WIP rule. GPU slices queue with the x86 breadth slices in an owner-chosen order.
   - The driver phase (G2) cannot start before breadth 04a, 04b and 12.
   - G0 and G1's `xe` uAPI subset and kernel ABI inventory are documents and can be prepared without the WIP slot.
7. **Not decided here.**
   - NVIDIA: the options are native NVK, passthrough after VMX P09, or a host model server. Owner decision later.
   - AMD dGPU stays parked.
   - The Apple Silicon lane (ADR-0032) is unchanged.

## Considered options

| Option | Outcome | Reason |
|---|---|---|
| Guest passthrough first, native later | Rejected | Blocked on VMX P09, a full guest platform and device assignment; produces no Cellos GPU knowledge |
| Native Intel discrete (Arc) first | Deferred | Higher performance, but VRAM/LMEM and BAR work on top of the driver; owner chose iGPU |
| Native NVIDIA | Not chosen now | CUDA closed; GSP firmware ABI churn |
| Native Intel iGPU, compute-only | **Chosen** | Fully open stack; matches the Intel direction and the planned HCL mini PC |

## Consequences

- New Law-1 items are expected, limited to kernel authority: device-DMA mapping of another Cell's buffers, its revocation on client death, and completion waits. G1 inventories them for CP1. No GPU API is added.
- No QEMU model exists for Intel GPUs. Evidence ceilings are host tests and the physical machine (`S2`). A KVM guest with an SR-IOV VF is evaluated as a faster loop.
- Images that ship GuC firmware carry `LICENSE.xe` in `NOTICE`/`LICENSE` material.
- Results are compared with Linux on the same machine (same model, prompts and backend). An unfavourable result is reported.

## Related

- [ADR-0022](0022-intel-x86-64-c2c-only-direction.md), [ADR-0024](0024-cell-kernel-authority-contract.md), [ADR-0029](0029-platform-image-families-and-runtime-detection.md), [ADR-0030](0030-license-model-and-copyleft-boundary.md), [ADR-0031](0031-x86-platform-breadth-kvm-vmware-intel.md), [ADR-0032](0032-apple-silicon-secondary-lane.md)
- [HCL](../hardware-compatibility-list.md), [Haily roadmap](../roadmap/haily-personal-assistant.md)
