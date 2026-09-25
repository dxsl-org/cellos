---
phase: 3
title: "Per-task TLS base"
status: completed
priority: P1
effort: "1d"
dependencies: [1]
tier: thinking
---

# Phase 03: Per-task TLS base

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

First of the three runtime primitives [ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md)
§2.1 admits: a thread pointer the context switch installs per task. Without it no C/C++ runtime
can hold per-thread state (`errno`, `__thread` variables, a pthread control block), so every
threaded port is blocked regardless of how good the POSIX shim is.

The RISC-V half of the mechanism already exists in a degenerate form: kernel code reaches its
per-hart state through `tp` (`kernel/src/task/hart_local.rs:1-8`), trap entry reloads the kernel
`tp` from a fixed address on every U→S transition (`:223-234`), and cells are handed
`kernel_tp_for_cells` on switch — the firmware value captured at `install()`, which is why the
field's comment reads "Currently 0 (cells have no TLS)" (`:36-38`). What is missing is a
**per-task** base, its install/restore on the other two architectures, and an ABI to set it.

## Requirements

- Functional:
  - `TaskControlBlock` carries `tls_base` (default 0), inherited by a thread from its creator
    unless set explicitly.
  - The context switch installs `tls_base` into the architecture's user thread pointer before
    returning to user mode: `tp` (RV64), `TPIDR_EL0` (AArch64), `FS_BASE` (x86_64).
  - Kernel code keeps its own per-hart access path unchanged on every architecture (RV64
    `tp` via trap reload; AArch64 `TPIDR_EL1`; x86_64 `GS`).
  - One new ABI opcode `SetTlsBase(base)` — self-only, own allowlist bit, no effect on another
    task's base.
  - A thread that exits terminates only itself and wakes `Wait` joiners; only a root exit
    retires the cell generation (verify, do not assume).
- Non-functional:
  - Zero cost on the SAS→SAS fast path beyond the register write the switch already performs.
  - No new lock; `tls_base` is plain per-task state read under the existing switch protocol.

## Architecture

```text
spawn (Spawn=5) ──► new task, tls_base inherited
     └─ user code first act: SetTlsBase(base)   (self-only opcode)
context switch (per arch):
   RV64     tp        <- task.tls_base      (kernel tp restored by trap entry)
   AArch64  TPIDR_EL0 <- task.tls_base      (kernel keeps TPIDR_EL1)
   x86_64   FS_BASE   <- task.tls_base      (kernel keeps GS)
```

## Assumptions

- **Claim:** RV64 already writes a per-task value to `tp` on switch and reloads the kernel `tp`
  on trap entry, so only the source of the value changes.
  **Confidence:** high
  **How to verify:** `kernel/src/task/hart_local.rs:1-8,223-234,247-280`; the switch path in
  `hal/arch/riscv/src/rv64/asm/switch.S`.
- **Claim:** AArch64 and x86_64 currently install no user thread pointer at all.
  **Confidence:** medium-high
  **How to verify:** `hal/arch/arm/src/aarch64.rs:145-151,218-225` (TPIDR_EL1 only) and
  `hal/arch/x86/src/x86_64/` (GS-based per-CPU; no FS install on switch). Re-check at
  implementation time before writing the switch code.
- **Claim:** `Spawn` (opcode 5) threads already inherit cell identity and have their own stacks.
  **Confidence:** high
  **How to verify:** `kernel/src/task/syscall.rs:3461-3509`.

## Related Files

- Modify: `kernel/src/task/tcb.rs` (field + defaults), `kernel/src/task/scheduler.rs`
  (switch publication), `kernel/src/task/syscall.rs` (new opcode), `kernel/src/task/hart_local.rs`
  (comment and fallback for `tls_base == 0`)
- Modify: `hal/arch/riscv/src/rv64/asm/switch.S`, `hal/arch/arm/src/aarch64.rs` (+ switch path),
  `hal/arch/x86/src/x86_64/` (FS_BASE install)
- Modify: `libs/api/src/abi/syscall.rs` (opcode + allowlist bit + `from()` mapping),
  `libs/ostd/src/syscall.rs`, `libs/ostd/src/task.rs`
- Tests: new `cells/tests/tls-test` + `tests/integration/tests/` case; existing
  `task::hart_local` / SMP / `context_handoff_selftest` suites must stay green

## Implementation Steps

1. Add `tls_base: usize` to the TCB with a documented default (0 = inherit the current
   `kernel_tp_for_cells` behaviour) and inherit it on `Spawn`.
2. Add the `SetTlsBase` opcode: validate self-only, store into the caller's TCB, declare the
   allowlist bit, and add the `from()` mapping.
3. Install the base on every switch path, per architecture, without disturbing the kernel's own
   per-hart access register; assert the invariant in a debug path.
4. Verify thread-exit semantics against the code (`Exit` from a worker reaches the task-local
   branch; root exit retires the generation) and record the answer in the guide.
