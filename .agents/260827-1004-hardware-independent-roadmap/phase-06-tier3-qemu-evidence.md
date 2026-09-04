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

Build architecture-aware hostile-input matrices, guest probes, and QEMU runners against current production paths. The x86 corpus may drive a minimal, evidenced repair in VMCB interrupt delivery or normal dispatch when it proves a production delivery fault; it remains QEMU evidence, not nested-virtualization fidelity or physical qualification.

## Key Insights

QEMU can close machinery and hostile-input regressions; it cannot validate nested-virtualization fidelity or physical containment.

## Requirements

- Preserve pinned Alpine and QEMU-TCG 10.2.0 strict x86 path.
- Cover guest-memory bounds, malformed VirtIO, reset, vCPU budget, and supervisor restart.
- Record persistence and x86 parity only through their own passing runners; do
  not infer physical qualification.
- Treat QEMU-TCG 8.2.2 incompatibility as environment risk, not runtime PASS.

## Architecture

`host runner → hostile guest/probe input → current VMM/VirtIO production path → observed denial/reset/recovery markers`. Phase 06 owns the runner and corpus, plus only the narrowly evidenced VMCB/dispatcher repair required to restore that path; persistence and parity retain their dedicated runners.

## Assumptions

- **Claim:** QEMU-TCG 10.2.0 strict is required for x86/ARM64 matrix runs.
  **Confidence:** medium
  **How to verify:** strict version gate in `scripts/qemu-tier3-hostile-runner-{x86,arm64}.sh` requiring `QEMU_VERSION` parse = `10.2.0`.
- **Claim:** AArch64 and x86 cases remain comparable once production transport coverage is demonstrated.
  **Confidence:** medium
  **How to verify:** inventory backend parity before sharing scenarios.

## Related Files

- `hal/arch/x86/src/x86_64/svm_vcpu.rs` — x86 `sti; hlt` shadow consumption and due LAPIC injection
- `cells/services/hypervisor/src/x86-irq-dispatch.rs` — bounded VirtIO/PIT service fairness
- `cells/services/hypervisor/src/{virtio_blk,net_backend}.rs` — stale supervisor-generation quarantine
- `tests/guests/x86-virtio-e2e/hostile-mmio.c` and focused runners — hostile corpus and strict parser
- Emit: environment-specific evidence; do not edit the acceptance ledger

## Implementation Steps

1. Build an architecture-by-scenario matrix from strict boot evidence.
2. Create deterministic hostile descriptor/address/reset/budget inputs outside production VMM files.
3. Drive supported scenarios through current QEMU production paths.
4. Verify markers, timeouts, host survival, and stale-state cleanup with a strict parser.
5. Publish reusable persistence/parity scenarios for Phases 09/10 without recording premature PASS.

## Todo List

- [x] Approve the architecture-by-scenario matrix.
- [x] Exercise malformed GPA/descriptor/backend inputs and independent vCPU
  preemption through the x86 production transport with strict result parsing.
- [x] Repair the proven x86 HLT interrupt-shadow delivery fault and dispatch
  starvation exposed by the corpus.
- [ ] Rerun the same supported hostile axes on ARM64 after an environment reaches
  the guest probe past the known synchronous TCG fault.

## Success Criteria

- [x] Strict x86 reaches `/bin/sh` at 1 GiB before applicable fault scenarios.
- [x] x86 normal two-boot VirtIO block/network persistence and the 27-scenario
  hostile corpus pass under pinned QEMU-TCG 10.2.0.
- [ ] Malformed guest inputs cause no host panic or cross-guest/service corruption
  on every supported architecture; x86 passes and ARM64 remains environment-blocked.
- [ ] Reset/restart and vCPU-budget runner behavior are deterministic on every
  supported architecture; x86 passes and ARM64 remains environment-blocked.
- [x] No physical x86 qualification claim is inferred from QEMU evidence.

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
- Strict matrix gate update: both runners now enforce exact QEMU 10.2.0 and fail fast with `BLOCKED_ENVIRONMENT` when unavailable. Current environment exposes only 8.2.2, so matrix execution is blocked at pre-run, not evidence failure.
- On 2026-08-29, the rebuilt x86 runner passed 27 bounded scenarios on pinned
  QEMU-TCG 10.2.0. A hostile-image-only port command arms one observation; only
  an actual `ViVmExit::Preempted` emits the untagged host outcome. The strict
  interval was `START vcpu-preemption`, one `[hv-virtio-host] vcpu-preempted`,
  then `DONE vcpu-preemption`, with outer-QEMU liveness retained. VFS/Net
  supervisor restart and recovery scenarios also pass. The runner still exits
  `2` because ARM64 execution remains `BLOCKED_SCOPE`; Phase 06 therefore stays
  blocked without weakening or promoting that architecture.
- On 2026-09-04, the hostile corpus exposed an x86 delivery defect addressed
  by a host-tested/pending-runtime repair: a consumed `sti; hlt` retained
  `INT_SHADOW` and rejected the cell-side timer injection. The repair clears
  that shadow at the HLT exit, preserves due LAPIC injection, services RX
  independently of PIT delivery, gates VirtIO console SPI and notification on
  published used entries, and alternates a due PIT tick with a pending level
  VirtIO interrupt. Host device/MMIO unit tests pass (14/14 in
  `service-hypervisor`). Full runtime smoke, two-boot VirtIO persistence, and
  hostile execution reflect historical observations and remain pending fresh
  clean qualification; ARM64 remains `BLOCKED_SCOPE`; no physical
  qualification is implied.
