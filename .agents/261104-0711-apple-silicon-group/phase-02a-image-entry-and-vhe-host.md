---
phase: 2a
title: "Linux arm64 Image entry and VHE host regime"
status: completed   # 2026-11-05, slice A1, QEMU 8.2.2 TCG ceiling; checkpoint approved (phase-02a-checkpoint-approval.md)
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

- [x] HostRegime + detection
- [x] VHE host
- [x] VHE vCPU switch
- [x] Image header + lane
- [x] Regression matrix + `.text` diff report ([evidence](reports/phase-02a-evidence.md))

## Success Criteria

- [x] `virt-vhe` lane: shell prompt, Cell syscalls at EL0, `QEMU_SMP=2`, the hypervisor lane at the same result as non-VHE. ([evidence §2–3](reports/phase-02a-evidence.md), QEMU 8.2.2 TCG)
- [x] `-kernel Image` boot reaches the same banner as the ELF path. ([evidence §2](reports/phase-02a-evidence.md))
- [x] Group-1 builds within ±1% `.text`; `raspi3b` boot log matches baseline. ([evidence §1, §4](reports/phase-02a-evidence.md))

## Risk Assessment

- The vCPU switch is where VHE bugs hide (wrong bank saved). Mitigation: host-side register-bank tests plus the existing hypervisor lane under both regimes.

## Security Considerations

Under VHE, EL0 and the kernel share the EL2&0 regime. Confirm that PAN, UAO and `TTBR1_EL2`/`TTBR0_EL2` separation give Cells the same isolation as the current EL1-mirror arrangement.

## Next Steps

02b and 02c.

## Deviation Log

**D1 — Deviation (selector, owner-delegated via Main 2026-11-05).**
- What: the `virt` VHE variant is selected by the build-time variable `CELLOS_AARCH64_HOST_REGIME` (absent or `nonvhe`, or `vhe`; any other value fails the build), which `backend/qemu_virt.rs` reads with `option_env!`. A Pi build that sets it fails too.
- Why: the convention is a cargo feature, but the feature lines sit in three `Cargo.toml` files outside A1's file set.
- Reversal: add `virt-vhe = []` to `hal/arch/arm/Cargo.toml`, `virt-vhe = ["hal-arm?/virt-vhe"]` to `hal/core/Cargo.toml` and `virt-vhe = ["hal/virt-vhe"]` to `kernel/Cargo.toml`. Then set `VIRT_HOST_REGIME` from `cfg!(feature = "virt-vhe")` and drop the variable from `ci.yml` and the lane docs.

**D2 — Deviation (file set, approved by Main 2026-11-05).**
- What: `scripts/qemu-hypervisor-smoke.sh` gains `HV_SMOKE_CPU` (default `cortex-a72`).
- Why: step 3 runs the hypervisor lane under VHE, and cortex-a72 has no FEAT_VHE. The script is outside the `qemu-aarch64-test*.sh` pattern.
- Effect: the default run is unchanged, and the before and after cortex-a72 runs both pass (evidence §3).

**D3 — Decision (lane shape).**
- What: `virt-vhe` is `LANE=virt-vhe` in `scripts/qemu-aarch64-test.sh` (`virt,virtualization=on,gic-version=2`, cortex-a76, 2 CPUs), not a second script. The raw `Image` comes from the new `scripts/build-aarch64-image.sh`.
- Why: one boot script, with the existing `virt` lane as the default.
- Note: `-cpu max` also passes the lane. cortex-a76 is the default because it has VHE without SVE/SME, which a guest would trap on in either regime.

**D4 — Surprise (VHE PAN inheritance), fixed.**
- What: the first VHE hypervisor run stalled after the guest's "applying system-wide alternatives". A QEMU monitor sample read `PSTATE=004003c9` at EL2h, with PC in `VBAR_EL2`'s current-EL slot.
- Cause: with `SCTLR_EL2.SPAN=1`, an exception to EL2 keeps the guest's `PSTATE.PAN`, so the host faulted on EL0-accessible memory.
- Fix: every lower-EL vector now clears PAN under VHE (`el2.rs`). Non-VHE EL2 has no PAN, so it is unaffected.

**D5 — Surprise (non-VHE EL2 multi-hart reachable through `Image`), fixed in `boot.rs`.**
- What: with an FDT in x0, `virtualization=on` now declares PSCI over SMC, so a non-VHE EL2 kernel starts hart 1 at EL2. The ELF path never passed an FDT and stayed single-hart.
- Defect: `_secondary_entry` only programmed the EL1&0 regime, so the core ran uncached at EL2 and boot hung.
- Fix: the non-VHE EL2 secondary now installs `HCR_EL2`/`MAIR_EL2`/`TCR_EL2`/`TTBR0_EL2`/`SCTLR_EL2` like the boot core. Result: `hart 1 online`, `SMP-IPI: PASS`, prompt.
- Open risk outside the file set (`kernel/src/task/smp.rs`): the secondary start "deadline" counts `system_ticks`, which do not advance while boot runs with IRQs masked. A core that never comes online therefore hangs boot instead of degrading to single-hart.

**D6 — Surprise (tooling ceiling).**
- What: the local QEMU is 8.2.2, not 10.2, so no FEAT_E2H0 model is available.
- Effect: `CPU forces E2H, backend requires non-VHE` is covered only by the compile-time verdict asserts in `backend/mod.rs`. The opposite stop, `CPU lacks VHE, backend requires E2H`, was observed on cortex-a57.
- Host-side register-bank tests were not possible either: the shared core modules are `cfg(target_arch = "aarch64")`-gated in `hal/arch/arm/src/aarch64.rs`, outside the set. The bank switch is evidenced by the hypervisor lane under both regimes.

**D7 — Surprise (measurement).**
- What: embedding the assembled `kernel_fs.img` instead of the stub moves virt `.text` by 3.8%.
- Effect: the ±1% gate is only meaningful with identical embedded inputs on both sides, which is how the evidence was built.
- Also found: the local development-Silo test-hooks lane panics identically on base `6fee21927` and on this slice (`kernel trap ec=0x25 … far=0x100000`). That failure predates the slice and is not addressed here.