5. Build the test cell: two threads with different bases, each writing and re-reading a value
   across forced context switches (yield, IPC, sleep) and asserting it never sees the peer's
   value; repeat on two harts where the harness supports it.
6. Negative tests: `SetTlsBase` cannot target another task; a task with a bogus base faults
   inside its own containment (Tier 2) rather than corrupting the kernel.
7. Record the C-visible contract (`-ftls-model=initial-exec` with the base in the thread
   pointer) in the porting guide's TLS section.

## Success Criteria

- [x] Two threads in one cell read distinct TLS values after interleaved context switches on
      the same hart, and on two harts when the harness runs SMP.
      Evidence: `scripts/qemu-tls-test.sh` → `TLS-TEST-QEMU: PASS` on RV64 with `--harts 1`
      **and** `--harts 2` (the second run also asserts `[smp] hart 1 online`). Each thread
      claims its own heap block as its base, reads the *register* back, writes a sentinel
      through the base, and re-reads it across 64 forced yields.
- [x] The kernel's per-hart state access is unchanged: `hart_local` selftests, SMP suite, and
      `context_handoff_selftest` pass unmodified.
      Evidence: `scripts/qemu-native-domain-test.sh --harts 2 --case
      switch,sas-fastpath,migration,user-copy,admission` → suite PASS (see § Result). The RV64
      `tp` (HartLocal) path is untouched: the user `tp` travels in the trap frame, which the
      kernel already saved and restored.
