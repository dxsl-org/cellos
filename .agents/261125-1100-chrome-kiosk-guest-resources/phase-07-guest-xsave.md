---
phase: 7
title: "AVX2 for the guest"
status: completed
priority: P2
effort: "3-5d"
dependencies: [3]
tier: thinking
checkpoint: true
---

# Phase 7: AVX2 for the guest

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Chromium draws on the CPU here (`--disable-gpu`), and the guest sees a CPU without XSAVE or
AVX: its pixel loops run their SSE paths. Until a GPU path exists (phase 12), wider vectors are
the rendering speed-up within reach.

## Requirements

- Functional: the guest sees XSAVE, OSXSAVE (when it sets CR4), AVX and AVX2, and CPUID leaf
  0Dh for x87, SSE and AVX state only. AVX-512, AMX and other components stay hidden.
- Functional: `XSETBV` exits are handled: XCR0 must be a valid subset of {x87, SSE, AVX}, else
  #GP. Guest XCR0 is loaded on entry and the host's restored on exit.
- Functional: guest extended state is saved and restored with `XSAVE`/`XRSTOR` per vCPU in an
  area sized from the host's CPUID; host state is preserved as today
  (`world_switch/host_ext_state.rs`).
- Non-functional: a host without AVX2 leaves the guest as it is today. SVM is unchanged.

## Architecture

`VmxGuestRegs` grows an XSAVE area and the guest XCR0; the world switch swaps XCR0 around the
guest FPU save/restore it already does with FXSAVE.

## Assumptions

- **Claim:** the CellOS kernel enables XSAVE/AVX for itself or can save its own state around a
  guest that uses YMM. **Confidence:** low. **Verify:** `world_switch/host_ext_state.rs`,
  `hal/arch/x86/src/x86_64/` FPU setup; if the host kernel runs without OSXSAVE, this phase
  first has to enable it for the host, which touches every x86 context switch (re-plan then).
- **Claim:** Chromium's Skia picks AVX2 paths at run time from CPUID. **Confidence:** high.
  **Verify:** bench `paint` and `scroll` before and after.

## Related Files

- Modify: `hal/arch/x86/src/x86_64/guest_cpuid.rs`, `world_switch.rs`,
  `world_switch/guest_fpu.rs`, `vmx_vcpu.rs` (XSETBV exit), `vmx_vcpu/cr_access.rs` (OSXSAVE)
- Extend: `tests/guests/x86-gpu-pattern/fpu-check.S` (YMM registers survive VM exits), display lane

## Implementation Steps

1. Verify the host-state assumption; stop and re-plan if the host lacks XSAVE support.
2. Guest XCR0, XSETBV, XSAVE area; CPUID.
3. YMM survival check in the display lane; bench; lanes.

## Success Criteria

- [x] The guest's `/proc/cpuinfo` flags include `avx2` (marker in the serial log).
      (`CHROME_GUEST_SIMD avx=1 avx2=1`, lane check `guest-avx2`.)
- [x] The display lane's FPU check covers YMM0-YMM15 across VM exits and passes.
      (`guest-ymm-intact` for both display-lane guests.)
- [x] Bench `paint` and `scroll` before and after; Chrome lane and x86 Tier 3 CI jobs pass.
      (`evidence/phase07-avx.md`; lanes and CI on 2026-11-26 after the last change.)

## Security Considerations

Extended state must not leak between guest and host or between vCPUs: each area is zeroed at
creation, and the host's XCR0 is restored before any host code that could use it runs.

## Risk Notes

A wrong XCR0 on the host side corrupts host FPU state silently; the YMM check runs in both
directions (guest values survive, a host task's values survive a guest run).

## Deviation Log

- **Step 1 (2026-11-26):** the host ran without `CR4.OSXSAVE` (nothing set it; host context
  switches and the guest FPU image use FXSAVE). Not a re-plan after all: `init_sse` (BSP and
  every AP) now sets `CR4.OSXSAVE` with XCR0 = x87 | SSE when the CPU has XSAVE. Host code
  still cannot use AVX (#UD) and keeps FXSAVE, so no context switch changes.
- **Decision:** XCR0 and the guest's x87/SSE/AVX state are switched once per `RunVcpu`, not
  around each entry (`VmxVcpu::run`): `XSETBV` traps to an outer hypervisor, and the kernel
  between entries uses no x87/SSE/AVX (an interrupt there saves and restores x87/SSE). The
  per-entry FXSAVE/FXRSTOR of the VMX path is gone with it (SVM keeps it). The image is one
  832-byte standard-format area: FXSAVE/FXRSTOR for the legacy region whatever the guest's
  XCR0, XSAVE/XRSTOR with feature mask AVX for the upper YMM halves; `VZEROUPPER` after the
  save.
- **Decision:** CPUID for a VMX guest (`vmx_vcpu/xsave.rs`): XSAVE and AVX shown, OSXSAVE
  follows the guest's CR4, AVX-512 and AMX bits hidden, leaf 0Dh built from the
  architectural layout for components 0-2 (XSAVEOPT only; no XSAVEC, XGETBV1, XSAVES).
  `XSETBV`: #UD without OSXSAVE, #GP(0) at CPL > 0, ECX ≠ 0 or an XCR0 outside x87/SSE/AVX
  or AVX without SSE. SVM unchanged.
- **Decision:** the per-entry XCR0 snapshot in `host_ext_state.rs` is gone: both backends
  intercept `XSETBV`, so a guest never writes XCR0 and the snapshot guarded nothing.
- **Surprise (measured, `evidence/phase07-avx.md`):** every `RunVcpu` call costs 30-40 µs
  more outside the guest once the host runs with `CR4.OSXSAVE`, with or without AVX for the
  guest. Not the per-entry `XGETBV` (removed, no change); a host XCR0 equal to the outer
  host's gave 103 vs 115 µs in one run. Cause not identified; it is paid in the outer
  hypervisors of the development machine (KVM under Hyper-V). The gain: paint frames +4-9 %,
  scroll +2-4 %, JavaScript unchanged or slower by up to 7 %. Kept on: the platform target
  is physical Intel hardware, where nested costs do not apply [INFERENCE, not measured
  there]. Owner to confirm at CP2.
