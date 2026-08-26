---
phase: 6
title: "G3 Accelerator Readiness Envelope"
status: blocked
priority: P2
effort: "4d"
dependencies: [1, 5]
tier: thinking
---

# Phase 06: G3 Accelerator Readiness Envelope

## Context Links

- `docs/project-roadmap.md:192-203`; `docs/specs/04-hardware.md:116-146`; `docs/TODO.md:28`.

## Overview

Prepare for G3 without freezing the accelerator ABI too early. The deliverable is a hardware-informed probe/FFI envelope, not a kernel NPU scheduler. This phase is blocked until real accelerator hardware, a reviewed vendor SDK/license, and measured inference evidence exist.

## Blockers

- No confirmed RK3588 or X390 hardware is available in this slice.
- No accepted vendor SDK/license package is present for RKNN or X390 VCIX.
- No real inference run exists yet, so there is no P99, error, or fault profile to freeze against.
- No restart/fault-injection evidence exists for proving that driver-cell restart does not kill app cells.
- Large-buffer IPC prerequisites remain design-only until a real probe path exercises them.

## Requirements

- Functional: define RK3588/RKNN and X390 evidence gates, trace large-buffer IPC prerequisites, and reserve a draft-only accelerator vocabulary. Do not add scheduler/API work before hardware evidence.
- Non-functional: no `ViAccelerator` ABI freeze until real NPU hardware and vendor APIs have been exercised.

## Architecture

Data flow: model bytes -> Tier 1b vendor runtime probe cell -> submit/wait observations -> latency/error/fault ledger -> later `TensorBuffer` and scheduler design. No kernel scheduling changes in this phase.

## Related Code Files

- Create later only after the hardware gate: `cells/drivers/rknn-probe/`.
- Evidence ledger target: `docs/research/g3-accelerator-evidence.md`.
- Read/possibly modify later: `libs/api/src/abi/syscall.rs`, `libs/ostd/src/grant.rs`, large-buffer IPC files identified by Phase 05.
- Reference: vendor SDK docs/binaries only after license review; no current local RKNN/X390 source observed.

## Implementation Steps

1. Confirm hardware target: RK3588 first or SiFive P870+X390 later.
2. Accept a vendor SDK/license package and pin the exact runtime/runtime version.
3. Define probe-only IPC that records load/submit/wait/error semantics without stable ABI promise.
4. Verify large-buffer IPC/sys_grant_pages prerequisite and gap list.
5. Run one bounded inference demo through Tier 1b on real hardware before drafting kernel scheduler work.
6. Produce a separate G3 scheduler/API plan after two months of real API evidence.

## Todo List

- [x] Evidence envelope documented while hardware and SDK remain unavailable.
- [ ] Hardware and SDK license confirmed.
- [ ] Probe cell reads model, submits inference, and reports P99/error/fault behavior.
- [ ] Fault injection proves driver cell restart does not kill app cells.

## Success Criteria

- [ ] G2 inference demo has a P99 bound and exact hardware evidence.
- [ ] Draft `ViAccelerator` remains marked non-ABI/non-Law-1 until evidence gate closes.
- [ ] No kernel NPU scheduler code lands in this phase.

## Test Matrix

- Unit: probe message parser and grant bounds.
- Integration: Tier 1b FFI load/submit/wait on real SDK.
- E2E: RK3588 or X390 real board inference loop with driver restart.

## Risk Assessment

| Risk | LxI | Mitigation |
|---|---|---|
| Premature ABI freeze | HxH | draft-only namespace; Law 1 gate after hardware evidence. |
| Vendor SDK license conflict | MxH | keep binary/vendor code out until license review. |
| Tensor copies hide performance | HxM | require large-buffer/zero-copy measurement before scheduler plan. |

## Security Considerations

NPU DMA/faults inherit Phase 05 isolation rules; model weights may be shared only after integrity and revocation rules exist.

## Backward Compatibility

No public ABI changes; any probe protocol is internal and versioned as experimental.

## File Ownership

Owns G3 research/probe docs and experimental probe crate only; no overlap with G1/G2 driver promotion.

## Rollback

Delete probe crate/docs and remove experimental package from build scripts. Irreversible part: none in code; hardware procurement and vendor account setup are external sunk costs.

## Assumptions

- Claim: no current local RKNN/X390 source exists in reference tree. Confidence: medium. How to verify: re-scan vendor/reference folders after hardware is chosen.
- Claim: phase 06 remains docs-only until real hardware and vendor SDK access exist.

## Evidence

- `docs/roadmap/product-stages.md:32-34` parks G3 until hardware exists and the team has vendor API experience.
- `docs/specs/04-hardware.md:120-145` keeps `ViAccelerator` hardware-informed and ties the contract to RK3588/X390 evidence.
- `.agents/260819-1416-port-common-drivers-g1-g2-g3/reports/harness/verification.json` records the phase 05 verification slice as `pass-with-deferred-unit-harness-gap`.
- `.agents/260819-1416-port-common-drivers-g1-g2-g3/reports/harness/review-decision.json` records the reviewer verdict as `PASS_WITH_RISK`.

## Deviation Log

2026-08-20: added docs/research/g3-accelerator-evidence.md as the explicit blocker/evidence envelope; no probe crate or ABI work started.
2026-08-20: phase marked blocked because the repo still lacks real hardware, an accepted SDK/license, measured inference P99/error/fault data, and restart evidence.

## Next Steps

Open a separate G3 scheduler/API plan only after hardware and probe evidence exist.
