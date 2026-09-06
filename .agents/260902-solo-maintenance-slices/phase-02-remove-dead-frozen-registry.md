---
phase: 2
title: "Remove Dead FROZEN Registry"
status: completed
priority: P2
effort: "1h"
dependencies: []
tier: fast
---

# Phase 2: Remove Dead FROZEN Registry

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs. On a contract-breaking edge case, choose the smallest reversible option, log it, and stop before changing live hotswap behavior.

## Overview

Delete the unused cell-ID `FROZEN` set and its three public helpers, then correct source documentation to describe the live task-incarnation hotswap protocol.

## Requirements

- Functional: remove `FROZEN`, `freeze(CellId)`, `is_frozen(CellId)`, and `unfreeze(CellId)` with no replacement registry or compatibility alias.
- Functional: remove only the obsolete `FROZEN.force_unlock()` entry; retain unlock handling for `SWAP_CEILINGS` and `NEXT_FREEZE_NONCE`.
- Non-functional: preserve `TaskState::Frozen { swap_id }`, generation/nonce-bound replacement ceilings, rollback to `Ready`, pending-mailbox cutover, and source-task retirement.

## Architecture

The dead registry occupies `kernel/src/cell/hotswap.rs:11-20,45-46,58-74` and has no Rust caller. The live path freezes an exact TID under `SCHEDULER`, snapshots its capability ceiling, and removes it from ready queues (`hotswap.rs:262-295`); rollback is `unfreeze_task` (`:297-319`), and atomic accepted-message transfer is `commit_hotswap_barrier` (`:321-435`). `TaskState::Frozen` remains the single freeze authority.

## Assumptions

None — repository-wide symbol search and the active syscall/runtime paths were read directly.

## Related Files

- Modify: `kernel/src/cell/hotswap.rs`
- Modify: `kernel/src/task/tcb.rs`
- Intentionally unchanged: `kernel/src/task/syscall.rs`, `tests/integration/tests/hotswap-smoke.rs`, archived `.agents/**` plans

## Implementation Steps

1. Remove the `BTreeSet`-backed `FROZEN` static and its cell-ID `freeze`, `is_frozen`, and `unfreeze` functions as one clean deletion.
2. Remove `FROZEN.force_unlock()` from `force_unlock_locks`; preserve the documented fault-path safety contract and the two live lock releases.
3. Rewrite the module header and former “Freeze registry” heading to name exact task-incarnation state, ceiling reservation, rollback, and mailbox cutover; do not claim queuing is a future stub.
4. Correct `TaskState::Frozen` documentation in `task/tcb.rs:95-97`: accepted incoming IPC is retained in the task’s pending mailbox and transferred only by the live cutover barrier, not by a global cell-ID set.
5. Verify every syscall caller still targets `freeze_task_with_ceiling`, `unfreeze_task`, and the barrier helpers; do not rename or reshape those APIs.
6. Clean-cutover check: `git grep -nE '\b(FROZEN|is_frozen|freeze\(|unfreeze\()' -- kernel/src` must return no matches. Separately require `git grep -nE 'freeze_task_with_ceiling|unfreeze_task|commit_hotswap_barrier' -- kernel/src` to retain live definitions and callers.

## Commit Contract

One source-only cleanup commit owns both source files. No new test file is justified for deleting unreachable state; run existing unit/runtime gates against that commit. If ship policy requests a changelog note, record it later in a separate docs commit.

## Regression Commands

```bash
cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu cell::hotswap::tests::stale_reservation_cannot_restore_a_new_freeze
cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test hotswap-smoke hotswap_cli_preserves_demo_state -- --nocapture
cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test hotswap-smoke supervisor_hotswap_preserves_demo_state -- --nocapture
```

The QEMU tests require the existing release kernel/disk prerequisites documented at `tests/integration/tests/hotswap-smoke.rs:6-10`; use the existing image flow without changing QEMU provenance or versions.

## Completion Evidence

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu cell::hotswap::tests::stale_reservation_cannot_restore_a_new_freeze` — exit 0; 1 passed.
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` — exit 0; 88 passed, 0 failed.
- `cargo check --workspace --exclude app-mlibc-smoke --exclude doom --exclude tetris-c --exclude lua --exclude tetris-lua --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` — exit 0.
- `cargo clippy --workspace --exclude app-mlibc-smoke --exclude doom --exclude tetris-c --exclude lua --exclude tetris-lua --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` — exit 0.
- `cargo build --release -p cellos-kernel -p app-shell -p app-sys-tools -p app-bench -p supervisor -p hotswap-demo-v1 -p hotswap-demo-v2 -p service-hypervisor -p service-vfs --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` — exit 0.
- `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test hotswap-smoke hotswap_cli_preserves_demo_state -- --nocapture` — exit 0; 1 passed, including v2 restore, retained `SpawnCap`, successful cutover, and `[hotswap-cli-probe] PASS (v1 counter=5 -> v2 counter=5)`.
- `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test hotswap-smoke supervisor_hotswap_preserves_demo_state -- --nocapture` — exit 0; 1 passed, including state stash, ordered frozen FIFO drain/old-TID rejection, v2 restore, retained `SpawnCap`, successful cutover, and `[hotswap-supervisor-runtime] PASS (v1 counter=5 -> v2 counter=6)`.
- `git grep -nE '\b(FROZEN|is_frozen|freeze\(|unfreeze\()' -- kernel/src` — exit 1, expected no matches. `git grep -nE 'freeze_task_with_ceiling|unfreeze_task|commit_hotswap_barrier' -- kernel/src` — exit 0 with 7 retained definition/callsite hits.
- Review verdict: **CORRECT / safe to ship**, confidence 0.98, zero findings; review confirmed preservation of the live replacement, rollback, mailbox, and task-incarnation lifecycle.

## Success Criteria

- [x] No production-source reference to the cell-ID registry or its three APIs remains.
- [x] Live task freeze, rollback, replacement ceiling, mailbox cutover, and retirement symbols and callsites remain intact.
- [x] The stale-reservation unit test passes unchanged.
- [x] Scoped hotswap QEMU checks still reach v2 state restore, retained `SpawnCap`, successful CLI cutover, and preserved/incremented counter markers.
- [x] No hotswap syscall ABI, scheduler lock order, mailbox depth/ordering, service registry, or migration protocol change.

## Security Considerations

Task generation, swap ID, and freeze nonce bindings are anti-stale authority checks and must remain untouched. Removing the disconnected cell-ID set must not broaden who can freeze, resume, replace, or kill a task.

## Risk Notes

The API is public within the kernel crate, so feature-gated Rust consumers are the only plausible hidden users; explicit bare-metal compilation plus source search closes that risk. Archived plans may still describe historical `FROZEN` designs and are evidence, not live callsites.

## Documentation Trigger

No living project document currently names the dead registry. The source module and TCB comments are the required correction; do not rewrite archived plans or add a roadmap claim for behavior that did not change.

## Deviation Log

None.
