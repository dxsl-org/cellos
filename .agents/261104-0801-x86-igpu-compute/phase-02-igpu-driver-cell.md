---
phase: 2
title: "Intel iGPU driver Cell: bring-up to compute submission"
status: pending
priority: P1
effort: "40d + hardware"
dependencies: ["G1", "breadth-04a", "breadth-04b", "breadth-12"]
tier: thinking
checkpoint: true
---

# Phase G2: Intel iGPU driver Cell

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- Linux `drivers/gpu/drm/xe/**` (MIT) — init order, GuC load/ABI headers, GGTT/PPGTT, exec queues; Intel PRMs where published
- Breadth [04a](../261103-2218-x86-platform-breadth/phase-04a-pcie-function-abi-and-msix.md), [04b](../261103-2218-x86-platform-breadth/phase-04b-irq-driven-pcie-drivers.md), [12](../261103-2218-x86-platform-breadth/phase-12-vtd-multi-unit-rmrr-ir.md)

## Overview

New `cells/drivers/intel-gpu` serving the G1 `xe` uAPI subset for the G0 iGPU:
- probe and MMIO;
- forcewake and power wells needed for the GT;
- GuC firmware load and handshake;
- GGTT (stolen memory) and per-client PPGTT;
- compute engine (CCS or RCS) exec queues through GuC submission;
- MSI-X completion interrupts;
- reset and recovery.

## Key Insights

- First light is a GPU-written value: a batch with a store-data instruction writes a marker into a client buffer, and the client reads it back through the `xe` requests (`VM_BIND`, `EXEC`, sync wait).
- The iGPU's DMA goes through its own VT-d unit, and the stolen-memory RMRR must stay mapped. Breadth phase 12 provides both.
- There is no QEMU model. Host tests use fake MMIO with recorded register sequences; the faster loop to evaluate is a KVM guest with an SR-IOV VF, where `xe` supports it on that platform.

## Requirements

- Exact PCI ID admission (`Target` until captured, then `Driveable`).
- GuC load of the pinned firmware with its version check; refuse other versions.
- Per-client GPU VM; mappings only of client-exported buffers; revocation on client death before frame release.
- Hang detection and engine reset; a hang never wedges the kernel.
- Cell exit leaves the GPU idle and its DMA stopped.

## Related Code Files

- Create: `cells/drivers/intel-gpu/**`, host test fixtures
- Modify: `kernel/src/task/drivers/pcie_ecam.rs` (ID admission), `boards/pc/**`, x86 ISO scripts, CI build list, `NOTICE`

## Implementation Steps

1. Probe, MMIO, forcewake; read GT identity registers (host tests with fakes).
2. GGTT + firmware upload + GuC handshake.
3. PPGTT per client; buffer binding through the `xe` `VM_BIND` request.
4. GuC exec queue on the compute engine; first light.
5. MSI-X completions, fences, cancel; hang/reset.
6. Physical captures.

Pre-hardware progress (2026-11-04): the hardware-independent parts are in `libs/intel-gpu-core` (standalone workspace until this Cell joins the root workspace), with 50 host tests:
- `guc_fw` (CSS header + exact pin, checked against the real `ptl_guc_70.bin`);
- `pte`/`ppgtt` (Xe2/Xe3 entries, 5-level PTL walk);
- `ct` (GuC CT headers and rings);
- `syncobj` (DRM sync objects).

## Todo List

- [x] Probe/forcewake
- [x] GuC up
- [ ] PPGTT
- [ ] First light
- [ ] Fences/reset
- [ ] Physical capture

## Success Criteria

- [ ] S2: first-light marker written by the GPU and read back by a client Cell; 10,000 submits with fences without loss; an injected hang is reset and the client gets an error.
- [ ] A client Cell killed mid-submit leaves no GPU mapping to its freed frames (host test + capture).
- [ ] Machine boots unchanged when the driver Cell is absent.

## Risk Assessment

- GuC ABI or init-order mismatch with the pinned firmware. Mitigation: mirror `xe`'s version handling exactly for that firmware.
- Power management of the iGPU (RC6, forcewake) differs per platform. Keep RC6 disabled first.

## Security Considerations

A GPU that bypasses its VT-d unit could read all memory. The driver must refuse to start if its graphics DRHD is not translating.

## Next Steps

G3.

## Deviation Log

None.
