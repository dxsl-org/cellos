---
phase: 6
title: "Stack Overflow Hardening"
status: completed
priority: P1
effort: "1d"
dependencies: [5]
tier: thinking
---

# Phase 06: Stack Overflow Hardening

## Overview

Phase 05 is closed on 2026-08-06, and this phase is now closed too: two bottom guards and a real U-mode `cause=0xf` probe landed, VFS continuation stayed intact, all three boot arches passed, and no public ABI or stack shrink landed.

## Requirements

- Functional: add stronger overflow protection than today's one verified bottom guard page.
- Functional: keep stack allocation fail-closed if guard/probe setup cannot be proven.
- Non-functional: no per-path shrink in this phase; no public ABI changes.

## Architecture

Data flow: spawn path asks `stack_pages_for(name)`, stack allocator reserves frames, guard/probe policy transforms allocation into a verified protected stack, scheduler receives only a valid stack. Failure exits as `ViError`, not a degraded unguarded stack.

## Assumptions

- **Claim:** Two guard pages or a production probe can fit current contiguous allocation constraints.
  **Confidence:** medium
  **How to verify:** Stress allocate under current `STACK_PAGES + guards` and run boot on all three arches.

## Related Files

- Modify: `kernel/src/task/stack.rs`
- Modify: `kernel/src/task/scheduler.rs`
- Modify: `kernel/src/task.rs`
- Modify: `kernel/src/task/thread_quota_selftest.rs`
- Modify: `tests/integration/tests/vfs-quota.rs`
- Modify: `docs/specs/12-reliability.md`

## Implementation Steps

1. Choose one hardening mechanism: two bottom guard pages, a stack probe, or another evidence-backed equivalent.
2. Update allocation accounting from `Stack` fields, not global constants.
3. Preserve existing `usable_bytes()` semantics for stack watermarks.
4. Add a deliberate-overflow test cell or test-hook marker that proves the trap/fail-closed path.
5. Keep `stack_pages_for` default-only until Phase07.

## Success Criteria

- [x] Deliberate overflow test fails loud without corrupting another cell.
- [x] `stack_pages_for(_name)` still returns 64 pages for all paths after hardening.
- [x] No boot arch regresses.

## Outcome

- Two bottom guards: PASS.
- U-mode overflow probe: PASS, `cause=0xf`.
- VFS continuation: PASS.
- RV64/AArch64/x86_64 boot: PASS.
- Tester: PASS.
- Reviewer: APPROVE.
- Public ABI: unchanged.
- Stack shrink: unchanged.

## Validation Commands

```bash
cargo fmt --all --check
bash scripts/build-test-hooks-ci.sh
cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test vfs-quota
BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh
BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso
BOOT_WINDOW=120 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel disk_v3.img
```

## Security Considerations

Stack overflow in SAS can corrupt another cell without a process boundary. A guard setup failure must refuse spawn, not log and continue.

## Risk Notes

- High x High: stronger guards increase contiguous allocation pressure. Mitigation: measure allocation refusal and keep shrink for Phase07.
- Medium x High: overflow test corrupts the shared QEMU run. Mitigation: run in isolated test-hooks image with timeout and log scan.
- Rollback: revert stack/test/docs edits; default 64-page stacks remain. Irreversible part: none.

## Deviation Log

None.
