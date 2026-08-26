---
phase: 04
title: "Retire syscall 400 and delete kernel hotswap orchestration"
status: complete
priority: P2
effort: "2-3d"
dependencies: [0, 1, 2, 3]
tier: thinking
---

# Phase 04 - Retire syscall 400; shrink hotswap to kernel mechanism

## Context Links
- Plan: [plan.md](plan.md)
- Current commit audited: `209e0f2b` on `main`.
- Boundary law: `docs/specs/15-kernel-boundary.md:233` says orchestration policy belongs in a privileged Cell.
- Law 1: `docs/code-standards.md:18` makes `libs/api/` ABI edits require 2 explicit confirmations.

## Overview
- Final destructive phase complete. The Law 1 ABI retirement of `HotSwap=400` was confirmed twice on 2026-08-08; `HotSwap=400` is retired/reserved and the kernel keeps only the listed mechanism syscalls.
- Delete only the legacy in-kernel whole-sequence hotswap path. Keep the kernel mechanisms used by `FreezeCell=413`, `ResumeCell=414`, `KillCell=415`, `QueryHotswapReady=419`, and `SpawnReplacement=421`.
- Replace the old "three-arch boot + reliability + hotswap + snapshot-write" gate with executable checks below. Real MMC snapshot save/restore remains host-gated.

## Key Insights
- `HotSwap=400` was retired/reserved in Phase 04: the ABI variant at `libs/api/src/abi/syscall.rs:346`, allowlist bit 32 at `libs/api/src/abi/syscall.rs:645`, number decode at `libs/api/src/abi/syscall.rs:837`, kernel enum arm at `kernel/src/task/syscall.rs:1093`, dispatch at `kernel/src/task/syscall.rs:4046`, decode at `kernel/src/task/syscall.rs:4841`, and ostd wrapper at `libs/ostd/src/syscall.rs:1089` are no longer live paths.
- No current cell CLI calls `sys_hotswap`: `/bin/hotswap` resolves `service::SUPERVISOR` and sends IPC at `cells/tools/sys-tools/src/bin/hotswap.rs:45`; the supervisor authorizes the exact CLI sender at `cells/services/supervisor/src/main.rs:50` and calls userspace orchestration at `cells/services/supervisor/src/main.rs:66`.
- `spawn=true` now feeds the surviving readiness/state-transfer intent bits through `app_syscall_set`; `HotSwapReady`/state-transfer semantics keep bit 32 for readiness without reviving the retired syscall path.
- There is no tracked root `CLAUDE.md` or `AGENTS.md` in this checkout. The old CLAUDE update item is stale; update docs listed in Related Files instead.

## Requirements
- Functional: syscall number 400 no longer reaches any hotswap behavior; supervisor IPC remains the only hotswap orchestration path.
- Functional: number 400 is reserved/retired, not reused. `ViSyscall::from_number(400)` must return `Unknown`, and a unit test must lock that behavior.
- Functional: `HotSwapReady=401`, state stash/restore/clear `410-412`, `FreezeCell=413`, `ResumeCell=414`, `KillCell=415`, `QueryHotswapReady=419`, `Snapshot=420`, `SpawnReplacement=421`, and `PauseService=422` remain ABI-stable.
- Non-functional: no new kernel policy path, no new userspace privilege, no stale docs claiming direct syscall 400 use.

## Architecture
Data flow after Phase 04:

1. `/bin/hotswap` input enters `cells/tools/sys-tools/src/bin/hotswap.rs:32`, validates service/path, sends `OP_HOTSWAP` to `service::SUPERVISOR`.
2. Supervisor validates sender name and parses the request at `cells/services/supervisor/src/main.rs:50`, then orchestrates pause, snapshot IPC, freeze, spawn replacement, restore, commit, kill at `cells/services/supervisor/src/hotswap.rs:95`.
3. Kernel accepts only mechanism syscalls from SupervisorCap holders: freeze (`kernel/src/task/syscall.rs:3601`), resume/commit (`kernel/src/task/syscall.rs:3643`), kill (`kernel/src/task/syscall.rs:3676`), ready query (`kernel/src/task/syscall.rs:3710`), replacement spawn (`kernel/src/task/syscall.rs:3725`), and snapshot (`kernel/src/task/syscall.rs:4083`).
4. Data exits as committed service mapping plus FIFO transfer via `commit_hotswap_barrier` (`kernel/src/cell/hotswap.rs:267`) or bounded rollback through supervisor code (`cells/services/supervisor/src/hotswap.rs:172`).

