# NET-OD CP2 dossier: Net-service reclaim of a dead client cell's sockets (implementation, host proof)

Date 2026-10-30. Author NetODImpl. Authority: `net-owner-death-cp1.md` (SHA-256 `c40fdf98…4965`, final text) and `net-owner-death-approval.md` (D-NOD-1..13 as recommended; acknowledgement of the two behavioural points). Scope: section 7/8 closure only, `DEV_POSTURE`, host tests. Nothing committed or pushed; no QEMU/VM/Docker/network; no `git stash/checkout/reset`; `CHANGELOG.md` left to the main agent (hash unchanged).

Ceiling: host tests and bare-metal builds only. **No QEMU/T3 observation was made**; V2/V3 (the T3 diag + full boots) belong to the later T3-D3 CP1 (cp1 section 15). Nothing here claims the T3 cancel marker is met.

## 1. Baselines (before the first edit; HEAD `d743aea78`, `git status --short` empty)

HEAD differs from the CP1's binding HEAD `296fecebc`, but every section-7 pre-edit SHA-256 and every pinned hash was re-taken and is **equal to the CP1's** (no drift; list in section 2).

| Command | Before | After |
|---|---|---|
| `cargo test --offline --locked -p cellos-kernel --target x86_64-unknown-linux-gnu` | 293 passed | **295 passed** (+2; 6 consecutive runs, all 295) |
| same, `--features phase04-driver-profile` | 293 passed | **295 passed** |
| `cargo test --offline --locked -p service-net --target x86_64-unknown-linux-gnu` | 56 passed | **65 passed** (+9; 3 consecutive runs, all 65) |
| `cargo test --offline --locked -p api` | does not build for the default riscv64 target (`can't find crate for std`, a property of the command, not of the tree) | same; the runnable form `-p api --target x86_64-unknown-linux-gnu`: 154 + 2 passed, 4 ignored, before and after |
| `cargo clippy … -p cellos-kernel --all-targets` (host) | bin 35 warnings, bin test 48 | identical (35 / 48) |
| `cargo clippy … -p service-net --all-targets` (host) | bin 7 warnings, bin test 53 | **6 / 50** (fewer; `remove_owner`/`get_unchecked`/`graceful_closes` dead-code warnings gone). No clippy finding lies in any added line (checked by location) |
| `python3 scripts/cellos-sign --check` | n/a | rc 0, `OK: F5`, `OK: F1` (134 crates, 927 files) |

## 2. Hashes (SHA-256)

Closure (before = the CP1 section-7 values, re-taken equal; after = now):

| Path | Before | After |
|---|---|---|
| `kernel/src/task/scheduler.rs` | `2065efd4…1b82a` | `e49d5fea5559275dff32dbdb2469d2d1ff190462cd94aaa30d63923908c711d2` |
| `kernel/src/task/syscall.rs` | `a23eb08c…e7c3` | `c00a71b14fc774e7d961cc302930812c59449ba70932b7e17c1f935148e13b85` |
| `kernel/src/task/tcb.rs` | `160628a3…d7859` | `3e8880236286f30b33afa8a72291eb7fefa58fe5b8c7a4f0fd38aba7127cdf61` |
| `cells/services/net/src/socket_table.rs` | `76e8724b…f882d` | `1964bc9d0ee14d9db1fd6d60715c65d4c0cfda1edda01c2999705903ff456ecb` |
| `cells/services/net/src/socket_table/tests.rs` | `1ac75c8d…af269` | `28529be349558c9829d0b76dcb459fef5223efdb7b6d25d06741a1a62c20a6ef` |
| `cells/services/net/src/service-runtime.rs` | `80c64227…b027ff` | `89d9334a01d047c93e3ff97352b7a99a460d9093d4a9dc9ac8d97044e7db811e` |
| `cells/services/net/src/service-runtime-owner-death-tests.rs` | ABSENT | `3c12780f690adb5a1629a1ccc0fdcab9515035e66e3a917c6cc369343de807a5` |
| `docs/network-api.md` | `41445926…8dea` | `5879be3971ed6b7d2782e804eb316a9f2733596b757fe0c90b5960a8e234b2d3` |
| `CHANGELOG.md` (main agent) | `8866d0db…bdba03` | unchanged (not touched) |

