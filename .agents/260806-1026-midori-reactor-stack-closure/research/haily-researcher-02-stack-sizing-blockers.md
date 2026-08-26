## `stack_pages_for` is real, but still a default-only decision point
**Verdict:** Phase08 added reversible plumbing, not production sizing; every current spawn path still gets the 64-page default.
- `STACK_PAGES` remains `64`, and `stack_pages_for(_name)` just returns that constant. `kernel/src/task.rs:41-43` `kernel/src/task.rs:223-225`
- Cell spawn, synthetic spawn, and scheduler-owned stack allocation all route through `stack_pages_for(name)`, so there is one decision point but no per-path divergence yet. `kernel/src/task.rs:871-875` `kernel/src/task.rs:1896-1899` `kernel/src/task/scheduler.rs:210-213` `kernel/src/task/scheduler.rs:357`
- The living docs match the code: Phase08 is recorded as “default-only” and “baseline-only,” not as a shipped stack table. `docs/project-roadmap.md:30-35` `docs/system-architecture.md:1002` `docs/project-changelog.md:18-28`
**Source:** kernel/src/task.rs:41-43,223-225,871-875,1896-1899; kernel/src/task/scheduler.rs:210-213,357; docs/project-roadmap.md:30-35

## The memset-overrun blocker is fixed; the overflow-hardening blocker is not
**Verdict:** The old immediate corruption hazard is gone, but the stronger-overflow-protection prerequisite is still honestly open.
- Both stack-zeroing sites now skip the guard frame and derive their length from the `Stack` instance, not from a global constant; that closes the “shrink stack, still zero 64 pages” corruption class. `kernel/src/task/scheduler.rs:241-259` `kernel/src/task/scheduler.rs:381-392`
- `Stack::allocate()` still provisions one bottom guard page only: `total_pages = pages + 1`, unmaps `base_addr`, and rejects the stack if that guard frame still resolves. There is no second guard page in the allocator contract. `kernel/src/task/stack.rs:55-74` `kernel/src/task/stack.rs:86-87` `kernel/src/task/stack.rs:159-182`
- The reliability spec still says the remaining verification is a deliberate-overflow test cell, which means the guard-page mechanism is accepted but not fully closed as “stronger overflow protection.” `docs/specs/12-reliability.md:111-116`
- The only probe-like logic in tree is test-hooks watermark instrumentation, not a production stack probe or runtime multi-guard policy. `kernel/src/task/stack.rs:220-253` `kernel/src/task/stack.rs:256-270`
**Source:** kernel/src/task/scheduler.rs:241-259,381-392; kernel/src/task/stack.rs:55-74,86-87,159-182,220-270; docs/specs/12-reliability.md:111-116

## The watermark markers are implemented and intentionally non-authoritative
**Verdict:** The test-hooks stack markers are real observability, but they are explicitly baseline telemetry only.
- Under `test-hooks`, the kernel primes stack memory with `0xA5`, scans for the deepest overwritten byte range, and logs `[stack-baseline]` markers with used and allocated bytes. `kernel/src/task/stack.rs:225-253` `kernel/src/task.rs:238-252`
- Emission is limited to named tasks `init`, `shell`, `vfs`, and `vfs-test`; boot-time emission waits for a tick gate, and `vfs-test` is deferred until exit so the marker covers the whole test workload. `kernel/src/task.rs:227-235` `kernel/src/task.rs:255-300`
- The documentation labels those numbers “baseline=non-authoritative” and says they must not be treated as a production stack table. `kernel/src/task.rs:242-252` `docs/project-roadmap.md:30-35` `docs/system-architecture.md:1002` `docs/project-changelog.md:20-27`
**Source:** kernel/src/task.rs:227-300; kernel/src/task/stack.rs:225-253; docs/system-architecture.md:1002

