# Beam-parity backend — B0: actor & supervisor library

**Status**: Active. B0 of `docs/roadmap/beam-parity-backend-roadmap.md` §5.
**Evidence ceiling**: `qemu` (RV64). No physical, fleet, or production claim.

## Decision

An application declares its own supervision tree through an in-tree userspace library instead of
editing `/bin/init`: `ostd::actor` (typed mailbox loop + call/reply + lifecycle) and
`ostd::actor::supervisor` (dynamic child table, restart policy, intensity, backoff, strategies).
No `libs/api` change, no new syscall, no new wire byte — the design and its rejected alternatives
are recorded in [ADR-0021](../../docs/decisions/0021-actor-supervisor-library-in-userspace.md).

Why it is B0 and first: OTP-style recovery already works, but only inside `init`'s static table;
the roadmap's critical path for a single-node backend starts here and this phase depends on
nothing else.

## Acceptance criteria

1. A demo cell pair (`backend-supervisor` + `backend-worker`) runs one supervisor over **three**
   workers, spawned and watched at runtime.
2. Killing one worker restarts it in **under 1 s**, and the supervisor logs the exit reason it
   observed.
3. A crash storm (6 abnormal exits inside one intensity window) makes the supervisor **give up on
   that child only**, while the other children stay alive and answer a typed call afterwards.
4. The library's pure decision logic (policy matrix, intensity window, backoff, strategy scope) is
   covered by host unit tests.
5. The witness runs from `tests/integration/tests/actor-supervisor.rs` through
   `scripts/qemu-actor-supervisor.sh`, and the raw + normalized logs are published under
   `docs/evidence/`.

## Phases

| Phase | File | Status | Depends on |
|---|---|---|---|
| 1 | `phase-01-actor-supervisor-library.md` | complete | — |
| 2 | `phase-02-witness-cells-and-launch-edge.md` | complete | Phase 1 |
| 3 | `phase-03-qemu-witness-and-closure.md` | complete | Phase 2 |

## Scope boundaries (recorded, not hidden)

- **Strategies beyond `one_for_one`** (`one_for_all`, `rest_for_one`) ship in the library and are
  unit-verified at the scope/policy level. The QEMU witness exercises `one_for_one`, which is what
  the roadmap's B0 acceptance asks for; a per-strategy QEMU matrix is not claimed.
- **No intra-cell concurrency.** Actors are single-threaded event loops; concurrent in-actor work is
  B1's reactor.
- **No fire-and-forget send and no per-actor mailbox.** The mailbox stays kernel-owned, bounded, and
  backpressured (Spec 17 §2/§6); the library does not add a post path.
- **`ServiceRef` needed no new work.** The roadmap's B0 item 3 ("add auto-`invalidate()` on restart")
  is already satisfied in-tree: `ServiceRef::call` invalidates the cached TID on `Send`/`Recv`/
  `WrongSender` and re-resolves on the next call (`libs/ostd/src/service.rs:120-129`). This phase
  verifies and documents that instead of re-implementing it.
- **No CI job was added.** No comparable named runner (`qemu-c-spawn.sh`, `qemu-pipe-test.sh`,
  `qemu-native-domain-test.sh`) is CI-wired today; the established evidence path is a local runner
  plus published logs. CI wiring is a separate decision with its own review.
- **`POLICY.BIN` is untouched.** `SpawnCap` is minted from the child's manifest, not from the
  install path, and a policy `NoEntry` keeps ordinary caps while stripping only the privileged
  path-minted ones (`kernel/src/policy.rs` module docs). The witness cells request no privileged
  path cap, so no policy row and no blob regeneration is required.

## Evidence

- Host: `cargo test -p ostd --target x86_64-unknown-linux-gnu --lib actor::` — 8 tests over the
  supervisor decision logic (policy matrix, intensity rollover and give-up, backoff growth/cap, and
  strategy scope for all three strategies).
- QEMU: `scripts/qemu-actor-supervisor.sh --harts 1` (RV64) driving
  `tests/integration/tests/actor-supervisor.rs`; markers recorded in
  `docs/evidence/actor-supervisor-harts1-qemu.{log,txt}` with the kill→restart latency, the
  give-up isolation, and the `one_for_all` expansion.