Pinned, verified byte-identical after (`sha256sum -c` against the pre-edit list, all OK): `libs/api/src/abi/{syscall,cell_owner,caller_identity}.rs`, `libs/api/src/services/ipc.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/clients/net.rs`, `kernel/src/cell/service_registry.rs`, `kernel/src/fast_ipc.rs`, `cells/services/net/src/main.rs`, `handlers/tcp.rs`, `tls/dispatch.rs`, `cells/services/net/Cargo.toml`, `Cargo.lock`, `kernel/src/main.rs`, `launch_profile/targets.rs`, `launch_profile/mod.rs`, `kernel/Cargo.toml`, `kernel/build.rs`. `git status --short` after: exactly the 7 modified closure files + the one new file; nothing else.

## 3. Item -> code -> test

| CP1 item | Code | Test |
|---|---|---|
| Grant predicate (4.1, D-NOD-1 A) | `scheduler.rs`: private `may_watch_owner_deaths(&Task)` = registered VFS cell **or** `service_registry::lookup_bound(service::NET)` with `(cell, generation) == (watcher.cell_id, watcher.cell_generation)`; `watch_live_cell_owner` calls it; `current_caller_*` equality and `resolve_live_cell_owner` unchanged; comments at the old `:386`, `:654` updated | `syscall.rs` tests: `net_owner_watch_grant_matrix` (granted; unregistered; CONFIG/INPUT/KMS/VFS providers carrying the watcher's identity; NET Paused; NET re-registered to another provider; stale watcher generation; other watcher cell; cell 0; serving another cell; asking for another cell; no serving context; stale / dead client generation; dead client root; VFS cell regression incl. "other principal" refusal and cleared VFS; `cancel_cell_owner_watch` only by the watcher's own tid and token; a refusal installs nothing) |
| `TryRecv` mask 0 delivers one owner death (4.2, D-NOD-2 A) | `syscall.rs` `Syscall::TryRecv` arm: first, under SCHEDULER, `peek_owner_death_for_try_recv` (mask 0 only, FIFO, nothing removed); `validate_user_buf` + 8-byte reason copy when `buf_len >= 8`; then `remove_owner_death(task, token)`; then trailer for the dead tid when `attest_caller` (zero tail, as `Recv`); returns the dead root tid. Before the context/pending-message code. `Recv` untouched | `try_recv_takes_owner_death_only_for_mask_zero` (masked receives see nothing and leave the queue; mask 0 FIFO; peek removes nothing; commit removes exactly one token, unknown token no-op; `pending_deaths` untouched). `Recv` unchanged by construction (diff) |
| Doc-only `tcb.rs` (4.4, D-NOD-8 A) | comments at the old `:328-333`, `:888-894` | n/a (18-line comment diff; `#[inline]` kept) |
| Net table abort/ledger (5.3) | `socket_table.rs`: `aborts: BTreeMap<u64,u64>`; `abort_owner(owner, sockets, deadline) -> (aborted_tcp, released_other)` (cursor scan with `Bound`, UDP detected by `is_udp_cap` never `get_mut::<tcp>`, graceful/abort ledger caps skipped, TCP with tuple -> `abort()` + ledger, no tuple -> removed now); `begin_abort`, `is_aborting`, `aborting`, `is_udp_cap`; `remove_internal` clears `aborts`; `remove_owner` deleted; `owned_cap` now `#[cfg(test)]` (its only production caller was the replaced loop) | `socket_table/tests.rs`: `abort_owner_treats_each_socket_kind_by_what_the_peer_must_hear` (SynSent kept in ledger with tuple until a poll then satisfied; UDP, listener, never-connected removed now; graceful skipped and still SynSent; other CellId and other generation untouched; second call `(0,0)`; slot returned after the ledger entry is removed), `remove_internal_clears_the_abort_ledger_and_the_budget_returns` (258 sockets in one owner, ledger bound, budget back to baseline) |
| Graceful-ledger preservation + retargeted test (5.4, D-NOD-4 A) | `abort_owner` skips `is_graceful_closing` | `owner_cleanup_preserves_other_generation` retargeted: graceful cap survives owner reclaim, plain cap freed `(0,1)`, other generation intact, idempotent, only `remove_internal` (the reaper's act) ends it |
| `release_owner` order (5.1) | `service-runtime.rs`: state steps in `reclaim_owner_state` (ready-wait, DNS cancel, `tls_pending.cancel_owner`, `tls_table.retain` by owner, `table.abort_owner`); `release_owner` = that + `abort_deadline = now + ABORT_DRAIN_TICKS`, watch cancel + `watched.remove`, then the log line (`owner_death_line`, only when something was reclaimed). `WATCH_REFUSED` string unchanged | `reclaim_frees_everything_the_dead_owner_held_and_nothing_else` (connect + UDP + listener + ready-wait + 3 DNS queries of the dead owner; other generation and other cell untouched, DNS slots of the dead owner freed in the resolver, the others still pending; `(1,2)`; repeat is a no-op), `the_log_line_appears_only_when_something_was_reclaimed` |
| Drain/reap (5.1, 5.3) | `reap_aborted(table, sockets, now)` + `ABORT_DRAIN_TICKS = 1_000 * MTIME_TICKS_PER_MS`, `ABORT_REAP_PER_TURN = 32`, called each turn after `reap_graceful_closes`; entry finished when the socket's `remote_endpoint()` is `None` or `now >= deadline` | `reap_aborted_keeps_an_entry_until_its_rst_leaves_and_drops_it_at_the_deadline` (kept at deadline-1, removed at the deadline, table back to baseline; and removed early once the RST has left) |
| RST on the wire with negative control (9.2 item 1) | n/a (behaviour of smoltcp + the above) | `silent_removal_leaves_the_peer_established_and_abort_resets_it`: two sockets over `smoltcp::phy::Loopback`; control: `SocketSet::remove` alone leaves the peer `Established`/`may_recv`; reclaim then 2 turns: peer `Closed`, `recv` errors, client tuple cleared, `reap_aborted` frees the slot |
| Graceful FIN not pre-empted (9.2 item 4) | n/a | `a_graceful_close_in_flight_is_flushed_not_pre_empted_and_only_its_reaper_ends_it` (reclaim `(0,0)`; peer sees `CloseWait`, an orderly FIN; abort reaper leaves it; graceful reaper keeps it in TIME-WAIT and ends it at `Closed`) |
| UDP/listener immediate removal, pending cancel | `abort_owner` / `reclaim_owner_state` | covered above (`reclaim_frees_everything…`, `abort_owner_treats_each_socket_kind…`) |
| Exact root-TID match, stale-generation sweep (9.2 item 5) | helpers `owner_for_root(&watched, root_tid)` and `stale_generation_of(&watched, owner)` replace the inline `find_map`/`find` in `run()` | `owner_for_root_matches_the_root_tid_exactly` (worker TID and 0 do not match), `a_new_generation_of_a_cell_releases_only_the_old_generation` (+ through the real reclaim) |
| Docs | `docs/network-api.md` new subsection "Owner death (client cell killed or relaunched)": mechanism, RST not FIN and its cost, per-kind actions, drain ledger and deadline, graceful preservation, log line, what is unchanged (5.7) | n/a |

## 4. Builds (private `CARGO_TARGET_DIR=/tmp/netod-target`)

- `cargo check --offline --locked -p cellos-kernel --target x86_64-unknown-none`: default, `--features test-hooks`, `--features phase04-driver-profile`: all Finished (3 pre-existing dead-field warnings each).
- `service-net` bare-metal, **`--release`** (as the image scripts build cells), all Finished: riscv64gc-unknown-none-elf (`relocation-model=pic`), aarch64-unknown-none-softfloat (`pic`, `+bti,+paca,+pacg`), x86_64-unknown-none with `RUSTFLAGS="-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic"`; also x86_64 `--features hypervisor-bridge` Finished. Warnings: the pre-existing ones only (`alloc::vec`, `LOOP_TRACE_INTERVAL_TICKS`, `validity_window`, `is_empty`, `handle`); the old `remove_owner is never used` warning is gone.
- The default dev profile does not link/compile these cells for bare metal even at baseline (rv64/aarch64 `R_RISCV_64`/lld link error; x86_64 `LLVM ERROR: Do not know how to split the result of this operator` in `polyval/sha2/aes`), so debug-profile bare-metal builds are not a usable gate; not caused by this slice.

## 5. Deviations from the CP1 text (none weakens a claim)

1. **Two kernel helpers instead of one.** The CP1 names a pure `take_owner_death_for_try_recv`. Section 4.2 also requires validate-then-copy-then-remove, which a single removing function cannot give, so the selection is `peek_owner_death_for_try_recv` (pure, nothing removed) and the commit is `remove_owner_death(task, token)` (exact record). Both are unit-tested; neither dead in production.
2. **Test accessor.** `Scheduler::cell_owner_watch_count()` (`#[cfg(test)]`, 1 line of logic) in `scheduler.rs` so the grant matrix in `syscall.rs` can see installed watches (`cell_owner_watches` is private to the scheduler module). Same file, no production effect.
3. **Production helpers added in `service-runtime.rs`** beyond the CP1's `reap_aborted`/`ABORT_DRAIN_TICKS`: `reclaim_owner_state` (the syscall-free steps 1-4 of 5.1, so host tests never reach `sys_cancel_cell_owner_watch`/`println`, which on the host target are real Linux syscalls with a ViSyscall number), `owner_death_line`, `stale_generation_of`, `owner_for_root`, `ABORT_REAP_PER_TURN`. `reap_aborted` takes `now` as a parameter (the host `sys_get_time` is not controllable). `release_owner` keeps its signature and order (state, watch cancel, log).
4. **`owned_cap` is `#[cfg(test)]`.** After the loop in `release_owner` was replaced it had no production caller and would have been a new bare-metal dead-code warning.
5. **9.2 item 1 harness.** Loopback is available (smoltcp `alloc` + the unified `medium-ethernet`; ARP self-resolves), so the primary form was used, not the `VirtioNetDevice::push_rx` fallback. The new test file has its own small `Stack` harness (the admission harness `Net` is private to its module and the admission file is outside the closure), over a loopback interface rather than `VirtioNetDevice`.
6. **9.1 item 3 not implemented** (`watch survives until root exit then queues once`): `exit_task` on a synthetic root was not attempted (it releases real per-task resources); the exit half is unchanged by this slice and stays covered by the existing boot self-test of the VFS watcher (`vfs_lifecycle_selftest.rs`).
7. **Established-TLS retain not unit-tested.** `TlsSocketEntry` can only be built by a completed handshake; `tls_table.retain(|cap,_| !table.is_owner(*cap, owner))` is covered by review and compilation only. (`tls/dispatch.rs` is pinned and untouched; its doc comment still says "Call before SocketTable::remove_owner", now a stale name, left because the file is byte-pinned.)
8. **The kernel grant matrix registers NET in the global registry** (the CP1 asks for `service_registry::register`). The registry tests in `service_registry.rs` do not take `TEST_STATE_LOCK` and `reject_zero_tid` asserts `lookup(NET) == None`; to avoid a race the matrix publishes the NET entry only for the duration of each single `watch_live_cell_owner` call and `clear_tid`s it immediately. 6 kernel runs showed no flake.
9. **`-p api` baseline command** cannot run as written (riscv64 default target); recorded the `--target x86_64-unknown-linux-gnu` form (section 1).
10. HEAD was `d743aea78`, not `296fecebc`; all closure/pinned hashes matched, so the binding digests hold.

## 6. Deferred / not done

- **No QEMU/T3 proof** (V2/V3). The wire effect through slirp, the guest sshd and the producer is unobserved; only smoltcp-over-loopback RST is proven.
- TX-queue behaviour of `VirtioNetDevice`/e1000 under a burst of RSTs, `hypervisor-bridge` runtime behaviour (the RST goes out on the next request there; built, not run), and wake latency for a queued death on `sys_wait_completion(NET_RX, 10)`: not measured.
- Follow-ups unchanged from CP1 5.7: graceful-deadline silent removal (D-NOD-7), legacy `TcpClose` silence, TLS handshake-in-flight silent drop (D-NOD-6), no keepalive.
- The T3-D3 CP1 (section 15), canonical re-promotion (D-NOD-12), lock-scheduling records (11.5): separate decisions.
- `CHANGELOG.md`: main agent.

## 7. Decisions applied

D-NOD-1 A (registry-bound Active NET provider + generation + serving context), D-NOD-2 A (`TryRecv` mask 0), D-NOD-3 A (RST), D-NOD-4 A (graceful ledger preserved), D-NOD-5 A (fail-open, log once, string unchanged), D-NOD-6 A (`tls/dispatch.rs` untouched), D-NOD-7 A (`reap_graceful_closes` untouched), D-NOD-8 A (`tcb.rs` comments), D-NOD-9 A/C (host proof here), D-NOD-11 A (no `libs/api`/`libs/types` byte changed), D-NOD-12 A (promoted bytes untouched).
