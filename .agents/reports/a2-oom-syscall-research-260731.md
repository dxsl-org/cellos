# A2 research: typed syscall OOM and allocation-failure evidence

**Date:** 2026-07-31  
**Scope:** planning only; no production edits  
**Decision:** A2 from `.agents/reports/decision-docket-260730.md:24`

## Verdict

A2 is small only if it is kept to the four cell-spawn syscalls. Add an additive
`OutOfMemory` return code, decode it in the four ostd spawn wrappers, preserve the
intentional thread-spawn `TryAgain`, and emit one path/caller summary plus a
stage-specific allocation log. Do not log from `GlobalAlloc::alloc`: logging may
allocate and recurse inside the allocator.

The implementation must first remove one error-destroying conversion in the loader.
`spawn_gated` currently converts every `spawn_from_mem` failure to `OutOfMemory`
(`kernel/src/loader.rs:190-192`). Exposing OOM before fixing that line would falsely
report malformed ELF, W^X/VA denial, and relocation failures as memory exhaustion.

## Existing contract

### Opcode and ABI ownership

- No new syscall opcode is required. The affected shipped opcodes are
  `SpawnFromMem=10`, `SpawnFromPath=12`, `SpawnPinned=16`, and
  `SpawnFromElf=238` (`libs/api/src/abi/syscall.rs:40-43`,
  `libs/api/src/abi/syscall.rs:121-134`). Their numeric values must remain unchanged.
- Opcode stability already has host tests and an explicit no-renumber rule
  (`libs/api/src/abi/syscall_tests.rs:76-96`).
- `libs/api/` and `libs/types/` are frozen ABI surfaces requiring two explicit user
  confirmations (`docs/code-standards.md:12-16`). A2 can avoid changing either one:
  `types::ViError::OutOfMemory` already exists (`libs/types/src/lib.rs:107-118`), and
  no opcode or public wire struct needs to change.

### Current error encoding

- The kernel has a rich internal `SyscallError` enum, but it is not represented at
  the register boundary (`kernel/src/task/syscall.rs:360-374`).
- All non-RV32 errors become `usize::MAX` (`-1` as `isize`) regardless of variant
  (`kernel/src/task/syscall.rs:4782-4785`). RV32 independently does the same with
  `u32::MAX` (`kernel/src/task/syscall.rs:4838-4841`). Both dispatchers must change
  together.
- ostd has a separate, userspace-only `SyscallError` enum and no `OutOfMemory`
  variant (`libs/ostd/src/syscall.rs:7-22`). Each spawn wrapper currently accepts a
  positive TID and collapses every non-positive value to `Unknown`:
  `SpawnFromMem` at `libs/ostd/src/syscall.rs:245-267`, `SpawnFromPath` at
  `libs/ostd/src/syscall.rs:298-310`, `SpawnFromElf` at
  `libs/ostd/src/syscall.rs:321-336`, and `SpawnPinned` at
  `libs/ostd/src/syscall.rs:361-375`.
- Therefore adding only a kernel enum variant is not an ABI fix. The return register
  needs an additive encoding that ostd decodes.

### Recommended additive encoding

Reserve `-2` (`usize::MAX - 1`, or `u32::MAX - 1` on RV32) for OOM and retain `-1`
for every legacy/generic error. This is backward compatible in both directions:

- an old cell on a new kernel still treats `-2` as `Unknown` because its spawn
  wrapper rejects every non-positive result;
- a new cell on an old kernel receives `-1` and still reports `Unknown`;
- successful spawn results are positive TIDs, so `-2` cannot collide with success.

Append `OutOfMemory` to both internal enums rather than inserting it before existing
variants. Add one kernel result-encoding helper used by both dispatchers and one ostd
spawn-result decoder used by all four wrappers. Keep the constants private to these
two modules unless the user separately confirms a frozen `libs/api` change.

## Mapping sites and semantic boundary

The exact `ViError::OutOfMemory -> Unknown` sites are:

