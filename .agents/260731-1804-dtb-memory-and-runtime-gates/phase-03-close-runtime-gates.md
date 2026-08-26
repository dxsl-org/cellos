---
phase: 3
title: "Close Phase 09 and 11 Runtime Gates"
status: completed
priority: P1
effort: 4h
dependencies: [2]
tier: thinking
---

# Phase 3: Close Phase 09 and 11 Runtime Gates

> Log every Decision / Deviation / Surprise in the Deviation Log when it occurs.

## Overview

Record phase 11 as runtime-verified from existing real-ELF/image evidence, then close phase 09 with
the missing negative runtime case and three-architecture/demo breadth.

## Requirements

- Do not rerun phase 11 solely to reproduce already sufficient evidence unless artifact provenance fails review.
- Phase 09 must exercise a validly signed, deliberately incomplete policy at runtime; a normal green boot is insufficient.
- The `/bin/nvme` fixture requests only P-TRUST authority. Preservation of ordinary authority
  is covered by the production policy self-test, which runs in the same boot and must pass.
- The test-only incomplete-policy path must not add a production bypass flag or weaken `assert_ptrust_covered`.
- Run current complete-policy smoke on RV64, AArch64, and x86_64 plus `periph-demo`, `robot-demo`, and `sensor-demo` where packaged.
- Preserve exact commands, commit, artifact hashes, positive event-26 trace, negative false-positive check, and pass counts.

## Assumptions

- **Claim:** Existing evidence closes phase 11: real RV64 sign/verify/tamper `ALL PASS`, 39 cells signed through F1/F5, W^X 2/2, and boot 54/54.
  **Confidence:** high
  **How to verify:** cross-check `f8eb7525`, the W^X report, and `.agents/reports/a4-runtime-gates-research-260731.md` before status edits.
- **Claim:** All three demos are present in the RV64 image; ARM/x86 packaging differs.
  **Confidence:** medium
  **How to verify:** inspect each image layout before declaring a skipped demo legitimate.

## Related Files and Ownership

| File | Action | Owner |
|---|---|---|
| `tests/integration/tests/policy-noentry.rs` | Create: runtime positive/negative policy gate | Phase 3 only |
| `tests/integration/Cargo.toml` | Modify: register the policy test | Phase 3 only |
| `tests/integration/src/lib.rs` | Modify only for reusable test-image injection helpers | Phase 3 only |
| `tests/integration/fixtures/` | Create: test-only incomplete signed policy input | Phase 3 only |
| `.agents/260727-2101-midori-lessons-cellos/phase-09-noentry-fail-closed.md` | Update verified status/evidence | Phase 3 only |
| `.agents/260727-2101-midori-lessons-cellos/phase-11-cellos-sign-f1.md` | Update verified status/evidence | Phase 3 only |
| `.agents/reports/a4-runtime-gates-260731.md` | Create consolidated evidence | Phase 3 only |

## Implementation Steps

1. Validate phase-11 provenance and update its stale open checkbox with existing evidence; do not alter signing code.
2. Build a test-only signed policy omitting one P-TRUST row and inject it into a disposable image copy.
3. Boot that image, spawn the omitted path, and assert event 26 reports stripped P-TRUST while ordinary authority survives.
4. Boot the complete policy and assert zero unexpected event-26 records.
5. Run RV64 serial boot suite and the three shell demos; run AArch64/x86 smoke and packaged demo paths.
6. Update phase 09/11 records only after the evidence report is complete.

## Test Matrix

| Gate | Expected |
|---|---|
| Phase 11 real cross-ELF | Sign and verify pass; PT_LOAD tamper rejected; `ALL PASS` |
| Phase 11 signed image | F1/F5 signs image cells and the same image reaches shell/boot suite |
| Phase 09 incomplete policy | Omitted P-TRUST path spawns without privileged bit; event 26 emitted |
| Phase 09 complete policy | Three-arch shell smoke; no unexpected event 26 |
| Demo breadth | Required markers for periph, robot five-cycle completion, and sensor output |
| RV64 regression | Current serial boot suite passes with recorded count |

## Success Criteria

- [x] Phase 11's original two missing runtime artifacts are linked and its status is verified.
- [x] Phase 09 has one positive event-26 runtime trace and one complete-policy zero-event trace.
- [x] RV64, AArch64, and x86_64 reach their expected smoke marker without panic/fault.
- [x] Demo availability and results are recorded per architecture without silent skips.
- [x] A2 and A3 remained deferred until this phase completed.

## Security Considerations

The negative fixture must be isolated to tests and visibly test-only. Never add a general option that signs an
incomplete production policy, and never reuse its image outside the test run.

## Risk Notes and Rollback

Verification failures do not authorize opportunistic fixes: record the shortest decisive failure and open a focused
bugfix. Test fixture/harness changes are independently revertible. `.agents` status edits roll back by restoring the
prior checkbox/status; production phase-09/11 code is unchanged by this phase.

## Deviation Log

- **Packaging gap:** the current ARM image contains `periph-demo` but not `sensor-demo` or
  `robot-demo`; those lanes are unavailable, not passing.
- **Harness limit:** the fresh full RV64 serial suite exceeded 20 minutes, so no new total is
  claimed. Focused gates and the prior 54/54 evidence are recorded separately.
