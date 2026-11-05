---
phase: 2a
title: "Linux arm64 Image entry and VHE host regime"
status: pending
priority: P2
effort: "6d"
dependencies: ["0"]
tier: thinking
checkpoint: true
---

# Phase 02a: Linux Image entry and VHE host regime (AS-2)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [Research §3–4](research/asahi-linux-reference.md); Linux `Documentation/arch/arm64/booting.rst` (the Image header is a documented boot protocol)
- `hal/arch/arm/src/aarch64/el2.rs:47-64,171-182`, `vcpu.rs:568-597,721-730`, `trap.rs:329-371`, `boot.rs:148-170`

## Overview

Two shared-core changes that both Apple tracks need:
1. An arm64 Image header and entry (`x0` = FDT physical address), so m1n1 and `VZLinuxBootLoader` can load Cellos.
2. A VHE host regime (`HCR_EL2.E2H=1, TGE=1`, EL0 translated by `TTBR0_EL2`), because Apple CPUs cannot clear `E2H`.

## Key Insights

- Today the host runs non-VHE at EL2 and mirrors MAIR/TTBR0/TCR into EL1 for EL0 Cells. Under VHE the EL2&0 regime applies, so the mirror is unnecessary. The vCPU world switch must save/restore the EL1 bank through the `_EL12` encodings.
- A CPU without FEAT_E2H0 ignores a write of `E2H=0`. Detect it (read back `HCR_EL2` / `ID_AA64MMFR4_EL1`) and fail loudly if the backend demands non-VHE.
- The Image header is 64 bytes at the image start: a branch, `text_offset`, `image_size`, flags (endianness, page size, physical placement), magic `ARM\x64`.

## Requirements

- Functional:
  - `Board::HOST_REGIME` (`El2NonVhe` | `El2Vhe` | `El1`), chosen per backend at compile time; `qemu-virt` gets a VHE variant for testing.
  - Runtime check: CPU forces VHE ⇒ a non-VHE backend stops with `[aarch64] host regime: CPU forces E2H, backend requires non-VHE`.
  - vCPU entry/exit correct under VHE (guest EL1 bank through `_EL12`).
  - Image header and a raw `Image` artifact next to the ELF; FDT from `x0`, with existing paths unchanged.
- Non-functional: group-1 `.text` within ±1%; no change to RPi entry or behaviour.

## Architecture

Static dispatch as in the split plan: `pub type Board = backend::<selected>::Board`. The VHE choice is an associated const, so non-VHE builds contain no VHE code.

## Assumptions

- **Claim:** QEMU `-cpu max` implements VHE, and a QEMU with FEAT_E2H0 modelling can present a CPU without E2H0 (Q4). **Confidence:** medium. **How to verify:** run with the local QEMU 10.2 and read back `HCR_EL2`.

## Related Code Files

- Modify: `hal/arch/arm/src/aarch64/{boot,el2,trap,trap_el2,vcpu,paging}.rs`, `backend/{mod,qemu_virt}.rs`, `kernel/linker-aarch64.ld`, aarch64 image build step, `scripts/qemu-aarch64-test*.sh`, CI aarch64 job.

## Implementation Steps

1. Add the `HostRegime` const and runtime detection; keep non-VHE as the default for every existing backend.
2. VHE host bring-up: `HCR_EL2 = E2H|TGE|RW`, EL2&0 TTBR/TCR, no EL1 mirror.
3. VHE vCPU world switch using `_EL12` accessors; run the existing hypervisor lane under VHE.
4. Image header + raw `Image` output; QEMU lane booting `-kernel Image` with the generated DTB.
5. Lanes: `virt` (non-VHE, unchanged), `virt-vhe` (new), `raspi3b`, RPi4 compile.

## Todo List

- [ ] HostRegime + detection
- [ ] VHE host
- [ ] VHE vCPU switch
- [ ] Image header + lane
- [ ] Regression matrix + `.text` diff report

## Success Criteria

- [ ] `virt-vhe` lane: shell prompt, Cell syscalls at EL0, `QEMU_SMP=2`, the hypervisor lane at the same result as non-VHE.
- [ ] `-kernel Image` boot reaches the same banner as the ELF path.
- [ ] Group-1 builds within ±1% `.text`; `raspi3b` boot log matches baseline.

## Risk Assessment

- The vCPU switch is where VHE bugs hide (wrong bank saved). Mitigation: host-side register-bank tests plus the existing hypervisor lane under both regimes.

## Security Considerations

Under VHE, EL0 and the kernel share the EL2&0 regime. Confirm that PAN, UAO and `TTBR1_EL2`/`TTBR0_EL2` separation give Cells the same isolation as the current EL1-mirror arrangement.

## Next Steps

02b and 02c.

## Deviation Log

None.