- Launch edge: enforced end-to-end by the same run — the runner fails on any `DENY launch edge`
  line, so the new `(backend-supervisor, Path|Elf, /bin/backend-worker)` row is exercised for real.
  The pins added to `kernel/src/loader/launch_profile/tests.rs` are **documentary** (see findings).

## Findings recorded, not fixed (out of B0 scope)

0. **FIXED as a B0 prerequisite — `RecvTimeout` dropped queued child-exit notifications.**
   The deadline receive used `snapshot_resume` for its delivery peek, and that function knew
   owner-deaths, `pending_exit_reason`, and queued messages — but **not `pending_deaths`**, the queue
   `exit_task` fills when the watcher is *running* rather than parked in `Recv`. A watcher that polls
   with `sys_recv_timeout` (the shape a supervisor needs for its backoff timers) therefore never
   learned that a child had exited: the witness killed child `w0`, the kernel logged
   `[kernel] ForceExit: task 21 killed by task 20`, and the supervisor waited forever until its own
   watchdog fired. Plain `Recv` checks that queue first and mask-agnostically, so `NotifyOnExit`'s
   contract (Spec 12 §4.3: the reason is delivered as the recv payload) held only for blocking
   receives. Fix: `take_queued_death()` at the head of the `RecvTimeout` arm in
   `kernel/src/task/syscall.rs`, with the same order and mask rule as `Recv`; the record's wire form
   is unchanged, so no ABI change. Two other B0 bugs were found by the same run and fixed in the
   library/witness rather than the kernel: `ActorCtx::now_ticks` originally read `GetTime` op 0
   (raw counter, not ticks), and the witness's syscall allowlist lacked the `LookupService`/grant
   trio its VFS-read spawn path needs.

1. **The kernel's `#[cfg(test)]` pins cannot execute.** `cargo test -p cellos-kernel` fails with
   `error[E0463]: can't find crate for 'test'` (the crate is `no_std`, binary-only, and CI excludes
   it from `cargo test --workspace`). `kernel/src/loader/launch_profile/tests.rs` is therefore a
   documented policy table, not enforcement — including the rows added here.
2. **`cargo test -p ostd` now compiles a host harness and exposes 6 pre-existing failures** in
   `clients::vfs::read_file::tests::bounds::*`. They could never link before this change (the crate
   was unconditionally `no_std`), so they are bit-rot in the VFS client lane, not a regression from
   B0. They are left red-visible rather than deleted or re-pinned.
3. **`tier2-fault-isolation` fails in this WSL environment with or without the B0 kernel change**:
   `2 passed / 3 failed`, and the failing cases wait for a `[domain] admitted cell` line that never
   appears — the exploit runs its default NULL-write mode, so the shell appears to drop the CLI
   argument (`tier2-exploit peer`). Verified *not* to be a B0 regression by stashing the kernel
   change, rebuilding, and re-running: the same case fails the same way on the pristine kernel. The
   parts of the suite that do run here (hardware page-fault isolation, positive execution) still pass
   with the B0 kernel, and the runner for this programme uses `x86_64-unknown-linux-gnu` because
   `tests/integration/.cargo/config.toml` pins the host target to `x86_64-pc-windows-msvc`. Recorded
   so the next reader does not attribute it to this change; it is a Tier-2-lane investigation.
4. **`libs/ostd/src/console.rs` is dead code**: it defines `print!`/`println!` but the module is not
   declared in `lib.rs` and no cell uses the macros; the live logging API is `ostd::io::println`.
   B0 uses the live one and does not touch the dead file.
5. **The roadmap's B0 item 3 was stale**: `ServiceRef` already invalidates and re-resolves on a dead
   peer (`libs/ostd/src/service.rs:120-129`), so B0 documented the behaviour instead of
   re-implementing it (ADR-0021 §2.5).
   Local `cargo clippy -p cellos-kernel --target riscv64gc-unknown-none-elf -- -D warnings` also
   reports a pre-existing `manual_is_multiple_of` lint in `kernel/src/task/futex.rs:116` (untouched by
   this change); every file this programme added is clippy-clean for the `app-backend` crate on all
   three cell targets.
