# Actor and supervisor library

## Requirements

- `ostd::actor`: an actor loop over the App SDK `0xAC` envelope with typed postcard dispatch, a
  masked typed call/reply, a lifecycle (`on_start`/`on_tick`/`on_shutdown`), and loud handling of
  traffic the actor has no arm for (Spec 17 §7).
- `ostd::actor::supervisor`: a **dynamic** child table with `Policy` (Permanent/Transient/Temporary),
  per-child intensity and window, capped exponential backoff, and `Strategy`
  (one_for_one/one_for_all/rest_for_one).
- Restart semantics must match the ones `/bin/init` already proved (Spec 12 §4.3), including
  "give up on that child only" and the check-before-increment budget order.
- No `libs/api` change, no new syscall, no new wire byte (ADR-0021).

## Result

- `libs/ostd/src/actor/mod.rs` — `Actor`, `ActorCtx` (send/reply/call/spawn/watch/force_exit/lookup/
  `now_ticks`/exit), `exit_reason`, `run`. The loop is `AppContext::run_with_timeout` with a 5-tick
  (≈50 ms) deadline, so backoff timers are the actor's `on_tick` and the SDK's lifecycle events keep
  arriving on the same mailbox.
- `libs/ostd/src/actor/supervisor.rs` — `ChildSpec`/`Child`/`Decision`/`Tree`/`Backoff`/`Policy`/
  `Strategy`, with the syscall-free decisions taking `now` as a parameter so they are host-testable.
- `libs/ostd/src/lib.rs` — `pub mod actor;`, plus `#![cfg_attr(not(test), no_std)]` (same pattern as
  `libs/api/src/lib.rs`) so the decision logic can be unit-tested on the host.
- Host tests: `cargo test -p ostd --target x86_64-unknown-linux-gnu --lib actor::` → **8 passed**
  (policy matrix, Transient on abnormal exit, Transient on clean exit, intensity give-up on the sixth
  exit, window rollover, backoff growth/cap, backoff deferral, strategy scope for all three
  strategies).

### Semantics matched to `init` by construction

`init` checks `restart_count >= MAX_RESTARTS_PER_WINDOW` **before** incrementing, so five restarts
are allowed and the sixth abnormal exit gives up. The first implementation incremented first, which
took the fifth restart away; the failing unit test caught it and the order now matches `init`
(`Decision::GiveUp` with the default intensity of 5, verified by
`intensity_gives_up_on_the_sixth_abnormal_exit_inside_one_window`).

## Risk assessment

- Rollback is deleting the two new modules and the `pub mod actor;` line; nothing else depends on
  them and no ABI changed.
- The library is bound by kernel IPC semantics it cannot change: one recv consumer per tid
  (Spec 17 §10.6) means a reply is a **blocking** send and callers must use `ActorCtx::call` (or a
  long enough `recv_timeout`). Intra-cell concurrency is B1's reactor, not this phase's.
- `one_for_all`/`rest_for_one` reuse the same sibling-termination path; `one_for_all` is witnessed
  end to end in phase 03, `rest_for_one` is covered by the scope unit test only.