- `SpawnFromPath`: `kernel/src/task/syscall.rs:2416-2420`
- `SpawnFromElf`: `kernel/src/task/syscall.rs:2575-2580`
- `SpawnPinned`: `kernel/src/task/syscall.rs:2633-2637`
- `SpawnFromMem`: `kernel/src/task/syscall.rs:3045-3049`

Map those four to the new syscall OOM variant. Do not change thread creation:
`Syscall::Spawn` intentionally maps an unaffordable stack, thread cap, or temporary
contiguous-run failure to `TryAgain` (`kernel/src/task/syscall.rs:1733-1741`). Commit
`7621a7f6` explicitly chose EAGAIN semantics because that resource may exist later.
The phase-08 report repeats the contract and has boot self-test evidence
(`.agents/reports/phase-08-stack-safety-260731.md:44-45`,
`.agents/reports/phase-08-stack-safety-260731.md:80-83`).

Also leave Grant allocation's documented `0` OOM sentinel out of A2
(`libs/api/src/abi/syscall.rs:212-216`, `libs/api/src/abi/syscall.rs:243-246`).
Changing all syscall result conventions is a separate ABI migration.

## Allocation paths and useful log data

### Cell-spawn path

`spawn_from_path/spawn_gated -> task::spawn_from_mem` is the common path
(`kernel/src/loader.rs:78-100`, `kernel/src/loader.rs:183-192`). The recoverable
allocation failures are:

1. PIE VA-slot exhaustion, already logged as `Spawn: cell VA space exhausted`
   (`kernel/src/task.rs:699-705`).
2. ELF segment frame exhaustion or page-table allocation failure
   (`kernel/src/loader/elf.rs:213-232`). Neither branch currently logs the failed
   page, requested frame, or number of pages already mapped.
3. Kernel/user stack contiguous-run failure. Each stack needs
   `STACK_PAGES + 1` contiguous frames because SAS has no separate VA allocator
   (`kernel/src/task/stack.rs:83-101`). Only the later page-map failure logs today
   (`kernel/src/task/stack.rs:133-140`); failure to find the contiguous run is silent.
4. An unavailable global frame allocator (`kernel/src/task.rs:712-716`,
   `kernel/src/task/stack.rs:83-87`). This is an initialization/invariant failure,
   not ordinary capacity pressure, and should be logged distinctly.

Recommended logging, at `warn` or `error` according to the existing local style:

- At the source: log the stage and concrete request without allocating. Segment
  allocation can report target VA and pages already mapped; stack allocation can
  report kernel/user, requested contiguous pages, and bytes.
- At the syscall boundary: one summary with operation, caller TID, path/name, and
  ELF length where available. This is the line a capacity test and an operator can
  correlate with the caller.
- Do not promise free-frame counts in A2. The allocator tracks `total_frames` and a
  bitmap but exposes no free count (`kernel/src/memory/frame.rs:44-68`,
  `kernel/src/memory/frame.rs:119-138`); that belongs to A3/MemInfo.

### Heap allocation is a different failure class

`QuotaAlloc::alloc` returns null on quota denial or heap OOM
(`kernel/src/memory/heap.rs:17-29`), and the allocation-error handler already logs
the `Layout` then waits forever (`kernel/src/memory/heap.rs:54-69`). Calling `log!`
inside `GlobalAlloc::alloc` is unsafe because formatting/log infrastructure may
allocate and recurse. A2 should not instrument that hot path or claim to make every
kernel heap allocation recoverable. Its acceptance wording should say "failed
cell-spawn frame/stack allocation".

## Loader correction required before exposure

Replace the blanket conversion at `kernel/src/loader.rs:190-192` with faithful error
propagation. `spawn_from_mem` can return at least:

- `InvalidInput` for malformed ELF (`kernel/src/task.rs:671-697`);
- `PermissionDenied` for VA/W^X violations through `load_segments`
  (`kernel/src/loader/elf.rs:55-60`);