6. **`init`'s restart-storm give-up is inert, and B0 nearly inherited the bug.**
   `cells/tools/init/src/supervisor.rs` compares its 1 000-unit window against
   `ostd::syscall::sys_get_time()`, which is `GetTime` **op 0** — the raw architected counter
   (10 MHz `mtime` on QEMU RV64), *not* scheduler ticks (`op 4`, `kernel/src/task/syscall.rs:5456`).
   The window is therefore ~0.1 ms, it rolls on every exit, and `restart_count` never reaches
   `MAX_RESTARTS_PER_WINDOW`: a crash-looping service is respawned forever, so Spec 12 §4.3's
   "≤5 / ~10 s, give up on that one service" is not in force in the shipped boot supervisor. The
   first B0 witness run reproduced exactly this pattern — its watchdog fired on the very first tick
   because `now - started_at` was measured in `mtime` units — which is how the defect was found in
   `ActorCtx::now_ticks` too. The library now uses op 4 (the same clock the kernel uses for
   `RecvTimeout` deadlines) and its budget is covered by the host tests and the QEMU witness. **`init`
   itself is deliberately left unchanged**: it is a different lane, and changing the boot
   supervisor's restart behaviour deserves its own review.

7. **`hotswap-smoke` fails 4 of its 15 cases in this WSL environment with or without the B0 kernel
   change** — `hotswap_cli_preserves_demo_state`, `peer_death_guardrail_is_bounded`,
   `supervisor_hotswap_preserves_demo_state`, `supervisor_rejects_unauthorized_hotswap_sender`;
   `11 passed / 4 failed` with the identical failure list after stashing the kernel change and
   rebuilding. The guardrail case times out waiting for `[peer-death-runtime] PASS` while the boot
   log is healthy, and the Tier-2 cases above show the same shape: a cell launched from the shell by
   name plus an argument runs its default mode, i.e. the argument does not reach the cell (the same
   class as the recorded RPi3 lane note that the console had to use a bare `ai-test` because argument
   handling mangles it). Recorded so the next reader does not attribute it to B0; the cell-death
   paths B0 actually depends on are covered by the B0 witness, which passes on the same kernel.

8. **The organisation's CI is red at HEAD for an infrastructure reason, not for code.** The matrix
   jobs install `gcc-riscv64-unknown-elf g++-riscv64-unknown-elf` and the runner now reports
   `E: Unable to locate package g++-riscv64-unknown-elf`, so every job behind that install step fails
   before it compiles anything (`Build (riscv64/aarch64)`, `Clippy (aarch64/x86_64)`,
   `Host unit tests (types + api)`, `Security Scan`, `CellosFS /srv`, `Network Data-Path`) — the same
   set was already failing on the pre-B0 tip (`199a802bc`), which is the baseline this programme was
   compared against. Six CI-visible problems *were* fixed (the first two are B0's own, the rest are
   one-line blockers from earlier lanes): `cargo fmt` on the new files (the job checks formatting);
   `docs/code-metrics.generated.md` (that job runs `generate-code-metrics.py --check`, and the file
   was already ~2.1k kernel lines stale before B0 added 68 more); `tools/cellos-cc` committed mode
   `100644` by `b9a32ad69`, which made a fresh checkout fail its CMake probe with `Permission denied`;
   the workspace's only `-D warnings` clippy failure (`manual_is_multiple_of`, `kernel/src/task/futex.rs`
   from `4884ace5e`); a stale duplicate doc comment in `cells/drivers/dwc2-usb/src/hid/mod.rs` from
   `f20f2d19d`; and `needless_range_loop` in `cells/tests/tls-test/src/main.rs` (also `4884ace5e`).
   With those, CI's own clippy command (workspace, rv64, `-D warnings`) is clean locally. What remains
   red is infrastructure: the runner cannot install `g++-riscv64-unknown-elf`, so the Build/Clippy/Host
   unit tests/Security Scan jobs fail at the install step. The same lint job also warns
   `fatal: No url found for submodule path 'cells/demos/doom/src/c/doomgeneric' in .gitmodules` — the
   orphan gitlink reported earlier.

## Assumptions
- A supervisor Cell is a signed, `SpawnCap`-bearing cell; monitoring stays gated on `SpawnCap`, so
  no ABI widening was needed (ADR-0021 §2.4).
- Restart semantics mirror the ones the project already proved in `init` (Spec 12 §4.3): Permanent /
  Transient / Temporary, ≤5 restarts per ~10 s window, give-up on that child only.
