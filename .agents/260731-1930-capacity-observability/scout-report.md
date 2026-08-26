# Scout Report — Capacity Observability

## Relevant Files

- `docs/code-standards.md:12` freezes `libs/api/` and `libs/types/`; every change requires two explicit confirmations.
- `kernel/src/task/syscall.rs:364` owns the internal error enum; `kernel/src/task/syscall.rs:4782` collapses every error to all-ones at the register boundary. RV32 has a separate equivalent dispatcher.
- `kernel/src/task/syscall.rs:2416`, `:2575`, `:2633`, and `:3045` map cell-spawn OOM to `Unknown` for SpawnFromPath, SpawnFromElf, SpawnPinned, and SpawnFromMem.
- `kernel/src/loader.rs:190` currently converts every `spawn_from_mem` failure into `ViError::OutOfMemory`; faithful error propagation is a prerequisite.
- `libs/ostd/src/syscall.rs:14` lacks `OutOfMemory`; all four spawn wrappers collapse non-positive returns.
- `kernel/src/memory/frame.rs:31` owns total frames and the allocation bitmap; `used_memory()` at `:206` is a zero-returning stub.
- `libs/api/src/abi/syscall.rs` owns stable opcodes, allowlist bits, and fixed-width wire structs; opcode 243 is explicitly pinned as unknown at `libs/api/src/abi/syscall_tests.rs:116`.
- `cells/tests/bench/src/scenarios/memory_footprint.rs:52` returns the constant 3,500,000; `cells/tests/bench/src/main.rs:14` declares its syscall allowlist.
- `tests/integration/tests/boot.rs:1558` runs the real benchmark suite but gates only on completion.

## Patterns To Follow

- Append stable opcodes and allowlist bits; never renumber. Pin discriminants, layouts, round trips, and collision behavior in API host tests.
- Use fixed-width `u64` wire fields so RV32 and 64-bit targets agree.
- Validate caller buffers before copying and snapshot allocator data under one `FRAME_ALLOCATOR` lock.
- Keep privileged telemetry opt-in. `GetProcs2` bit 55 and its no-implicit-grant assertion are the direct precedent.
- Log allocation failure at the source and summarize it at the syscall boundary; never log inside `GlobalAlloc::alloc` or while holding the heap allocator lock.

## Precedents

- `26a0584e` added `GetProcs2 = 239`, a fixed-width telemetry row, allowlist bit 55, ABI tests, kernel dispatch, ostd wrapper, and explicitly recorded Law 1 confirmation. Its footprint is the A3 checklist.
- `7621a7f6` made spawn-related OOM recoverable and intentionally retained `TryAgain` for thread spawn. A2 must not change that separate contract.
- `49a15348` added a complete syscall path across API, kernel, ostd, cell allowlist, and self-tests; it confirms the cross-layer blast radius.
- `3808e87a` introduced the synthetic memory benchmark; its TODO has never been completed.

## Prior Failures

- No matching entries in `.agents/failure-history.jsonl` or `.agents/incidents/`.
- D5 runtime evidence is the incident analogue: capacity stopped at OOM but userspace saw `Err(Unknown)` and the kernel logged nothing (`.agents/reports/d5-cell-scale-measurement-260731.md:23`).

## Blast Radius

- Stable contract: A2 return encoding; A3 opcode 243, bit 56, and `ViMemInfoV1` layout.
- Kernel: loader error identity, four spawn mappings, both dispatcher widths, allocator accounting, MemInfo pointer validation.
- Userspace: ostd error decoder/wrapper, runtime no-implicit-grant assertion, benchmark allowlist and metric source.
- Verification/docs: API tests, allocator/syscall tests, four target builds, bounded RV64 OOM probe, benchmark runtime, performance/TODO/changelog records.

## Constraints And Technical Debt

- A real allocated-frame reading is not the documented “kernel + 3 services resident footprint”: the allocator excludes the kernel image but includes a 16 MiB eagerly reserved heap. Do not subtract a magic baseline.
- The current shared worktree contains concurrent IPC edits in `kernel/src/task/syscall.rs`, `kernel/src/task.rs`, `kernel/src/main.rs`, and integration tests. Implement with narrow patches after re-reading the live diff.
- Grant allocation and thread-spawn sentinels use different established semantics; redesigning the global syscall error system is out of scope.