Dependency graph:

```
P00 cap ceiling + P01 atomic cutover + P02 CLI/supervisor path + P03 snapshot gate
    -> confirm syscall400 retirement
    -> remove ABI/dispatch/wrapper/runtime grant
    -> delete kernel-only orchestration helpers
    -> rebuild images
    -> run RV64 hotswap/snapshot gates and arch boot gates
    -> update docs/status
```

## Keep/Delete Map
Keep:
- `force_unlock_locks` because fault teardown calls it at `kernel/src/task.rs:490`.
- `take_frozen_replacement_ceiling` because `SpawnReplacement=421` consumes it at `kernel/src/task/syscall.rs:3756`.
- `clear_swap_ceiling` because `unfreeze_task` and scheduler exit use it at `kernel/src/cell/hotswap.rs:245` and `kernel/src/task/scheduler.rs:491`.
- `bind_replacement` because `SpawnReplacement=421` binds the new TID at `kernel/src/task/syscall.rs:3781`.
- `set_task_hotswap_ready` because `HotSwapReady=401` calls it at `kernel/src/task/syscall.rs:4079`.
- `freeze_task_with_ceiling` for `FreezeCell=413` at `kernel/src/task/syscall.rs:3610`.
- `unfreeze_task` for plain `ResumeCell=414` at `kernel/src/task/syscall.rs:3659`.
- `commit_hotswap_barrier` for atomic cutover `ResumeCell=414` at `kernel/src/task/syscall.rs:3665`.
- `exit_task_internal` for `KillCell=415` and failed replacement rollback at `kernel/src/task/syscall.rs:3703` and `kernel/src/task/syscall.rs:3792`.

Delete after grep confirms no non-orchestrator callers:
- `HotSwap=400` ABI/dispatch/decode/wrapper.
- `freeze`, `is_frozen`, `unfreeze`, `next_swap_id`, `HOTSWAP_TIMEOUT_TICKS`, `APP_MSG_MAGIC`, `DISC_SNAPSHOT`, `DISC_RESTORE`, `find_tid_for_cell`, `set_task_frozen`, `send_snapshot_event`, `send_restore_event`, `fmt_u64_decimal`, `stash_key_for`, `wait_for_stash_key`, `wait_for_hotswap_ready`, and public `hotswap()`.
- `kernel/src/task.rs` internal `send_to`/`recv_from` only if they become unused after deleting kernel hotswap; current comments name the old orchestrator at `kernel/src/task.rs:1588` and `kernel/src/task.rs:1596`.

## Related Files
- Modify `libs/api/src/abi/syscall.rs`: remove `HotSwap`, reserve 400 in comments, ensure `from_number(400) -> Unknown`.
- Modify `libs/api/src/abi/syscall_tests.rs`: add number-retirement and allowlist tests.
- Modify `libs/api/src/abi/manifest_flags.rs`: remove direct `HotSwap` wording from SpawnCap docs.
- Modify `libs/ostd/src/syscall.rs`: remove `sys_hotswap`; keep `sys_hotswap_ready`.
- Modify `libs/ostd/src/runtime.rs`: replace implicit `HotSwap` grant with explicit surviving readiness/state-transfer grant; keep bit 32 for `HotSwapReady`/`Snapshot`.
- Modify `libs/ostd/src/cap.rs`: replace "use sys_hotswap" guidance with supervisor hotswap guidance.
- Modify `kernel/src/task/syscall.rs`: remove `Syscall::HotSwap`, its `ViSyscall` mapping, and dispatch arm.
- Modify `kernel/src/cell/hotswap.rs`: delete legacy orchestrator, keep mechanisms listed above.
- Modify `kernel/src/task.rs` only if compiler/grep proves `send_to`/`recv_from` are dead.
- Regenerate and include `kernel/src/embedded/init` when the fresh disk build changes this tracked bootstrap ELF; it is a derived source artifact, not incidental test drift.
- Modify docs: `docs/specs/15-kernel-boundary.md`, `docs/hotswap-guide.md`, `docs/codebase-summary.md`, `docs/specs/12-reliability.md`, `docs/system-architecture.md`, `docs/project-roadmap.md`, `docs/project-changelog.md`.

