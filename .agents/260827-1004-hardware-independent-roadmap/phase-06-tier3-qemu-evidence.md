---
phase: 6
title: "Build Tier 3 Hostile QEMU Runners"
status: blocked
priority: P2
effort: "4d"
dependencies: [1]
tier: thinking
---

# Phase 06: Build Tier 3 Hostile QEMU Evidence

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `.agents/260821-0642-app-tiers-completion/phase-04-tier3-qualification.md`
- `docs/roadmap/open-risk-register.md`
- `docs/guides/tier3b-linux-vm.md`

## Overview

Build architecture-aware hostile-input matrices, guest probes, and QEMU runners against current production paths without editing VMM/VirtIO production files.

## Key Insights

QEMU can close machinery and hostile-input regressions; it cannot validate nested-virtualization fidelity or physical containment.

## Requirements

- Preserve pinned Alpine and QEMU-TCG 10.2.0 strict x86 path.
- Cover guest-memory bounds, malformed VirtIO, reset, vCPU budget, and supervisor restart.
- Keep persistence and x86 parity implementation in Phases 09/10; expose reusable scenarios without claiming their results early.
- Treat QEMU-TCG 8.2.2 incompatibility as environment risk, not runtime PASS.

## Architecture

`host runner → hostile guest/probe input → current VMM/VirtIO production path → observed denial/reset/recovery markers`. Phase 06 owns the runner and input corpus only.

## Assumptions

- **Claim:** QEMU-TCG 10.2.0 is available for strict x86.
  **Confidence:** high
  **How to verify:** use existing `QEMU_X86_BIN` version check.
- **Claim:** AArch64 QEMU covers the same logical cases without EL2 physical fidelity.
  **Confidence:** medium
  **How to verify:** inventory backend parity before sharing scenarios.

## Related Files

- Do not modify: `cells/services/hypervisor/src/vmm.rs`, `virtio_blk.rs`, `virtio_net.rs`
- Modify/create: focused hypervisor runners, hostile guest payloads, scenario matrix, strict log parser
- Emit: environment-specific evidence for Phase 08; do not edit the acceptance ledger

## Implementation Steps

1. Build an architecture-by-scenario matrix from strict boot evidence.
2. Create deterministic hostile descriptor/address/reset/budget inputs outside production VMM files.
3. Drive supported scenarios through current QEMU production paths.
4. Verify markers, timeouts, host survival, and stale-state cleanup with a strict parser.
5. Publish reusable persistence/parity scenarios for Phases 09/10 without recording premature PASS.

## Todo List

- [x] Approve the architecture-by-scenario matrix.
- [ ] Implement malformed GPA/descriptor/backend guest inputs; runners and
  strict result parsing are implemented.
- [x] Keep every VMM/VirtIO production file under Phase 09/10 ownership.

## Success Criteria

- [x] Strict x86 reaches `/bin/sh` before applicable fault scenarios.
- [ ] Malformed guest inputs cause no host panic or cross-guest/service corruption.
- [ ] Reset/restart and vCPU-budget runner behavior are deterministic.
- [x] No persistence, x86 parity, or physical qualification claim is inferred before owning phases pass.

## Security Considerations

Every guest-controlled GPA, length, descriptor, queue index, and DMA-like range is hostile. vCPU budget must be observable.

## Risk Assessment

TCG may not model nested virtualization or timing faithfully. Such cases remain blocked rather than softened.

## Next Steps

Run current non-persistence scenarios immediately. Phases 09/10 reuse the same runners for persistence and x86 parity evidence.

## Deviation Log
- Attempted the QEMU-TCG 10.2.0 x86 runner with a `repack-initramfs.py`-overlaid Alpine PVH guest probe. It reaches the probe and host-observes outer-QEMU liveness after a CPU-bound budget stimulus. The guest reset stimulus produces neither nested-VMM exit nor supervisor restart. Bounds, descriptor, and backend inputs have no VMM/VirtIO transport; no independent VMM-preemption outcome exists.
- Attempted the ARM64 QEMU runner. It confirms VMM liveness before the known TCG address-size fault prevents payload execution; this is `BLOCKED_ENVIRONMENT`, not hostile-runner evidence.
- Correction: the earlier x86 parser accepted guest-authored classifications and markers as PASS. The runner now reserves PASS entirely, reports actual started/host-observed budget and reset stimuli separately, and emits `BLOCKED_SCOPE` until every axis has an independent VMM/VirtIO outcome.
- Re-ran strict x86 on 2026-08-28 with pinned QEMU-TCG 10.2.0. The hostile
  probe started, outer-QEMU liveness remained observable after the CPU-bound
  stimulus, and reset still produced no nested-VMM exit or supervisor restart.
  Result remains `BLOCKED_SCOPE`; no guest-memory, descriptor, or backend axis
  has a production transport before Phases 09/10.
- Rebuilt and re-ran ARM64 on 2026-08-28. The VMM reached its liveness marker,
  then the known TCG address-size fault prevented hostile payload execution.
  Result is `BLOCKED_ENVIRONMENT`, not PASS. No production VMM/VirtIO file was
  changed during either run.
- Review correction: ARM64's required scenarios are blocked by its current TCG
  environment, not inapplicable. The ARM64 runner now classifies the known fault
  as `BLOCKED_ENVIRONMENT` and has a fail-closed userspace-probe branch for
  environments where the fault is absent. The malformed GPA, descriptor, and
  backend corpus remains open; guest-authored `NOT_APPLICABLE` markers are not
  accepted as scenario PASS.
