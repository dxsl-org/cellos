---
phase: 3
title: "QEMU Evidence and Partial Status Closure"
status: completed
priority: P1
effort: "4h"
dependencies: [1, 2]
tier: medium
---

# Phase 3: QEMU Evidence and Partial Status Closure

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs.

## Overview

Make the QEMU gate assert the new `ReadFileGrant` markers, save a concise report, and update the original Phase 01 status honestly as partial.

## Requirements

- Functional: QEMU test fails if `ReadFileGrant` markers are absent or any new grant FAIL marker appears.
- Non-functional: original Phase 01 remains partial because fast-IPC `GetFile` is still blocked by D1.
- Backward compatibility: existing `[vfs-test] ALL TESTS PASSED` marker at `cells/tests/vfs-test/src/main.rs:480` remains.

## Architecture

Data flow:

1. `scripts/build-test-hooks-ci.sh:44` builds `service-vfs` and `app-vfs-test` with `test-hooks`.
2. `scripts/build-test-hooks-ci.sh:110` builds the test-hooks kernel artifact.
3. `tests/integration/tests/vfs-quota.rs:67` boots the kernel under QEMU.
4. The test waits for `ReadFileGrant` grant markers and `[vfs-test] ALL TESTS PASSED`.
5. Evidence report records command, commit SHA, serial markers, `ReadGrant` source decision, and fast-IPC D1 blocker state.

## Assumptions

- Claim: The integration harness can wait for multiple markers from the accumulated serial buffer.
  Confidence: medium
  How to verify: run the QEMU gate with the new waits.

## Related Files

- Modify: `tests/integration/tests/vfs-quota.rs`
- Modify: `.agents/260801-parallel-midori-closure/phase-01-p02-runtime-verify.md`
- Modify: `.agents/260801-parallel-midori-closure/reports/midori-p02-runtime-verify-report.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`

## Implementation Steps

1. Add marker waits in `tests/integration/tests/vfs-quota.rs` for:
   - `[PASS] grant: ReadFileGrant copies nonzero bytes`
   - `[PASS] grant: ReadFileGrant is refused after sealing`
   - Phase 2 `ReadGrant` marker only if unblocked.
2. Keep the existing wait for `[vfs-test] ALL TESTS PASSED`.
3. Add a serial dump assertion that no `[FAIL] grant:` marker appears.
4. Run baseline commands from `plan.md`.
5. Update `.agents/260801-parallel-midori-closure/phase-01-p02-runtime-verify.md` to keep `status: partial`; do not check the full criterion at line 54.
6. Update the existing implementation report with command lines, decisive serial markers, `ReadGrant` source decision, and `fast-IPC GetFile blocked by D1`.
7. Update living docs to say Phase 01 remains partial pending separate approved Tier-1 rewrite/rescope or formal success-criteria rescope.

## Success Criteria

- [ ] `bash scripts/build-test-hooks-ci.sh` passes.
- [ ] QEMU integration command passes with all mandatory markers.
- [ ] Original Phase 01 report states `ReadFileGrant` is QEMU-proven and Phase 01 remains partial.
- [ ] Original Phase 01 report states actual fast-IPC `GetFile` remains blocked by D1.
- [ ] Roadmap/changelog status matches partial report and does not overclaim.

## Security Considerations

- Risk High x Medium: roadmap says complete while fast-IPC remains blocked. Mitigation: status wording must include partial/D1 blocker.
- Risk Medium x Medium: marker wait passes due stale serial text. Mitigation: boot a fresh QEMU runner and dump serial on failure.
- Risk Low x Medium: docs mention production fast-IPC. Mitigation: wording must say blocked by D1, separate rewrite/rescope required.

## Test Matrix

- Build: `bash scripts/build-test-hooks-ci.sh`
- Integration: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`
- Contract guard: `git diff -- libs/api libs/types kernel/src/loader/reloc.rs`
- D1 guard: `rg -n "resolve_export|R_RISCV_JUMP_SLOT" kernel/src docs/specs/17-ipc-wire-contract.md`

## Risk Notes

- Rollback: revert test wait changes and status/report edits; source changes roll back via Phases 1-2.
- Cannot undo: historical report entries should be superseded, not deleted, if evidence changes.
- Stop gate: if the only way to make the gate green is to weaken prerequisites or remove `ci_guard`, stop.

## Deviation Log

- 2026-08-01 — The first post-edit `vfs-quota` run still booted the pre-edit test-hooks kernel artifact, so the new grant markers were absent even though the host-side test binary had recompiled. Rebuild the kernel image with `bash scripts/build-test-hooks-ci.sh` before trusting any QEMU marker result from this lane.