## Implementation Steps
1. Checkpoint: both Law 1 confirmations recorded on 2026-08-08; retire public ABI syscall `HotSwap=400` and reserve the number only within this approved phase scope.
2. Re-run grep before editing: `git grep -n -E "ViSyscall::HotSwap\b|Syscall::HotSwap\b|sys_hotswap\b|400 => ViSyscall::HotSwap|HotSwap = 400" -- kernel libs cells tests docs`.
3. Remove live ABI, dispatch, decode, wrapper, and implicit runtime grant. Add a test that 400 decodes to `Unknown` and that `HotSwapReady` keeps its allowlist bit.
4. Delete kernel-only orchestration helpers from `kernel/src/cell/hotswap.rs`; keep every mechanism named in Keep/Delete Map.
5. Update stale comments/docs that now claim direct syscall hotswap or kernel-owned orchestration.
6. Rebuild images from fresh sources before QEMU; stale embedded images have previously produced false evidence.
7. Run the verification matrix. Record host-gated items as deferred, not failed or passed.

## Verification Matrix
- Static ABI: `cargo test -p api --target x86_64-unknown-linux-gnu syscall` proves 400 retirement and preserved bits.
- Kernel/API build: `cargo check -p vicell-kernel`; `cargo check -p ostd --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` if local target deps are installed.
- Fresh RV64 image: `pwsh ./gen_disk.ps1`; inspect for `FATAL` because this script can exit zero after inner cargo failure.
- RV64 boot: `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel`.
- RV64 hotswap: `CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --manifest-path tests/integration/Cargo.toml --test hotswap-smoke -- --test-threads=1`; this target exists at `tests/integration/Cargo.toml:83` and covers supervisor state, SpawnCap retention, FIFO cutover, unauthorized sender denial at `tests/integration/tests/hotswap-smoke.rs:126`, `tests/integration/tests/hotswap-smoke.rs:167`, and `tests/integration/tests/hotswap-smoke.rs:218`.
- RV64 snapshot authority: `CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --manifest-path tests/integration/Cargo.toml --test launch-profile -- --test-threads=1`; it checks supervisor-routed NullBlock unavailability and non-supervisor denial at `tests/integration/tests/launch-profile.rs:93` and `tests/integration/tests/launch-profile.rs:101`.
- AArch64 boot: build fresh aarch64 cells/kernel/disk using the CI-equivalent commands in `.github/workflows/ci.yml:326` through `.github/workflows/ci.yml:385`, then `BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh`.
- x86_64 boot: build fresh x86 cells/kernel/ISO using `.github/workflows/ci.yml:648` through `.github/workflows/ci.yml:696`, then `BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso`.
- Host-gated defer: real MMC snapshot save/restore. QEMU currently proves only `NullBlock` unavailability (`kernel/src/task/drivers/block.rs:18`, `tests/integration/tests/launch-profile.rs:95`), not a successful write/restore.

## Success Criteria
- [x] Both user confirmations recorded for the Law 1 ABI retirement on 2026-08-08.
- [x] No live `HotSwap=400` ABI/dispatch/wrapper remains; 400 is reserved and decodes to `Unknown`.
- [x] `git grep -n -E "ViSyscall::HotSwap\b|Syscall::HotSwap\b|sys_hotswap\b|400 => ViSyscall::HotSwap|HotSwap = 400" -- kernel libs cells tests` returns empty.
- [x] `HotSwapReady=401`, `Snapshot=420`, `SpawnReplacement=421`, and supervisor hotswap continue to pass the RV64 hotswap and launch-profile tests.
- [x] AArch64 and x86_64 boot gates are explicitly recorded as host/tooling-gated with the exact missing prerequisite; the fresh-image RV64, hotswap-smoke, launch-profile, and release-kernel lanes passed.
- [x] `docs/specs/15-kernel-boundary.md` §3.2 table updated with the true mechanism/policy LOC split.

## Post-Plan Followups

- Fresh x86 and AArch64 QEMU boot packaging remains host-tooling-gated; the missing prerequisite is the Windows/WSL path bridge that produces matching fresh artifacts.
- Host API coverage remains 33.26 percent line and 0 percent branch. Bare-metal behavior is still covered by the QEMU lanes above.
- The broader docs sweep still has legacy hotswap-orchestrator wording in `docs/hotswap-guide.md` and `docs/specs/12-reliability.md`; that cleanup is non-blocking and separate from plan closure.

