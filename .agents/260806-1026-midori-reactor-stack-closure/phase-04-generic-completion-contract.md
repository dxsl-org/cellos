---
phase: 4
title: "Generic Completion Contract"
status: completed
priority: P1
effort: "1d"
dependencies: [3]
tier: thinking
---

# Phase 04: Generic Completion Contract

## Overview

After Law 1 confirmation only, extend completion semantics just enough for a parked executor to wait on non-NET_RX sources without breaking Recv or peer-death behavior.
Blocked until Phase03 captures the two explicit Law 1 confirmations.

## Requirements

- Functional: widen `WaitCompletion` source handling beyond `NET_RX` only if Phase03 confirms it.
- Functional: define target-gone/generation semantics for operations that depend on a peer.
- Functional: preserve `Recv` and `RecvTimeout`; do not migrate them to CQ in this phase.
- Non-functional: additive ABI where possible; old cells with only `WaitForEvent` authority must continue to boot.

## Architecture

Data flow: user future submits a source-specific wait, kernel reserves a completion slot, source completion writes `ViCompletion`, scheduler wakes the waiter, `ostd` drains the record. Peer-dependent waits must bind to a generation or equivalent kernel-internal identity so a reused TID cannot satisfy an old wait.

## Assumptions

- **Claim:** The reserved bytes in `ViCompletion` can carry future source/error metadata without moving `result`.
  **Confidence:** medium
  **How to verify:** Re-run ABI size/layout tests in `libs/api/src/abi/completion.rs` and enumerate all `ViCompletion` parsers before editing.

## Related Files

- Modify: `libs/api/src/abi/syscall.rs`
- Modify: `libs/api/src/abi/completion.rs`
- Modify: `libs/api/src/abi/syscall_tests.rs`
- Modify: `libs/api/src/abi.rs`
- Modify: `kernel/src/task/completion_wait.rs`
- Modify: `kernel/src/task/completion.rs`
- Modify: `kernel/src/task/scheduler.rs`
- Modify: `libs/ostd/src/syscall.rs`

## Implementation Steps

1. Verify Phase03 has two explicit confirmations; abort if not.
2. Enumerate every consumer of `ViCompletion`, `WaitCompletion`, and event bits; update all tests in the same change.
3. Add only the minimum source/result vocabulary required by Phase05; do not add VFS, DMA, or RecvScatter sources.
4. Add peer-death completion for newly introduced peer-dependent waits, using generation-safe identity or a fail-closed invalidation path.
5. Preserve existing `WaitCompletion(NET_RX)` behavior and allowlist bit 42 compatibility unless Phase03 authorized otherwise.
6. Add host ABI tests for stable numeric IDs, encoded size, reserved fields, and rejected multi-bit source masks.

## Success Criteria

- [x] `cargo test -p api --target x86_64-unknown-linux-gnu` passes and proves no accidental ABI drift.
- [x] `WaitCompletion(NET_RX)` still works with old net service behavior.
- [x] Dead-peer test from Phase02 remains on the old mailbox path; no completion migration or hang was introduced.

## Validation Commands

```bash
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test -p api --target x86_64-unknown-linux-gnu
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
export CARGO_BUILD_TARGET=riscv64gc-unknown-none-elf
export CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export OBJCOPY=riscv64-unknown-elf-objcopy
pwsh ./gen_disk.ps1
BOOT_WINDOW=120 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel disk_v3.img
```

## Security Considerations

Completion records cross the kernel/cell boundary. A stale task id or ambiguous error value can wake the wrong cell logic after restart.

## Risk Notes

- High x High: TID reuse satisfies stale completion. Mitigation: generation-safe identity or fail-closed invalidation.
- Medium x High: allowlist compatibility breaks old cells. Mitigation: preserve bit 42 semantics unless explicitly authorized.
- Rollback: revert Phase04 files and keep Phase01/02 ABI-free proof. Irreversible part: shipped ABI cannot be silently reused; if committed, document deprecation.

## Deviation Log

Law 1 confirmations 1/2 and 2/2 were recorded before the first public edit. Reviewer initially blocked a dead-task TIMER slot leak and a `WaitForEvent` accounting collision; both were fixed with explicit wait bookkeeping and deferred out-of-lock cleanup. The user authorized adding `kernel/src/task/tcb.rs` and `kernel/src/task.rs` for that internal lifecycle fix. End-to-end userspace TIMER proof moves to Phase05, its first real consumer.
