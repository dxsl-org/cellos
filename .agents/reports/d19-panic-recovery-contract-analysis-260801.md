# D19 — Panic recovery: unwind boundary or process-style termination?

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

`catch_unwind` is not the Cellos recovery model:

- Workspace dev and release profiles use `panic = "abort"` (`Cargo.toml:163-167`).
- A cell-linked `ostd` panic logs and exits through `sys_exit(1)`
  (`libs/ostd/src/startup.rs:120-133`).
- Hardware faults and kernel-observed cell panics converge on
  `terminate_current_cell_on_fault` (`kernel/src/task.rs:348` and
  `kernel/src/main.rs:870-922`).
- `Scheduler::exit_task` delivers lifecycle notification; init restarts a service only
  according to its permanent/transient/temporary supervisor policy.

Spec 01 §5's `catch_unwind`, `Poisoned`, automatic hardware reset, and hot re-linking are
not one mechanism and should not be promised as an atomic panic path. Spec 12
`12-reliability.md:95-99` already identifies the mismatch, but its “no restart” sentence
is now itself stale because supervision has shipped.

## Recommended ruling [FINAL]

**Approve recommendation A: replace unwind recovery with terminate-and-supervise.**

1. Rewrite Spec 01 §5 as: panic/fault -> terminate task -> reap resources -> emit death
   notification -> supervisor policy decides restart.
2. Remove `catch_unwind` requirements from Spec 10 §3 and 00-fork §C.
3. State that hardware reset is driver/device-specific recovery work, not a kernel panic
   guarantee.
4. Correct Spec 12's stale “no restart” statement and “panic caught” wording.
5. Fault injection should assert termination, cleanup, notification, policy-controlled
   restart, and kernel survival; it should not test unwinding across a cell boundary.

No runtime or ABI change is required for this ruling.