## Risk Assessment
- High x High: Law 1 ABI removal can break stale cells compiled against `HotSwap=400`. Mitigate with double confirmation, number reservation, from-number test, and rollback by reverting this phase.
- Medium x High: deleting a still-live mechanism helper could break 413/414/415/421. Mitigate with the keep map and pre-edit grep of every helper caller.
- Medium x Medium: `spawn=true` allowlist could accidentally stop granting bit 32, breaking demo `sys_hotswap_ready`. Mitigate by replacing `HotSwap` with `HotSwapReady` explicitly and testing the demo path.
- Medium x Medium: QEMU evidence can be false if images are stale. Mitigate by rebuilding images before integration tests and checking `gen_disk.ps1` output for `FATAL`.
- Low x High: docs could overclaim real snapshot writes. Mitigate by stating NullBlock/unavailable for QEMU and deferring MMC save/restore.
- Rollback: `git revert` the Phase 04 implementation commit restores syscall 400 and the orchestrator. Cannot rollback external cells already rebuilt without syscall 400 except by rebuilding them from the revert.

## Security Considerations
- Security win: removes the weaker SpawnCap-gated whole-sequence replacement path at `kernel/src/task/syscall.rs:4051`; orchestration remains behind supervisor identity checks and SupervisorCap mechanisms.
- Security risk: a stale `spawn=true` grant for retired 400 would preserve a dead authority bit with confusing semantics. The phase must remove live dispatch and document the reservation.

## Assumptions
- None - all claims above were grepped/read in this checkout. Runtime pass/fail is deliberately left to the verification matrix.

## Deviation Log
- Decision: Law 1 confirmation 1 approved on 2026-08-08.
  Scope: retire `HotSwap=400` while reserving the number and preserving all listed mechanism syscalls; the exact code/test/docs boundary and real-MMC deferral remain subject to confirmation 2.
  Impact: Phase 04 may prepare the evidence package but must not edit source until the second approval.
- Decision: Law 1 confirmation 2 approved on 2026-08-08.
  Scope: ABI/runtime, kernel mechanism boundary, focused retirement proof, hotswap/snapshot regressions, and listed documentation changes are approved; no snapshot format, restore, or MMC success claim may enter the phase.
  Impact: Phase 04 may enter Build.
- Deviation: old `CLAUDE.md` update is removed.
  Why: no root `CLAUDE.md` or `AGENTS.md` is tracked in the current checkout.
  Impact: docs scope shifts to current tracked docs.
  Revert: re-add only if such a file appears before implementation.
- Deviation: old `snapshot-write` gate is replaced.
  Why: no `snapshot-write` test target exists; current RV64 evidence is NullBlock unavailable plus non-supervisor denial.
  Impact: real MMC save/restore remains host-gated.
  Revert: add a concrete snapshot-write lane if one lands before Phase 04 starts.
- Verification: reachable final lanes passed on 2026-08-08: API host tests 75/75, RV64/AArch64/x86 release-kernel builds, hotswap-smoke 15/15, and launch-profile 1/1.
  Evidence: the hotswap markers retain SpawnCap, FIFO cutover, direct-sender denial, and state preservation; snapshot authority remains the Supervisor-routed NullBlock/unavailable proof.
  Boundary: x86 and AArch64 QEMU boot gates are host-tooling-gated because the Windows/WSL bridge corrupted fresh artifact paths. They are deferred with exact failure evidence, not claimed green.
- Surprise: a fresh `gen_disk.ps1` run succeeded after the baseline PowerShell/Zig-script failure and regenerated `kernel/src/embedded/init`.
  Decision: retain the binary in this phase because it is the tracked embedded init ELF copied from the freshly rebuilt `app-init`; previous hotswap commits track the same derived artifact.
  Boundary: optional `tetris-lua` remained omitted. API host coverage is 33.26 percent line and 0 percent branch; it is reported as a coverage gap, while the affected runtime paths have QEMU evidence.
- Decision: finalize acceptance approved on 2026-08-08.
  Accepted boundaries: fresh x86 and AArch64 QEMU boot remain host-path-gated, and host API coverage remains below the desired threshold. Neither is represented as a passing runtime result.
  Impact: Phase 04 may finalize and request a commit; the two evidence gaps remain recorded as follow-up host/tooling work.

## Next Steps
- Plan closed. Use the post-plan followups above for any non-blocking cleanup or host-gated boot work.
