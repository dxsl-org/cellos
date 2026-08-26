# Scout Report — Parallel Midori Closure

## Verdict

Accept two bounded implementation streams: A runtime verification and B `/bin/vfs` region fold c-e. Reject C for implementation now; keep it as a readiness audit because CQ exists but no async IPC submission currently registers target dependencies. Reject spawn-broker/shell deprivilege for this parallel batch because it starts new service-ID/broker work.

## Candidate Validation

- A is ready: Phase 02 is marked code-done/runtime-unverified in `.agents/260727-2101-midori-lessons-cellos/plan.md`, and VFS read gates exist in `cells/services/vfs/src/dispatch.rs:55`, `:72`, `:81`, `:161`, `:178`, `:209`.
- B is ready: masks and boot ceiling already cover bit 3 (`kernel/src/policy.rs:68-69`, `scripts/sign-policy.py:57-58`, `kernel/src/loader/boot_ceiling.rs:79-82`); remaining blocker is still visible as `/bin/vfs` policy `0b111` and loader raw grant (`scripts/sign-policy.py:82`, `kernel/src/loader.rs:343-350`).
- C is not implementation-ready: CQ exists and is kernel-owned (`kernel/src/task/completion.rs:1-9`), but `completion_wait` accepts only `NET_RX` (`kernel/src/task/completion_wait.rs:55`) and no async IPC submission records a target-tid dependency. `exit_task` still wakes only `Sending` and `Wait(tid)` waiters (`kernel/src/task/scheduler.rs:512-540`).

## Non-Conflict Check

- A owns tests/evidence only.
- B owns loader/cap/policy/sign-policy.
- C owns reports only; scheduler/completion internals are read-only in this batch.
- Shared merge conflict risk is low; shared final runtime tests are mandatory because B and C both affect boot/runtime behavior.

## Rejected Scope

- Spawn-broker, shell manifest deprivilege, service ID allocation, and broker allowlist are excluded. They touch `libs/api/src/abi/syscall.rs`, `cells/tools/init/src/main.rs`, `cells/tools/shell/*`, and a new service cell, which would widen the batch beyond 2-3 safe streams.