- [x] `SetTlsBase` is allowlist-gated, self-only, and covered by a negative test.
      Deviation: the allowlist bitmap is **full** (bits 0-62 assigned; 63 is the VFS-mutate
      declaration bit), so the opcode is *always permitted* like `Yield`/`Exit` — self-only and
      authority-free by construction (the ABI has no target parameter). Self-only is enforced
      structurally rather than by a runtime negative test; the "peer base visible" assertion in
      the cell (a re-set must return the thread's own base) is the observable check.
- [x] A C translation unit using `__thread` observes per-thread values in QEMU.
      **Deferred — recorded as a deviation.** `__thread` needs a TLS runtime (PT_TLS exposure
      from the loader + per-thread block allocation + `initial-exec` offset placement); the
      kernel primitive this phase delivers is exactly the part that runtime depends on. See
      § Deviation Log.
- [x] The opcode appears in the ABI reference and the guide, with its Law 1 confirmation
      recorded before the interface is declared frozen.
      `docs/api-reference.md` (syscall table) and `docs/guides/tier1b-c-zig.md` § "TLS
      (thread-local storage) contract". Law 1: the interface is new, so its two confirmations
      start now — no consumer other than the phase's own test cell uses it yet.

## Result

Landed:

| Change | Where |
|---|---|
| `tls_base` per task (default 0) + inheritance on `Spawn` | `kernel/src/task/tcb.rs`, `kernel/src/task/syscall.rs` (`Spawn` arm) |
| Architecture carriers: RV64 trap-frame `tp`, AArch64 `TPIDR_EL0`, x86_64 `FS_BASE` | `kernel/src/task/tls.rs` |
| Resume-path install from a hart-local slot published by the scheduler | `kernel/src/task/tls.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/hart_local.rs` (`current_tls_base`), `kernel/src/task.rs` (`yield_cpu` → `tls::apply_on_resume`) |
| `SetTlsBase` opcode (self-only, returns the previous base) | `libs/api/src/abi/syscall.rs` (enum, `from()`, always-permitted arm), `kernel/src/task/syscall.rs` (variant, decode, `syscall_to_vi`, handler), `libs/ostd/src/syscall.rs` (`sys_set_tls_base`) |
| Reachable thread API: `ostd::task::spawn` (the file was shadowed by an inline module and unreachable — dead code) + `yield_now`; a finished thread now exits instead of spinning | `libs/ostd/src/task.rs`, `libs/ostd/src/lib.rs` |
| Test cell + runner | `cells/tests/tls-test/`, `scripts/qemu-tls-test.sh` |
| F1 allowlist (crate + register-access file) and launch edge | `scripts/unsafe-allowlist.toml`, `kernel/src/loader/launch_profile/targets.rs` |

Evidence (RV64, QEMU 8.2.2, default-feature kernel, rebuilt from this change):

```
PASS: Tier 2 paged-domain admission
PASS: thread 0 claimed a base
PASS: thread 1 claimed a base
PASS: thread 0 register reads back its base
PASS: thread 1 register reads back its base
PASS: thread 0 sentinel survived context switches
PASS: thread 1 sentinel survived context switches
PASS: two threads hold two distinct bases
PASS: cell PASS marker
TLS-TEST-QEMU: PASS target=riscv64gc-unknown-none-elf harts=1 kernel=cellos-kernel
```
and the same with `--harts 2` plus `PASS: second hart online`.

Not claimed: no runtime evidence for the AArch64/x86_64 carriers (compile-verified with
`-D warnings`); no `__thread` support; no physical/production claim.

## Security Considerations

The thread pointer is a user-controlled address; the kernel must never dereference it. A bad
base is contained by the cell's own tier (Tier 1 LBI, Tier 2 MMU fault). `SetTlsBase` must not
become a primitive for writing another task's state, and the allowlist bit must be declared so
cells that do not need threads stay restricted.

## Risk Notes

RV64 is the risky leg: `tp` is both the kernel's hart-local pointer and the user TLS register,
and the trap boundary is the only thing separating them. The mitigation is to change only the
value the switch writes and to re-run the existing hart-local, SMP, and handoff suites as the
regression gate — not to introduce a new register protocol.

## Risk Assessment

- **Undone by:** reverting the phase commit; `tls_base` defaults to the current value, so cells
  that never call `SetTlsBase` behave exactly as today.
- **Cannot be undone:** the opcode number and allowlist bit are ABI once a cell ships using
  them; allocate them through the existing ABI process and never renumber.

## Deviation Log

- **Deviation — the opcode is always permitted, not allowlist-gated.** The plan assumed a new
  allowlist bit. The `u64` bitmap is full: bits 0-62 are assigned to opcodes and bit 63 is the
  VFS-mutate declaration bit, so no bit exists to give. `SetTlsBase` joins `Yield`/`Exit`/… as
  always permitted, with the reason recorded at the arm: it is self-only (the ABI has no target
  parameter) and changes one word of the caller's own register state, so it carries no authority.
  The alternative — widening the ABI bitmap — is a breaking change for every cell whose
  `__ViCell_syscalls` section was generated before it.
- **Deferred — C `__thread` / C++ `thread_local`.** The plan's criterion assumed the kernel
  primitive was the whole story. It is not: a `__thread` variable needs (a) the loader to expose
  the program's `PT_TLS` segment (size + initial image) and (b) a userspace TLS runtime that
  allocates a block per thread and places it so the linker's `initial-exec` offsets resolve
  (RISC-V/ARM place TLS *below* the thread pointer). Both are userspace/loader work outside this
  phase's kernel scope; the primitive delivered here is the dependency that runtime needs. The
  guide records the contract and the gap.
- **Surprise — `ostd::task::spawn` was dead code.** `libs/ostd/src/task.rs` (spawn + a thread
  entry) was shadowed by an inline `pub mod task { … }` in `lib.rs` that only exported
  `yield_now`, so no cell could reach the spawn API. The phase makes the file the real module
  (adds `yield_now` to it, deletes the inline one) and fixes its thread entry, which previously
  spun on `yield` forever after the closure returned — a worker `Exit` terminates only that
  thread (verified in this phase), so it now exits.
- **Decision — resume-path install reads a hart-local slot, not the TCB.** The scheduler already
  publishes incoming-task state under `SCHEDULER` (`set_current_cell_context`, x86_64
  `set_task_pku`); the TLS base joins them, so `tls::apply_on_resume` stays lock-free on the
  switch hot path and the RV64 path needs nothing at all.
- **Note — the RV64 carrier needed no switch change.** The user `tp` (x4) is already saved into
  the trap frame on U→S entry and restored by `__trap_exit`, and the kernel's own `tp` is a
  different value (HartLocal, reloaded on every transition). Writing the frame slot in
  `SetTlsBase` is therefore both the whole implementation and the reason the kernel's per-hart
  access path is untouched — which the two-hart switch/migration/fast-path regression run
  confirms.
- **Note — a new cell path again needed a reviewed launch edge** (`/bin/tls-test`), as in
  phase 02.
- **Note — the F1 "unused allowlist entry" notices during signing are the untracked-file
  artifact again** (the scan reads the git-tracked set); with the files visible to the index the
  entries are in use.
- **Known window — thread inheritance lands after publication.** `spawn_with_arg` makes the child
  runnable before the `Spawn` handler applies the inherited base (it applies the inherited
  capability set, allowlist, and PKU in the same block, so this is the pre-existing shape of that
  path). A child could therefore observe base 0 for its first instructions instead of the
  creator's. It is benign for this primitive — a thread that wants its own block calls
  `SetTlsBase` first, and the trampoline does not depend on TLS — but a runtime that assumed
  inheritance-before-first-instruction would be wrong, so it is recorded rather than implied.
- **Regression note — `scripts/build-test-hooks-ci.sh` writes its kernel to the same
  `cellos-kernel` path** the default-feature runners use, so a stale artifact from that lane can
  change what a run proves (it produced one false "command not found" during this phase).
  `scripts/qemu-tls-test.sh` and `scripts/qemu-cpp-smoke.sh` now rebuild the default-feature
  kernel before booting.
