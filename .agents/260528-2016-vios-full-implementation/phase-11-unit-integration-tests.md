# Phase 11 — Unit & Integration Tests

**Effort:** 80h | **Priority:** P2 | **Status:** pending | **Blockers:** Phase 03, Phase 04

## Overview

Build a comprehensive test suite: unit tests for kernel internals (frame allocator, scheduler, IPC, cap registry) and integration tests for multi-Cell flows (Init→Config→VFS→Shell, VFS file ops, keyboard streaming, FileHandle IPC). Targets ≥80% line coverage on `kernel/` and `libs/`. Every prior phase ships with at least one test; this phase fills the gaps.

## Context Links

- `docs/10-testing.md` — testing strategy
- `cells/apps/test-isolation/src/lib.rs` — existing isolation test scaffold
- `kernel/src/task/tests.rs` and `kernel/src/memory/tests.rs` already exist (audit + extend)
- `kernel/src/task/ipc_test.rs` exists (audit + extend)

## Key Insights

- `no_std` unit tests need a custom test runner: cargo's default test harness assumes `std`. Use `#![test_runner(crate::test_runner)]` + `#![feature(custom_test_frameworks)]` per kernel crate.
- Integration tests for an OS = boot QEMU + inject input + grep serial output. Wrap that in `cargo test --test <name>` using a build.rs that runs QEMU as a subprocess.
- Coverage measurement on `no_std` kernels: `cargo llvm-cov` with `--target` flag, requires `llvm-tools-preview` component (already pinned in Phase 02 toolchain).
- Property tests via `proptest` work fine in `no_std` if we vendor a slim subset; for v1.0 sticking to example-based unit tests is sufficient.

## Requirements

**Functional**
- `cargo test --workspace` runs all tests (host + integration)
- ≥80% line coverage on `kernel/src/` core modules (memory, task, cell, loader)
- Integration tests boot QEMU, exercise multi-cell flows, assert via serial output
- Stress tests: 10K allocator cycles, 1000 scheduler ticks, 100K IPC round-trips

**Non-functional**
- Test suite wall-time < 5 min in CI (parallelize where possible)
- Tests deterministic: PRNG seeds fixed, no wall-clock dependencies

## Architecture

```
Test pyramid:
   ┌───────────────────────────────────┐
   │  Integration (QEMU-driven)        │   ~10 tests, ~3 min
   │  - multi_cell, vfs_operations,    │
   │    keyboard_stream, file_handle,  │
   │    spawn_from_path, ring3_smoke   │
   ├───────────────────────────────────┤
   │  Subsystem unit (host or QEMU)    │   ~40 tests, ~30 sec
   │  - allocator, scheduler, ipc,     │
   │    cap_registry, elf_loader,      │
   │    relocation, syscall dispatch   │
   ├───────────────────────────────────┤
   │  Pure-Rust unit (host)            │   ~80 tests, ~5 sec
   │  - VAddr/PAddr arith, ELF parser, │
   │    ring buffers, async primitives,│
   │    libs/types, libs/api type tests│
   └───────────────────────────────────┘
```

## Related Code Files

**Modify / extend:**
- `kernel/src/memory/tests.rs` — add: random alloc/free patterns, fragmentation, 10K stress
- `kernel/src/task/tests.rs` — add: scheduler fairness, preemption, blocked→ready transitions
- `kernel/src/task/ipc_test.rs` — add: timeout on Recv (after Phase 20 lands timeouts), Call/Reply, cap-carrying message
- `cells/apps/test-isolation/src/lib.rs` — extend with capability isolation tests
- `kernel/src/cell/registry.rs` — link to `cap_registry_tests` from Phase 07

**Create:**
- `kernel/src/loader/elf_tests.rs` — feed crafted byte slices, assert correct phdr parsing, BSS zeroing, relocation
- `kernel/src/loader/reloc_tests.rs` — R_RISCV_RELATIVE correctness
- `kernel/src/task/syscall_tests.rs` — dispatch table coverage, malformed args rejected
- `libs/api/src/syscall_tests.rs` — ABI encode/decode (each variant round-trips)
- `libs/types/src/tests.rs` — VAddr/PAddr arithmetic, overflow guards
- `tests/integration/multi_cell.rs` — Init→Config→VFS→Shell boot chain
- `tests/integration/vfs_operations.rs` — file create/read/write/delete (after Phase 13 lands)
- `tests/integration/keyboard_stream.rs` — 6000 events / 60s (overlaps with Phase 05 test)
- `tests/integration/file_handle_ipc.rs` — overlaps with Phase 07 test
- `tests/integration/spawn_from_path.rs` — overlaps with Phase 06 test
- `tests/integration/ring3_smoke.rs` — overlaps with Phase 03 test
- `tests/integration/harness.rs` — shared helper: spawn QEMU, drive stdin, parse serial output, timeout
- `scripts/measure-coverage.sh` — `cargo llvm-cov` orchestrator

## Implementation Steps

1. **Stand up the test harness** `tests/integration/harness.rs`:
   - Function `boot_and_drive(input: &str, timeout: Duration) → SerialLog`
   - Spawns `qemu-system-riscv64 -nographic -chardev pipe,path=…`
   - Writes `input` to the pipe, reads serial until timeout
   - Returns the captured log for assertion