## The measurement blocker is real: there is still no generic parked executor
**Verdict:** Production shrink is correctly blocked because current async measurements would still be distorted by stack-pinned, busy-yield userland futures.
- The async ADR states the stack-sizing work stays blocked until the shim that pins a future on the caller stack is replaced, because it perturbs every watermark measurement. `docs/specs/03b-async-reactor-adr.md:143-145`
- `ostd::executor::block_on()` still pins the future on the caller stack with `Pin::new_unchecked`, uses a dummy waker, and loops on `sys_yield()` when pending. `libs/ostd/src/executor.rs:9-31` `libs/ostd/src/executor.rs:36-44`
- Shell input still intentionally uses `sys_recv_timeout()` so the shell enters `TaskState::Recv`; it is not running on a parked completion-driven executor. `cells/tools/shell/src/async_utils.rs:13-18` `cells/tools/shell/src/async_utils.rs:36-45`
- `WaitCompletion` is still hard-gated to `NET_RX` only, so there is no generic wait substrate for stack-representative async services yet. `kernel/src/task/completion_wait.rs:73-77`
- The only production `WaitCompletion(NET_RX)` consumer is the net service’s narrow RX wait loop, which is not a general executor replacement. `cells/services/net/src/main.rs:173-185`
**Source:** docs/specs/03b-async-reactor-adr.md:143-145; libs/ostd/src/executor.rs:9-31,36-44; kernel/src/task/completion_wait.rs:73-77; cells/tools/shell/src/async_utils.rs:13-18,36-45

## Current blocker statement in the docs is accurate
**Verdict:** The repo’s current status text is consistent with the code: baseline gate closed, production shrink still blocked on generic-wait evidence plus stronger overflow protection.
- Roadmap, changelog, and system architecture all repeat the same boundary: default 64 unchanged, baseline markers passed, shrink blocked on parked-executor or equivalent generic-wait evidence and stronger overflow protection. `docs/project-roadmap.md:30-35` `docs/project-changelog.md:18-28` `docs/system-architecture.md:1002` `docs/system-architecture.md:1055`
- Code agrees with both halves of that statement: baseline-only plumbing/markers exist, while generic wait and stronger overflow protection do not. `kernel/src/task.rs:223-225` `kernel/src/task.rs:227-300` `kernel/src/task/stack.rs:159-182` `kernel/src/task/completion_wait.rs:73-77`
**Source:** docs/project-roadmap.md:30-35; docs/project-changelog.md:18-28; docs/system-architecture.md:1002,1055; kernel/src/task.rs:223-300

## Risks if shrink starts now
**Verdict:** Shrinking stacks before the two blockers clear is medium/high risk and likely to generate false confidence rather than usable evidence.
- Watermark data gathered under `block_on` still includes caller-stack-pinned futures and busy-yield behavior, so any per-path table would be calibrated against the wrong execution model. `docs/specs/03b-async-reactor-adr.md:143-145` `libs/ostd/src/executor.rs:9-31`
- One bottom guard page catches simple downward overflow, but it does not satisfy the repo’s own stronger-overflow-protection requirement or the deliberate-overflow follow-up. `kernel/src/task/stack.rs:159-182` `docs/specs/12-reliability.md:111-116`
- Because Cellos is SAS, a wrong stack reduction is not a contained process crash; the prior memset bug existed precisely because an overrun lands in another cell’s frames with no hardware boundary. `kernel/src/task/scheduler.rs:245-248` `docs/project-changelog.md:571-573`
**Source:** docs/specs/03b-async-reactor-adr.md:143-145; libs/ostd/src/executor.rs:9-31; kernel/src/task/stack.rs:159-182; docs/specs/12-reliability.md:111-116

## Recommended safe sequence
**Verdict:** Rank 1 is to keep Phase08 closed only as a baseline gate; rank 2 is to resume shrink work only after the runtime and overflow blockers are cleared in that order.
- **1. Keep today’s status as-is.** No non-default `stack_pages_for` entries until the measurements become representative. `docs/project-roadmap.md:30-35` `kernel/src/task.rs:223-225`
- **2. Land the runtime-side prerequisite first.** Replace stack-pinned busy-yield `block_on` with a parked executor or equivalent generic-wait path broad enough to represent real async service behavior, not just NET_RX. `docs/specs/03b-async-reactor-adr.md:143-145` `libs/ostd/src/executor.rs:9-31` `kernel/src/task/completion_wait.rs:73-77`
- **3. Land stronger overflow protection second.** Minimum honest closure is better than today’s single verified bottom guard plus follow-up note; that means either the planned extra hardening mechanism or explicit evidence that the chosen replacement closes the remaining deliberate-overflow concern. `kernel/src/task/stack.rs:159-182` `docs/specs/12-reliability.md:111-116`
- **4. Only then re-measure and size.** Re-run the test-hooks baselines on the post-shim runtime, add conservative per-path entries, and keep unmeasured paths at 64 pages. `kernel/src/task.rs:238-300` `docs/project-changelog.md:20-27`
**Source:** docs/project-roadmap.md:30-35; docs/specs/03b-async-reactor-adr.md:143-145; kernel/src/task/completion_wait.rs:73-77; docs/specs/12-reliability.md:111-116
