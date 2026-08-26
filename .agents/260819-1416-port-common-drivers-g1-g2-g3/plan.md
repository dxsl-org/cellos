---
title: "Common Driver Port Plan for G1-G3"
description: "Approved implementation plan and execution ledger for shared Cellos drivers across G1 robot SBCs, G2 server/PC targets, and a hardware-gated G3 accelerator envelope."
status: blocked
priority: P2
effort: 38d
branch: feat/common-drivers-g1-g2-g3
tags: [feature, drivers, hardware, g1, g2, g3]
blockedBy: []
blocks: []
created: 2026-08-19
---

# Common Driver Port Plan for G1-G3

## Scope Contract

Deliverable is the approved implementation plan plus execution ledger for the shared-driver slice. It covers common shared drivers only: board/SoC selection data, safe MMIO/DMA/IRQ substrate, G1 hardware controller path, G1/G2 boot/display/input/storage baseline, G2 PCIe storage/network, and G3 NPU readiness envelope.

Boundaries: preserve `boards -> hal/soc -> hal/arch`; no per-board copies of UART/SDHCI/DesignWare I2C/SPI/GIC/PLIC/VirtIO/PCIe; no claim that QEMU or compile proves physical boards; no G3 `ViAccelerator` freeze before hardware. Implementation progress is tracked separately from hardware promotion gates.

## Evidence Base

- G1/G2/G3 definitions: `docs/project-roadmap.md:169-203`.
- Hardware driver priorities: `docs/specs/04-hardware.md:48-146`.
- Peripheral implementation status: `docs/specs/13-peripherals.md:4-5`, `docs/specs/13-peripherals.md:160-177`.
- Ownership law: `docs/code-standards.md:58-74`.
- Code scout: `./reports/scout-report.md`.
- Reference map: `./research/haily-researcher-02-reference-os-driver-map.md`.

## Driver Capability Matrix

| Stage | Boot-critical | Bring-up | Storage | Input/display | Network | Optional |
|---|---|---|---|---|---|---|
| G1 | UART, timer, IRQ, SDHCI/MMC | GPIO IRQ/ownership, real I2C, real SPI | SDHCI, VirtIO-blk fallback | UART input, VirtIO input/GPU on QEMU | VirtIO-net baseline | bit-bang/sim fallback; PWM, ADC, CAN later |
| G2 | COM1, HPET/APIC, q35 PCIe ECAM, IOMMU | q35 BAR handoff | q35 NVMe, VirtIO-blk fallback | VirtIO-GPU/input, desktop later | q35 e1000 first; RTL8125/i225 research-only | Pioneer blocked on SG2042 substrate |
| G3 | G2 baseline + large-buffer IPC | NPU vendor probe cell | model cache later | telemetry only | inference demo path | RKNN/X390 contract after hardware |

## Phases

| # | Phase | Status | Depends |
|---|---|---|---|
| 01 | [Evidence and Provenance Gate](./phase-01-evidence-and-provenance-gate.md) | completed | none |
| 02 | [Shared Driver Substrate](./phase-02-shared-driver-substrate.md) | completed | 01 |
| 03 | [G1 Hardware Peripheral Controllers](./phase-03-g1-robot-peripheral-drivers.md) | completed | 02 |
| 04 | [Boot Storage Input Display Baseline](./phase-04-boot-storage-input-display-baseline.md) | completed | 02 |
| 05 | [G2 PCIe Storage and Network](./phase-05-g2-pcie-storage-and-network.md) | completed | 02,04 |
| 06 | [G3 Accelerator Readiness Envelope](./phase-06-g3-accelerator-readiness-envelope.md) | blocked | 01,05 |

## Dependency Graph

`01 -> 02 -> {03,04}; 04 -> 05 -> 06`. Phases 03 and 04 may run in parallel only after file-ownership is split; default execution is sequential because both may touch board driver selection.

## Validation Log

- Self-verify: checked stage docs, peripheral spec, board descriptors, driver cells, MMIO/DMA syscalls, scripts, and reference licenses.
- Reference path correction: observed `D:\Cellos\.references`; this is no longer an unresolved assumption.
- User checkpoint: RPi3 BCM is the first real G1 controller lane after the current smoke test; RPi4/VF2 follow only after that evidence slice.
- Execution sync: approved implementation landed for the Phase 03 safe slice, including dedicated I2C/SPI capability bits, exact MMIO allowlists, BCM BSC/SPI controller cores, and RPi3 pinmux wiring.
- Gate status: Phase 04 is complete: live RV64/AArch64 baseline and optional-device omission runs pass; physical RPi3 TFTP, SDHCI/mount, shell, interactive `help`, and 100/100 UART burst pass. Phase 03 wired GPIO/I2C/SPI evidence now passes on the current RPi3 head and stays scoped to the BCM/RPi3 lane; DesignWare remains deferred.
- Gate status: Phase 05 q35 lane is complete: NIC 2/2, NVMe 3/3 with VFS FAT32 roundtrip, `X86_NIC_MODEL=e1000e` fail-closed, and VT-d active; physical x86 remains hardware-gated/deferred, Pioneer stays blocked, RTL8125/i225 stay research-only, and the BAR no_std unit-harness gap is deferred low risk.
- Phase 06 remains blocked as a hardware/SDK evidence envelope only; the repo has no local RKNN/X390 source, no probe crate, and no ABI freeze was started.
- Failed environmental checks: `docs/coding.md`, `docs/engineering-standards.md`, and `.claude/scripts/set-active-plan.cjs` are absent.
- Red-team result: top risk is promoting fallback/prototype drivers or partial G2 DMA/NIC code before real-controller/IOMMU/physical gates; mitigated by Phase 03 and Phase 05 ordering.

## Confirmed Decisions

- G1 first hardware controller proof: RPi3 BCM BSC I2C/SPI after the current RPi3 smoke gate.
- RPi4/VF2 and DesignWare-compatible controllers remain later lanes requiring their own board/DTB evidence.
- RTL8125/i225 remain research-only until q35/e1000 and physical x86 evidence pass.
