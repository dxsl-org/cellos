**VERDICT:** PASS_WITH_RISK — APPROVE for Phase00 code landing; prior HIGH findings are fixed, with runtime E2E still blocked by pre-existing packaging.

[MED]      cells/services/supervisor/src/hotswap.rs:151 — supervisor hotswap still hard-freezes before `Snapshot`; `ipc_send` queues messages to `TaskState::Frozen` tasks at kernel/src/task.rs:1363, so stateful snapshot handlers cannot run and the timeout path at cells/services/supervisor/src/hotswap.rs:161 cold-starts the replacement. This is pre-existing and acceptable for Phase00 cap-ceiling code-complete, but remains a release blocker for state-preserving supervisor hotswap claims.
[POSITIVE] kernel/src/cell/hotswap.rs:193 — `freeze_task_with_ceiling` now publishes the Frozen state and swap ceiling while holding `SCHEDULER -> SWAP_CEILINGS`, closing the resume gap from the prior review.
[POSITIVE] kernel/src/cell/hotswap.rs:125 — `take_frozen_replacement_ceiling` holds the same lock order, rechecks that `old_tid` is live and `TaskState::Frozen`, then removes the one-shot ceiling token.
[POSITIVE] kernel/src/task/scheduler.rs:487 — `Scheduler::exit_task` clears the swap ceiling in the shared death funnel before the task disappears, covering clean exit, fault, watchdog, and hotswap retirement paths.
[POSITIVE] kernel/src/cell/hotswap.rs:215 — `unfreeze_task` now follows `SCHEDULER -> SWAP_CEILINGS` and clears the record before requeueing the old task.
[POSITIVE] kernel/src/task/syscall.rs:3677 — `SpawnReplacement` now runs the exact launch-edge authorization for `LaunchRoute::Path` and intersects `profile.parent_ceiling` with the frozen task ceiling before calling the loader.
[POSITIVE] libs/api/src/abi/syscall.rs:717 — `SpawnReplacement` remains isolated on allowlist bit 57, preserving backward compatibility for older supervisor-op bit 49 allowlists.

Runtime boundary: QEMU SpawnReplacement E2E remaining unverified is acceptable as a documented code-complete boundary because the blocker is pre-existing gen_disk packaging; it is not acceptable as release evidence for end-to-end state-preserving hotswap.