2. **Migrate the partial tests from Phases 03–07** into `tests/integration/` so they live in one place; ensure each still passes.
3. **Memory allocator unit tests** in `kernel/src/memory/tests.rs`:
   - Alloc 1000 frames sequentially, free in reverse — check no fragmentation
   - Random alloc/free with seeded PRNG, 10K iterations
   - Alloc all available frames, verify exhaustion returns clean error
   - Free non-allocated frame returns error (no double-free corruption)
4. **Scheduler unit tests** in `kernel/src/task/tests.rs`:
   - Create N tasks, run scheduler, verify each executed N rounds (round-robin fairness)
   - Mark a task Blocked, verify it never runs until wake_task called
   - Preempt mid-task on timer IRQ; verify resumed with same register state
5. **IPC unit tests** in `kernel/src/task/ipc_test.rs`:
   - Send → Recv basic message
   - Call → Reply with return value
   - Blocking Recv unblocks correctly on Send
   - Capability transfer via message: sender loses, receiver gains
   - 100K round-trip stress test, no leak in cap registry
6. **ELF loader tests** in `kernel/src/loader/elf_tests.rs`:
   - Synthesize minimal ELF header + 1 PT_LOAD; load; assert mapped region readable
   - Synthesize BSS-only segment; assert zero-init
   - Malformed: truncated phdr table → InvalidElf
   - Malformed: PT_LOAD overlapping kernel image → MapError
7. **Relocation tests** in `kernel/src/loader/reloc_tests.rs`:
   - R_RISCV_RELATIVE: offset+addend, verify destination has expected value
   - Unsupported reloc type → returns error, not panic
8. **Syscall ABI tests** in `libs/api/src/syscall_tests.rs`:
   - Each `ViSyscall` variant: encode to register tuple, decode back, assert equal
   - Malformed ABI input → clean error (e.g., bad UTF-8 path)
9. **libs/types tests** in `libs/types/src/tests.rs`:
   - VAddr arithmetic: `+offset`, `-offset`, alignment helpers
   - VAddr overflow: VAddr::MAX + 1 must NOT silently wrap (debug builds panic; release saturates)
   - PAddr ↔ VAddr via HHDM offset round-trip
10. **Coverage**:
    - `scripts/measure-coverage.sh` runs `cargo llvm-cov --workspace --target riscv64gc-unknown-none-elf -Z build-std=core,alloc --html`
    - Inspect `target/llvm-cov/html/index.html`
    - Identify uncovered modules; add tests until ≥80% line coverage on `kernel/src/`
11. **Wire into CI** (extend Phase 02's ci.yml):
    - Add `cargo test --workspace` job (depends on build)
    - Add `cargo llvm-cov` job (informational, doesn't block; uploads to Codecov)
12. **Document**:
    - `docs/10-testing.md` already exists — append section on how to add a new integration test
    - Reference `tests/integration/harness.rs` as the canonical helper

## Todo List

- [ ] Build `tests/integration/harness.rs` (QEMU subprocess driver)
- [ ] Migrate existing phase smoke tests into `tests/integration/`
- [ ] Expand `kernel/src/memory/tests.rs` (4 new cases + 10K stress)
- [ ] Expand `kernel/src/task/tests.rs` (round-robin, blocked, preempt)
- [ ] Expand `kernel/src/task/ipc_test.rs` (call/reply, cap transfer, 100K stress)
- [ ] Create `kernel/src/loader/elf_tests.rs`
- [ ] Create `kernel/src/loader/reloc_tests.rs`
- [ ] Create `libs/api/src/syscall_tests.rs`
- [ ] Create `libs/types/src/tests.rs`
- [ ] Run `cargo llvm-cov` baseline, identify gaps
- [ ] Add tests until coverage ≥80%
- [ ] Add `cargo test` + `cargo llvm-cov` jobs to CI
- [ ] Update `docs/10-testing.md`

## Success Criteria

- `cargo test --workspace` exits 0 in CI
- Line coverage ≥80% on `kernel/src/` modules; ≥70% on `libs/`
- Integration suite covers: boot, ring3, virtio_blk, keyboard, ELF, file handle IPC
- Test wall-time < 5 min in CI
- New `tests/integration/harness.rs` reused by ≥5 test files (DRY check)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Custom test runner in no_std crates introduces maintenance burden | Med | Low | Encapsulate in single helper crate `kernel-test-runner`; reuse across modules |
| QEMU-driven integration tests flaky under CI load | High | Med | Per-test 60s timeout, retry once on transient failure; pin QEMU version |
| Coverage tool misses async branches | Med | Low | Manual review of async-heavy modules; supplement with case-counted assertions |
| Tests block on phase 13/14 features that don't exist yet | Cert | Med | Mark phase-13/14 dependent tests `#[ignore]` until those phases land; CI surfaces as TODOs |
| Async executor test contention with real scheduler | Low | Med | Use a separate executor instance per test; teardown in test guard |

## Security Considerations

- Tests must not bypass the capability system to take shortcuts (a test that uses internal kernel APIs to "fake" a capability gives false confidence)
- Fuzz harnesses (Phase 12) complement these; this phase is for example-based correctness

## Rollback

Tests are inert until invoked; reverting removes tests without affecting runtime. CI's coverage gate (if introduced as required) needs to be lowered or removed in a follow-up PR if a regression must ship.

## Next Steps

Phase 12 adds fuzz + Kani harnesses on top of this base. Phase 19's CI changes upload coverage reports to Codecov. Every subsequent phase MUST include at least one test in this framework.