- relocation/W^X enforcement errors (`kernel/src/task.rs:742-761`);
- genuine `OutOfMemory` for VA slots, frames, page tables, or stacks.

Without this correction, the new typed ABI would be observably wrong and could
cause callers to retry permanent security/input failures as if they were transient
capacity failures.

## Wrapper behavior risk

`ostd::sys_spawn_from_path` reads via VFS, calls `SpawnFromElf`, and falls back to
the bootstrap `SpawnFromPath` syscall after *any* spawn failure
(`libs/ostd/src/syscall.rs:278-309`). Once OOM is typed, it should return OOM
immediately rather than perform a second spawn attempt. Retrying the same image via
the bootstrap path adds allocator pressure and produces duplicate logs. Preserve
fallback for VFS read/routing failure and legacy generic errors unless a broader
loader-routing change is explicitly accepted.

## Test precedents and proposed gate

Existing precedents:

- Direct kernel self-tests call the real `handle_syscall` path and assert typed
  variants for thread-cap/quota refusal
  (`kernel/src/task/thread_cap_selftest.rs:256-290`,
  `kernel/src/task/thread_quota_selftest.rs:128-179`).
- Syscall opcode/ABI contracts have host tests in
  `libs/api/src/abi/syscall_tests.rs:64-117`.
- The measured D5 experiment already proved the real symptom: parked
  `bench-probe` cells stop at OOM but userspace prints `Err(Unknown)` and the kernel
  emits no diagnostic (`.agents/reports/d5-cell-scale-measurement-260731.md:23-43`).
- The integration harness can select RV64 RAM for the minimal lane
  (`tests/integration/src/lib.rs:298-327`), but the disk-backed boot constructor has
  no memory parameter yet (`tests/integration/src/lib.rs:243-274`).

Recommended verification matrix:

1. Unit-test kernel encoding: OOM -> `-2`; all existing errors -> `-1`; success is
   unchanged. Exercise both pointer-width expressions without architecture-specific
   literals.
2. Unit-test the private ostd spawn decoder: positive TID, `-1` Unknown, `-2` OOM.
3. Add a real RV64 integration probe based on D5's parked `bench-probe` method. Boot
   with a bounded disk-backed memory size, spawn until refusal, and require:
   `Err(OutOfMemory)`, the allocation-stage log, the caller/path summary, no panic or
   fault, and a responsive shell afterward.
4. Re-run normal shell/bench boot to prove successful positive-TID decoding and VFS
   spawn routing are unchanged.
5. Compile RV64, RV32, AArch64, and x86_64 because both dispatcher widths and three
   assembly return-register ABIs are involved.

## Implementation file set

Minimum production set:

- `kernel/src/task/syscall.rs`: append internal variant, map four spawn sites, encode
  OOM in both dispatchers, log caller/path summary.
- `kernel/src/loader.rs`: preserve `spawn_from_mem` error identity.
- `kernel/src/loader/elf.rs`: log segment allocation failure at source.
- `kernel/src/task/stack.rs`: log silent contiguous-run failure at source.
- `libs/ostd/src/syscall.rs`: append userspace variant, shared spawn decoder, stop
  fallback on typed OOM.

Tests likely touch `tests/integration/src/lib.rs`, one integration test file, and a
small bench/probe command path. No `libs/api/` or `libs/types/` edit is required for
the minimal additive design.

## Risks

- **False OOM:** highest risk; caused by the blanket loader conversion. Fix first.
- **ABI drift across widths:** update both dispatchers and compile RV32 as well as
  64-bit targets.
- **Fallback masks OOM:** VFS spawn currently retries through bootstrap.
- **Log amplification:** a caller can loop failed spawns. Emit one summary per syscall
  plus one source-stage line; avoid per-frame scan logging.
- **Allocator recursion:** never log inside `GlobalAlloc::alloc` or while holding the
  linked-list allocator lock.
- **Scope creep:** do not redesign every syscall error, Grant sentinels, MemInfo, or
  global heap recovery under A2.
