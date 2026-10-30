# NET-OD CP1: Net-service reclaim of a dead client cell's sockets (kernel owner-death grant + delivery, Net abort semantics) (final text; decisions in section 18; documents only; nothing applied)

Written 2026-10-30 at HEAD `296fecebc01740b451610ddfa5ecba0e3cabe4a9` (`git status --short` empty at binding). Documents only: no source, `Cargo.toml`/`Cargo.lock`, script, lock, plan, image, key, network, build, test or QEMU action was made to produce it. Everything cited as `file:line` was read in this session; `[INFERENCE]` marks anything not observed in a file or command output; `OPEN (decider)` marks a decision for the named owner. Form follows `.agents/261030-1000-kms-ssh-key-custody/k3-cp1.md` and `k5-cp1.md`; the lock packet (section 11) follows `lock-handoff-k2-cp2.md` / `k3-cp1.md` section 11.

Inputs read: `cancel-diagnosis.md` (SHA-256 `cb3c7e6c…9fc9`), `cp2-dossier-part3.md` (`2b8b39bb…4452`), `cp2-dossier-part4.md` (`1cf7203a…bf09`), `cp1-draft.md` (`2f7ae851…12ba8dc`, the approved Tier-3 CP1, sections 6.3 and 9), `cp1-diag-approval.md` (`b86eb387…558b`), `.agents/cell-contract-0024-locks.md` (`258e920e…1499`), `.agents/plan-portfolio.md` (`1300997b…5ff4`), `261006-0934-httpd-single-cell-concurrency/phase-03-network.md` (`ac00a027…679bd`), `261020-1750-cellbox-cli/phase-04-s2n-cp2-dossier.md` addendum (owner-watch line), `261020-1750-cellbox-cli/reports/s10-10-evidence/probe/tier3-and-guards.md`, `docs/decisions/0024-cell-kernel-authority-contract.md` (`275b068c…c32dc`), `docs/specs/25` and `17` (grep level), `docs/code-standards.md` (`9cb004ad…1185`, Law 1); source: `kernel/src/task/{scheduler,syscall,tcb,async_ipc,cell_session}.rs`, `kernel/src/task.rs`, `kernel/src/cell/service_registry.rs`, `kernel/src/fast_ipc.rs`, `libs/api/src/abi/{syscall,cell_owner,caller_identity}.rs`, `libs/ostd/src/{syscall.rs,clients/net.rs}`, `cells/services/net/src/{main,service-runtime,socket_table,handlers,handlers/tcp,tls/dispatch,service-runtime-admission-tests,socket_table/tests}.rs`, `cells/services/vfs/src/main.rs`, `cells/tools/cellbox/src/ssh.rs` (close calls), smoltcp 0.11.0 `src/socket/tcp.rs` and `src/iface/socket_set.rs` (local cargo registry copy, not hashed), `scripts/qemu-tier3-ssh-guest-x86.sh`, `scripts/tier3_ssh_guest_judge.py`, `cells/tests/phase04-terminal-driver/src/ssh/guest.rs`.

## 0. Summary

| Item | Proposal |
|---|---|
| Defect (observed, T3-D and T3-D2) | Ctrl-C of `cellbox ssh` kills the client cell (`sched.exit_task(root, usize::MAX)`, `cell_session.rs:1568`); the Net cell never learns, never sends FIN/RST, the smoltcp socket stays ESTABLISHED, the guest sshd session and its producer stay alive (pcap: no FIN/RST ever; cell ACKs slirp probes with window 0 for >= 106 s; `cp2-dossier-part4.md` section 5) |
| Why (three independent causes, all read) | (1) the kernel grants `WatchCellOwner` only to the registered VFS cell (`scheduler.rs:663`); (2) even if granted, the Net loop receives with `TryRecv`, which never delivers a queued owner death (`syscall.rs:5098-5214` vs `Recv` `:4525-4582`); (3) `release_owner` / `remove_owner` would call `SocketSet::remove`, which emits nothing (smoltcp `socket_set.rs:120-130`) |
| Kernel change | (a) one predicate in `Scheduler::watch_live_cell_owner`: VFS cell **or** the live registry-bound provider cell of service `NET`; (b) `TryRecv` with mask 0 delivers one queued owner-death event exactly as `Recv` does. **No libs/api, libs/types, libs/ostd, policy, CapSet, launch-edge, manifest or allowlist change.** Pure kernel permission-table + delivery edit |
| Net change | on owner death: `abort()` every TCP socket of the dead generation (one RST), keep each in a small `aborts` ledger until its RST has left or a deadline, free UDP/listen/never-connected sockets at once, leave sockets already in the graceful-close ledger to their own reaper, cancel pending waits/DNS/TLS as today |
| TCP semantics | **RST (abort)**, not FIN (section 5.2) |
| Files | 3 kernel files (2 production + 1 doc-only), 3 Net files (+1 new test file), `CHANGELOG.md`, `docs/network-api.md`; section 7 |
| Law-1 | none triggered: `libs/api/` and `libs/types/` byte-identical (section 12) |
| Locks | CELL-CONTRACT-0024 rows L2, L8, L23, L35 (kernel), L38 (httpd phase-03 step 6 is this exact item), adjacent L21/L33/L36/L37/L32; file-bounded exception and owner handoff required before any edit (section 11) |
| Proof | host tests (kernel grant matrix, delivery helper, socket-table reclaim, RST on the wire over smoltcp loopback) + one T3 `diag` boot with a new outcome label + one T3 `full` boot (section 10) |
| Effort | S-M; roughly 120-200 production lines and 350-550 test lines [INFERENCE from the K3 sizing, not measured] |

## 1. Scope and non-scope

**In:** the kernel rule that lets the registered Net cell subscribe to the death of the principal whose request it is serving; delivery of that death to a polling receiver; Net-side reclaim with correct TCP wire behaviour; host tests; the lock handoff; the follow-up gate for a T3-D rerun (section 15).

**Out:** any ABI change (syscall numbers 244-248, `CellOwner` 32 B, `CellOwnerRequest` 16 B, trailer layout, `NetRequest` opcodes); other services (section 13); the graceful-close deadline path, TLS handshake-in-flight sockets and legacy `TcpClose` silence (section 5.7, follow-ups); candidate import / canonical promotion (the promoted bytes keep the defect until a separate re-promotion slice); any lane-script or judge edit (those belong to the follow-up CP1, section 15); any edit by this document.

**Serves:** Cellbox CBX-23 "producer traffic stops on consumer close", cancel half (`cp2-dossier-part4.md` section 8: not met, product finding) and the general rule in `phase-09-ssh-client.md` item 5 "close resources on cancellation or consumer death". **Not an Intel C2C outcome** (same statement as `lock-handoff-k2-cp2.md` section 1); the owner is asked to confirm the bounded-slice basis (section 11.1).

## 2. Findings that fix the design (kernel side, all read)

### 2.1 The mechanism, ABI and who may use it

| Item | Fact | Where |
|---|---|---|
| Registering syscall | `WatchCellOwner` = 245 (64-bit pointer ABI: a0 cell_id, a1 generation, a2 out ptr, a3 out len; returns a non-zero token, writes the 32-byte `CellOwner`) and `WatchCellOwnerRecord` = 248 (RV32-safe: a0 = pointer to the 16-byte `CellOwnerRequest {cell_id u64 LE, generation u64 LE}`); `CancelCellOwnerWatch` = 246 (a0 = token); `ResolveCellOwner` = 244 and `ResolveCellOwnerRecord` = 247 only attest, they install nothing | `libs/api/src/abi/syscall.rs:694-716`; `cell_owner.rs:7-8,10,55-80`; `libs/ostd/src/syscall.rs:980-1022` (`sys_watch_cell_owner` picks 245 or 248 by pointer width) |
| Token | positive, monotonic, never reused, bounded to `MAX_RETURNABLE_OWNER_WATCH_TOKEN` (stops before the signed return boundary) | `scheduler.rs:641-652`; host test `owner_watch_tokens_stop_before_the_signed_return_boundary` (`:2880`) |
| The one place permission is decided | `Scheduler::watch_live_cell_owner` (`scheduler.rs:656-681`): `is_registered_vfs_cell(watcher.cell_id)` **and** `watcher.current_caller_cell_id == cell_id` **and** `watcher.current_caller_cell_generation == generation`, then `resolve_live_cell_owner` (live Cell generation whose root task is live) | `scheduler.rs:662-669` |
| Correction to `cancel-diagnosis.md` section 2.4 | `syscall.rs:6352` / `:6405` gate the **Resolve** syscalls (244: VFS principal **or** the owner is a registered-service root, `:6351-6360`; 247: VFS only), not Watch. Both Watch syscalls (`:6368-6388`, `:6421-6446`) call `watch_live_cell_owner` and have no gate of their own. So the single predicate at `scheduler.rs:663` is the whole permission edit; `syscall.rs` needs an edit only for delivery (2.3) | `syscall.rs:6335-6453` |
| Why the attested-caller condition | the right is "learn when the principal you are serving dies", not "watch any cell": the kernel-written receive context (`current_caller_cell_id/generation`) is the proof that this cell is the caller's current counterparty. Net sets that context on every `TryRecv` delivery (`syscall.rs:5189-5194`; `task.rs:2699-2704`) and calls the watch before it dispatches or replies (`service-runtime.rs:631-645` before `:648`), so the context is still present; the async reply path clears it only afterwards (`async_ipc.rs:311-319`) | |
| Why only VFS today | history, not principle: the watch was built for the VFS lease/quarantine work (`260809-0922-sas-lbi-revocable-vfs-access/phase-03-lifecycle-cleanup-checkpoint.md:27,37`: "the kernel-visible current-caller rule supplies immediate owner death events without broad VFS `SpawnCap`"). The context itself is VFS-special-cased (`tcb.rs:861-886`: VFS keeps its outer caller across nested receives), and delivery is documented as "VFS's public plane only" (`tcb.rs:328-333,888-897`). Nothing in `libs/api` says VFS-only for Watch (the doc of 244 says it for Resolve) | |
| Event | queued unconditionally at the watched root's `exit_task`: `pending_owner_deaths.push((token, root_tid, exit_reason))`, the watch removed, a `Recv`-parked watcher (mask 0) woken; other states keep it queued. Cleanup when the **watcher** dies: `scheduler.rs:1491-1494` (`retain` by watcher tid). Worker-thread exit is not root death (`watched_root_tid == tid` match, `:1666-1672`) | `scheduler.rs:1663-1702` |
| Not an alternative | `NotifyOnExit` needs lifecycle (`SpawnCap`) authority (`syscall.rs:5866-5900`, `caller_has_spawn`); giving Net that would be a large authority escalation; `cells/services/ai/src/main.rs:91-96` records the same limitation ("A service Cell cannot watch another Cell's exit") | |

### 2.2 Permission is a kernel table edit, no new right

No policy flag, launch right, caller flag or manifest bit is needed:

- the Net cell already declares `WatchCellOwner`, `CancelCellOwnerWatch` and `TryRecv` in its allowlist (`cells/services/net/src/main.rs:42-60`), and `SyscallSet::EMPTY` permits the owner-watch calls anyway (`libs/api/src/abi/syscall_tests.rs:515-519`), so the syscall ceiling check (`cellbox-ceiling-diff.py`) and the Net manifest do not change;
- the identity of "the Net cell" is the **service registry binding**, not a name or a boot-time static: `RegisterService` is `SpawnCap`-gated and records the provider's live `(tid, cell_id, generation)` (`service_registry.rs:84-125`, `syscall.rs:5965-5984`), and the existing `may_query_dir_handles` already uses the registry because "a cell's name comes from a spawner-chosen path hint and is therefore forgeable" (`syscall.rs:3391-3395`). Net is registered as service id `NET` = 2 (`libs/api/src/abi/syscall.rs:1377`; `config/services.toml:57-62`, `required = true`, `restart = "always"`);
- `lookup_bound(NET)` returns `(tid, cell_id, generation)` only for an **Active** entry with a live identity (`service_registry.rs:143-150`). It needs no new function. Lock order SCHEDULER -> REGISTRY is the documented one (`syscall.rs:3396`; `commit_paused` is called under SCHEDULER);
- a restarted Net is a new generation and re-registered by the supervisor; the predicate compares the **generation**, so a stale cell with the same CellId cannot use the grant.

### 2.3 Delivery to a polling receiver

`Recv` delivers a queued owner death first, mask-agnostically for `pending_deaths` and for mask 0 for owner deaths: the 8-byte exit reason is written at the start of the buffer, the event is removed after the copy, the identity trailer is written for the **dead** tid (which `attested_identity_of` cannot attribute, so the tail stays zero), and the dead root tid is returned (`syscall.rs:4525-4582`; `attested_identity_of` returns `None` unless the task is in `tasks` and its Cell generation is live, `:2895-2923`; `write_caller_identity` leaves zeros, `:3366-3383`). `TryRecv` (`:5098-5214`) and its fallback `ipc_try_recv` (`task.rs:2644-2708`) read only `pending_msgs` and Sending tasks; they never look at `pending_owner_deaths`. The Net loop is the polling kind (`sys_try_recv_attested` at `service-runtime.rs:588`, then `sys_wait_completion(NET_RX, 10 ticks)` when idle, `:699`), because it must also wake on NIC RX and cannot park in `Recv`. A watch granted to Net would therefore queue events that nothing consumes (diagnosis section 2.4, confirmed here).

The Net loop already contains the consumer for exactly this wire shape: a received message whose trailer does not parse (`from_recv_buf` is `None`) and whose sender equals a watched owner's root tid releases that owner (`service-runtime.rs:595-607`). The same shape is what VFS handles (`cells/services/vfs/src/main.rs:177-190`, `handle_unattributed_owner_death(sender)`). VFS itself uses `TryRecv` while a prepared-cwd reservation is unsettled (`main.rs:157-163`), so it currently gets owner deaths only at its next blocking `Recv`; the change below also makes that earlier. [INFERENCE: not run; the VFS handler is the same branch for both receive kinds.]

## 3. Findings that fix the design (Net side, all read)

| Item | Fact | Where |
|---|---|---|
| Ownership | every cap is bound to one `SocketOwner {cell_id, generation}` at creation and every operation checks `table.get(cap, owner)`; accepted sockets are inserted with the accepting owner (`handlers/tcp.rs:187`); UDP, TCP, TLS sockets all go through `make_tcp`/`insert` with the requester's owner | `socket_table.rs:24-29,74-123`; `handlers.rs:90`; `tls_handler.rs:59`; `udp.rs:43` |
| Delegation | **none exists.** No `NetRequest` transfers, duplicates or shares a cap (opcode list `libs/api/src/services/ipc.rs:726-811`); caps are provider-local `u32`, monotonic, never reused (`docs/specs/17-ipc-wire-contract.md` section 10.7). Threads of a Cell share the Cell principal, so a worker-thread socket is owned by the Cell and dies with its root. ADR-0024 keeps service-net as owner of "IP/TCP and network-resource semantics" and requires new shared ownership operations to take their own Law-1 approval (ADR table row ADR-0023/Spec 17). Consequence: "owner death = release every cap of the owner" is exact today; if delegation is ever added, owner death must release the owner's **reference** and abort only when the last holder dies (stated as a constraint, section 14 R-8) | |
| Current reclaim code | `release_owner` (`service-runtime.rs:409-442`): drops the ready-wait, cancels the owner's DNS queries, `tls_pending.cancel_owner`, then `remove_internal` + `sockets.remove(handle)` for every cap, then cancels the watch. Reachable only from the unattributed-death branch (`:597-603`) and the stale-owner sweep (`:634-640`, same CellId, other generation) inside the `Some` arm of the watch (`:631-645`). `SocketTable::remove_owner` (`socket_table.rs:203-210`) does the same minus the TLS/DNS/ready parts and is **unused** (the build warns `method remove_owner is never used`, `cp2-dossier-part4.md` section 5) |
| Silent removal | `SocketSet::remove` "without changing its state" (`socket_set.rs:120-130`); the API doc of `tcp_close` says "The peer is not told (no FIN or RST)" (`libs/ostd/src/clients/net.rs:149-153`), and the legacy `TcpClose` handler is exactly `sockets.remove` (`handlers/tcp.rs:111-121`) |
| Graceful close | `TcpCloseGraceful` calls `close()` and records a deadline (100 000 000 ticks, nominally 10 s at the assumed 10 MHz, `handlers/tcp.rs:245-262`); `reap_graceful_closes` removes the socket when its state is `Closed` or at the deadline **without an abort** although its comment says "explicit abort" (`service-runtime.rs:304-321`). `cellbox-ssh` ends every normal session with `tcp_close_graceful` and then exits (`cells/tools/cellbox/src/ssh.rs:1192,1260`), so an owner can die while its socket is mid-graceful-close |
| Test pinning today's behaviour | `owner_cleanup_preserves_other_generation` asserts that `remove_owner` also drops a cap in the graceful ledger (`socket_table/tests.rs:138-154`); this slice changes that expectation on purpose (section 5.4) |
| Log line | `WATCH_REFUSED` prints `[net] owner death watch unavailable; socket reclaim deferred` once (`service-runtime.rs:90,642-644`); present in T3-D (`serial.log:314`), T3-D2 and L5f. No script greps it (`grep` over `scripts/` and `.github/`: no match) |

## 4. Kernel design

1. **Grant predicate (`scheduler.rs:662-668`).** Replace `is_registered_vfs_cell(watcher.cell_id.0 as usize)` by a private helper `may_watch_owner_deaths(&Task) -> bool` = registered VFS cell **or** `lookup_bound(api::syscall::service::NET)` returns `(_, cell, generation)` with `cell == watcher.cell_id.0 && generation == watcher.cell_generation`. The `current_caller_*` equality and `resolve_live_cell_owner` checks stay. VFS behaviour is byte-for-byte unchanged. A paused (hot-swap) or unregistered Net is refused, which is fail-open for Net (section 5.6) and self-healing (Net retries on the owner's next request because the owner is not yet in its `watched` map).
2. **Delivery (`syscall.rs`, `Syscall::TryRecv`).** New private fn next to the arm (the `Recv` block is left untouched to keep the VFS blast radius at zero): when `mask == 0` (`Task::owner_death_matches_receive_mask`) and `pending_owner_deaths` is non-empty, **before** `begin_receive_context` (`:5111`; `Recv` also returns its death before any context drop): snapshot the first `(token, root_tid, reason)` under SCHEDULER, copy the 8-byte reason when `buf_len >= 8` (validate first, remove only after the copy, same two-phase order as `:4561-4581`), remove that exact record, write the trailer for the dead tid when `attest_caller`, return `Ok(root_tid)`. `pending_deaths` (NotifyOnExit) are **not** delivered by `TryRecv`: that would change every `TryRecv(0)` caller that has a NotifyOnExit subscription; only owner deaths, which exist only for registered watchers, are added. A masked `TryRecv` (mask != 0) never consumes an owner death (`tcb.rs:328-333,888-897` invariant).
3. **Pure helper for testing.** The selection/removal is one pure fn `take_owner_death_for_try_recv(task: &mut Task, mask: usize) -> Option<(usize /*root*/, usize /*reason*/)>` so host tests need no user memory (section 9.1).
4. **Doc-only (`tcb.rs`).** The comments at `tcb.rs:328-333` and `:888-894` say "VFS root-owner death events" / "VFS's public plane only"; update them to "registered watchers (VFS, Net)" and note `TryRecv` mask 0. Also `scheduler.rs:386` ("VFS-only") and `:654-655`. Droppable (decision D-NOD-8) but stale comments in a permission path are maintenance debt.
5. **Unchanged on purpose:** the `exit_task` delivery (`:1663-1702`), `watcher` cleanup (`:1491-1494`), `ResolveCellOwner*` gates, `libs/api`, `kernel/src/main.rs`, boot self-tests (the VFS lifetime self-test is wired from the protected `main.rs` and its "PASS" text is matched by `scripts/x86/qemu-domain-test.sh` and `scripts/qemu-aarch64-test-hooks.sh`; extending it would risk both, so the kernel proof is host tests plus the QEMU proof of section 10).

Granted-by construction: the watch exists only if (a) the caller is the Active registered NET provider cell of the right generation, (b) it is currently serving exactly that principal, (c) that principal is a live generation. A cell can neither watch a principal it is not serving nor impersonate Net (registration is `SpawnCap`-gated). Information gained by Net: that a cell it just heard from has died, and its exit reason; VFS already receives the same.

## 5. Net design

### 5.1 What happens on owner death

Trigger (unchanged): the unattributed-death branch (`from_recv_buf` is `None`, sender equals a watched root tid) and the stale-generation sweep. New `release_owner` order:

1. `pending_ready.remove(&owner)`; cancel the owner's pending DNS queries (`resolver.cancel`, existing code); no reply is owed (the kernel cancels the dead owner's async operations in `on_owner_exits`).
2. `tls_pending.cancel_owner(owner, ..)` (existing; drops in-flight TLS futures and the sockets of handshakes still in progress: silent drop, section 5.7).
3. `tls_table.retain(|cap, _| !table.is_owner(*cap, owner))`: drop established-TLS state (no close-notify is attempted; a dead cell cannot answer a TLS shutdown).
4. `table.abort_owner(owner, sockets, now + ABORT_DRAIN_TICKS)` (new, section 5.3) over every remaining cap of the owner.
5. cancel the kernel watch token (a no-op if the death already consumed it) and `watched.remove`.
6. one log line only when something was reclaimed: `[net] owner death: cell C gen G: aborted A TCP, released U other` (normal clients close their sockets before exit, so the line appears exactly for leaks; no UART noise on normal exits).

The next turn's `iface.poll` (`service-runtime.rs:575`, run immediately because the death branch ends in `continue`) emits the RSTs. In the `hypervisor-bridge` feature build the poll only runs inside `dispatch_admitted` (`:398`, `#[cfg(not(feature = "hypervisor-bridge"))]` around `:526-579`); there the RST goes out at the next request or at the deadline [INFERENCE: that flavour is not built by the Tier-3 lane (`-p service-net` default features, lane script line 171)].

A new per-turn `reap_aborted(table, sockets)` next to `reap_graceful_closes` (`:584`) removes ledger entries whose RST has left or whose deadline has passed.

### 5.2 TCP semantics: RST (abort), recommended (D-NOD-3 = A)

smoltcp 0.11 API, read: `tcp::Socket::abort()` sets `Closed` (`tcp.rs:922-924`); with a connection tuple the next `dispatch` emits **one RST** (`:2111-2113` "outgoing segment will abort connection", `:2158-2160` `control = Rst`) and then clears the tuple (`:2320-2324`), after which `remote_endpoint()` is `None` (`:702-704`) — that is the completion predicate. A listener or never-connected socket has no tuple, so `abort()` sends nothing and it is removed at once. `close()` queues a FIN from `Established`/`CloseWait` (`:892-913`). `SocketSet::remove` is silent.

Why RST:

1. **The owner was killed, not finished.** A kernel kill is an abnormal termination (`cancel` -> `exit_task(root, usize::MAX)`); TCP's abnormal-close primitive is ABORT: send a reset, discard queued data. Linux does the same whenever the closing process has unread receive data (`tcp_close`: `data_was_unread` -> reset), which is the T3 case (cell ACKed window 0, i.e. a full receive buffer: observed).
2. **A FIN needs the peer's cooperation and will not complete here.** After `close()` the Net cell still has to receive the peer's FIN. In T3 the peer's data (`yes` flood) is queued ahead of its FIN behind a closed 4 KiB window nobody drains; the graceful path would sit in `FinWait2` until its 10 s deadline and `reap_graceful_closes` would then remove the socket **silently** (section 3), i.e. the failing behaviour again, only 10 s later.
3. **No post-mortem effects.** Today a leaked socket keeps retransmitting data the dead owner queued before it died. Abort discards the unsent buffer: after the owner's generation is dead it causes no further wire effect except the reset. This is the transport-level reading of ADR-0024 decision 6 / Spec 25 ("exactly-once side effects are not a transport guarantee"; cancel is "pre-dispatch refusal vs uncertain post-dispatch outcome", `cp1-draft.md` T3-D scenario): the client reports `unknown`/`cancelled`, never a silent late delivery.
4. **One state, one frame, bounded time.** Frees the slot (of 290) as soon as one segment is out, regardless of peer state or window.

Cost, stated: the peer sees "Connection reset" instead of a clean EOF, and bytes the dead cell had queued but not yet got acknowledged are lost. Option B (Linux-like hybrid: FIN after flush when the receive buffer and send buffer are empty, RST otherwise) preserves a gentle close for an idle killed client and is the closest to Linux, at the price of a second code path, a third socket state to prove and the same deadline problem for the non-idle case; not recommended for the first slice, listed as D-NOD-3 B. Option C (FIN only) is rejected by point 2.

[INFERENCE, not run] What the guest sees through slirp for an RST on flow A: slirp drops its connection, closes the host socket SA, the hostfwd accept socket SB sees EOF/reset, slirp B closes towards the guest. The T3 marker needs only "the producer is gone" (`guest.rs:76-83`), not a specific sshd log wording; the follow-up CP1 must not require "Connection closed by" over "Connection reset" (section 15).

### 5.3 Table primitives (`socket_table.rs`)

- `abort_owner(owner, sockets, deadline) -> (aborted_tcp, released_other)`: iterate the owner's caps with `BTreeMap::range` (no allocation, no `owned_cap` restart loop; caps skipped must not cause an endless loop). Per cap: in `graceful_closes` or already in `aborts` -> skip (5.4); UDP (`udp_caps`) -> `remove_internal` + `sockets.remove` (nothing on the wire); TCP -> `get_mut::<tcp::Socket>().abort()`; if `remote_endpoint().is_some()` the cap stays and is recorded in a new `aborts: BTreeMap<u64 cap, u64 deadline>` ledger, else (listener, never connected) it is removed at once. UDP is detected from the table, never by `get_mut::<tcp::Socket>` on a UDP handle (that panics, `socket_table.rs:41`).
- `begin_abort`, `is_aborting`, `aborting()` (ledger access, mirror of the graceful-close API at `:184-195`), `is_udp_cap(cap)` (unchecked variant of `is_udp`), and `remove_internal` also clears `aborts`.
- `remove_owner` (unused) is **deleted**; its test is retargeted.
- Bound: the ledger is at most `MAX_SOCKETS` (290) entries; the reaper handles up to 32 per turn.
- Abort deadline `ABORT_DRAIN_TICKS = 1_000 * MTIME_TICKS_PER_MS` (nominally 1 s at the assumed 10 MHz; same unit as the existing 10 s graceful deadline). The deadline is only a fallback for a TX path that cannot take a frame (neighbour entry expired, NIC queue full); on expiry the socket is removed silently as today and the peer's next probe is answered with RST by smoltcp's unknown-segment rule (`iface/interface/mod.rs:1215`, per the diagnosis).

### 5.4 Sockets already closing gracefully (D-NOD-4 = A)

A cap in the graceful ledger has already been accepted by Net as "send queued output, then FIN" and the owner may exit right after the reply (this is what `cellbox-ssh` does at `ssh.rs:1260`). Aborting it on owner death would cut the very flush the client asked for, a regression that exists only once the reclaim path is reachable. So `abort_owner` **skips** it and `reap_graceful_closes` finishes it under its own deadline. The old expectation that `remove_owner` also drops a gracefully-closing cap is wrong under this reading and is replaced by "the graceful cap survives owner reclaim and is reaped by its deadline".

### 5.5 Per-kind table

| Socket / state of the dead owner | Action | Wire effect |
|---|---|---|
| TCP established / close-wait / syn-sent / syn-received / fin-wait (not graceful-ledger) | `abort()`, ledger, reaper | one RST |
| TCP in graceful-close ledger | untouched, own reaper | FIN as already queued |
| TCP listener, never-connected, closed | `remove_internal` + `sockets.remove` now | none |
| Accepted sockets of a listener | owned by the accepting owner (`tcp.rs:187`), same rule | RST each |
| UDP (incl. bound, multicast) | removed now | none (IGMP leave not sent: existing `UdpClose` behaviour, out of scope) |
| Established TLS (cap in `tls_table`) | `tls_table` entry dropped, underlying TCP aborted | one RST, no close-notify |
| TLS connect/handshake in flight (`tls_pending`) | existing `cancel_owner`, silent drop | none (D-NOD-6) |
| Pending ready-wait / DNS query / queued TLS replies | removed / cancelled (existing) | none |
| `L2` bridge users (`NET_L2` right) | no sockets; nothing to reclaim [INFERENCE: not audited beyond the table] | |

### 5.6 Watch refusal policy (D-NOD-5 = A, fail-open kept)

Keep "a refusal must never deny live traffic" (`service-runtime.rs:628-630`) and the once-only log line. After the fix a refusal means (i) the principal died between kernel attestation and the watch call (a window of microseconds; the common death-while-in-flight case is already caught earlier because the dead sender's trailer parses as `None`, `:595-607`), (ii) Net is Paused or unregistered, or (iii) token exhaustion. Residual: one socket leaked until the owner's next request retries the watch or Net restarts. Option B (once any watch has succeeded, treat a refusal as "owner dead" and drop the request without creating state) closes (i); it adds a latch and a behaviour a mismatched kernel image could trip. Not recommended now.

### 5.7 Known limits left as they are (follow-ups, not in this slice)

1. `reap_graceful_closes` removes silently at its deadline although it is documented as an abort (`service-runtime.rs:304-321`); `httpd` phase-03 step 5 already plans "abort explicitly on deadline" (D-NOD-7: separate slice).
2. Legacy `TcpClose` / `TcpStream::drop` is silent by documented contract (`clients/net.rs:149-153`; `libs/api/src/services/ipc.rs:809`: "Legacy TcpClose remains an immediate abort", which it is not on the wire). A normal owner that drops a `TcpStream` leaves the peer to learn from the next probe. Distinct from owner death.
3. TLS handshake-in-flight sockets are dropped silently (a peer mid-handshake times out, or gets RST on its next segment).
4. No TCP keepalive or user timeout is set on any socket (peer vanishing is out of scope).

## 6. Delegation and pending-request model (answer to question 2)

Summarised from section 3: one principal per cap, exact `(cell_id, generation)` match on every call, no delegation operation, threads share the principal, worker death is not root death. Pending requests of a dead owner: ready-waits and DNS queries are cancelled by `release_owner`; deferred replies are not owed (kernel `async_ipc::on_owner_exits`, `scheduler.rs:1436`); TLS queued replies are dropped by `cancel_owner` (`tls/dispatch.rs:195`). A request that is in flight from the dying cell when the kernel kills it is delivered with a zero trailer (sender no longer attributable) and handled by the same unattributed branch: it is never dispatched, so it cannot create a socket for a dead owner.

## 7. Exact file closure (pre-edit SHA-256 at HEAD `296fecebc`)

| Path | Action | SHA-256 before | Locked surface |
|---|---|---|---|
| `kernel/src/task/scheduler.rs` | edit: `may_watch_owner_deaths` helper + predicate at `:662-668`; comments `:386`, `:654` | `2065efd47e19277b6dd22a6bbeb5665cf1426015720388165d1507f88cf1b82a` | kernel permission / owner lifetime |
| `kernel/src/task/syscall.rs` | edit: `Syscall::TryRecv` owner-death delivery + `take_owner_death_for_try_recv`; host tests in its `mod tests` (grant matrix, helper) | `a23eb08cd9e3b7bc19c3b1ac161b5b174c38ed0c93ab517ce4df851226e2e7c3` | kernel, shared with VFS receive path |
| `kernel/src/task/tcb.rs` | doc comments only (`:328-333`, `:888-894`); optional (D-NOD-8) | `160628a31c159e58f0a4066f9bc1034156e87d817be62351d23108fbc8cd7859` | kernel |
| `cells/services/net/src/socket_table.rs` | edit: `aborts` ledger, `abort_owner`, `begin_abort`/`is_aborting`/`aborting`/`is_udp_cap`, delete `remove_owner`, `remove_internal` clears the ledger | `76e8724b5d79ace59ec797a0001fab2fb38ce21fc89aec16f1061de3134f882d` | Net service authority (L37/L32 adjacency, L38) |
| `cells/services/net/src/socket_table/tests.rs` | edit: retarget `owner_cleanup_preserves_other_generation`; add ledger/abort tests (section 9.2) | `1ac75c8d6765ab1410eec28360b3dec54c14bc3a2d01a665fe472f10bd7af269` | tests |
| `cells/services/net/src/service-runtime.rs` | edit: `release_owner` order of 5.1, `reap_aborted`, `ABORT_DRAIN_TICKS`, one `#[path]` test module line, log line; `WATCH_REFUSED` string unchanged | `80c64227c53cf28fa01d20dd09094528f631d1419a4be81f832ba03c50b027ff` | Net service |
| `cells/services/net/src/service-runtime-owner-death-tests.rs` | **new** (section 8) | ABSENT | tests |
| `CHANGELOG.md` | edit at CP2 | `8866d0db4b1044c9c5567418ac9319d5941d7ac03d3d9c134c12270b76bdba03` | docs |
| `docs/network-api.md` | edit at CP2: "Owner death" subsection (RST, graceful ledger kept, deferred cases of 5.7) | `414459262cb183821d035f89cce8ca44e41ebcd8d392dd03bdb391cf5d948dea` | docs |

Six source/test paths (three kernel, three Net) plus one new test file, plus the two docs at CP2. **Pinned, must stay byte-identical:** `libs/api/src/abi/syscall.rs` `b202dfa28f461b2c8f9940901294187e65b6ef4ff94638dc1004c01d78325957`, `libs/api/src/abi/cell_owner.rs` `d6d14a5c115858262cb6b2961c96c6afe0a2a037e8ec1f53044ee272f3779e22`, `libs/api/src/abi/caller_identity.rs` `ac5886ae8f03fcef9cb3984bf89fb2c1c57c9154f94f76d17cb63602f5d2cb89`, `libs/api/src/services/ipc.rs` `53600a6a38bf48f72600af4d884035726af21db23c05bd16189610e9a54e9ba4`, `libs/ostd/src/syscall.rs` `95e4beb7e60561a061a6aed841bf2fc981a7a99e0c54716fcd3db4df7708c944`, `libs/ostd/src/clients/net.rs` `2344128b467f292f42d4dc4be6a2b45f56c62c4b6983dbca7be34bf156de54b4`, `kernel/src/cell/service_registry.rs` `3209e8328bc3b89bf8f38674e5703b9a8b470333b6d484b681a725455079fd2c`, `kernel/src/fast_ipc.rs` `4676aa66513c9f938544a46a69272d8f59818a648ea75db79a10b21747042d68`, `cells/services/net/src/main.rs` `76c486d06511d0ab72c567c2318b05326c88a2e795abdcc917a4116967766ced` (allowlist unchanged), `cells/services/net/src/handlers/tcp.rs` `3f1a7611f121586ab278eda239290db3d79a7777b9f7fd3e04fbc91487b72f98`, `cells/services/net/src/tls/dispatch.rs` `1d3feea4cb1cc986365bb23dfbdc4829764fbb29185a4e27181655b024adc3b5`, `cells/services/net/Cargo.toml` `aaab0a0c175e3e0eb3bbfb9606caf80c188fbb57292f4a9be665299096b6f33f`, `Cargo.lock` `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130`. **Protected (user's / locked / pinned elsewhere), never touched:** `kernel/src/main.rs` `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64`; `kernel/src/loader/launch_profile/targets.rs` `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f` and `launch_profile/mod.rs` (pinned by `scripts/qemu-phase04-terminal-test.sh:309-314`); `kernel/Cargo.toml`, `kernel/build.rs`, `kernel/src/embedded-x86_64/**`, `kernel/src/hypervisor/**`, `scripts/cellos_sign/**`, `scripts/assemble-x86-image.py` (digests in `cp1-draft.md` section 6.3).

Any other path discovered mid-slice stops the slice and returns to the owner as an amendment (Phase 02 A1 pattern). No dependency, feature, manifest, allowlist or `Cargo.lock` change.

## 8. New files

Only `cells/services/net/src/service-runtime-owner-death-tests.rs`, registered with the same `#[cfg(test)] #[path = ...] mod` form as `service-runtime-admission-tests.rs` (`service-runtime.rs:713-719`), so the 275-line admission harness (`Net`: device, interface, sockets, table, TLS and DNS state) is reused through `super::*` rather than extended. Nothing else new in the product tree; the follow-up lane edits (section 15) are not part of this slice.

## 9. Tests

### 9.1 Kernel host tests (`cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`, default and `--features phase04-driver-profile`; fixtures `with_scheduler_task_caps`, `publish_live_cell_owner`, `service_registry::register`, all under `crate::TEST_STATE_LOCK`; registry entries removed with `clear_tid` at the end)

1. `net_owner_watch_grant_matrix`: granted when the watcher's cell/generation equal the Active `NET` binding **and** `current_caller_*` equal the live client principal; refused for: unregistered cell, `CONFIG`/`INPUT`/`KMS` provider, NET entry `Paused` (`pause`), NET re-registered to another tid/generation (stale watcher generation), `current_caller` of another cell, stale client generation, dead client root, `cell_id == 0`; VFS cell still granted (regression); `cancel_cell_owner_watch` cancels only the watcher's own token.
2. `try_recv_takes_owner_death_only_for_mask_zero`: helper returns `(root_tid, reason)` and removes exactly that record for mask 0; mask != 0 leaves the queue intact; FIFO for two deaths; empty queue `None`; `pending_deaths` untouched.
3. `watch_survives_until_root_exit_then_queues_once` if the scheduler fixture allows `exit_task` on a synthetic root [INFERENCE: not checked]; otherwise the existing boot self-test of the VFS watcher (`vfs_lifecycle_selftest.rs:940-1060`, exit side unchanged by this slice) stands for the exit half and is cited, not extended.
4. Counts before/after recorded in the CP2 dossier (baseline **not measured** here).

### 9.2 Net host tests (`cargo test -p service-net --target x86_64-unknown-linux-gnu`)

`socket_table/tests.rs`: `abort_owner` per kind (5.5 table): SynSent TCP kept in the ledger with `remote_endpoint().is_some()` until a poll; UDP removed now; listener removed now; graceful cap skipped and still graceful; other owner and other generation of the same CellId untouched (caps and handles); second call returns `(0, 0)` (idempotent); no `get_mut::<tcp>` on a UDP handle (the test registers a UDP socket in the same owner); `can_insert` budget returns to baseline after the reaper; `remove_internal` clears the ledger; ledger bound with 258 sockets in one owner.

`service-runtime-owner-death-tests.rs` (new):
1. **RST on the wire, with a negative control.** Two TCP sockets on one smoltcp interface over `smoltcp::phy::Loopback`, connect and poll to `Established`; (a) control: `sockets.remove(client)` alone, poll, the server socket is still `Established` (proves the defect mechanism, silent removal); (b) `abort_owner` then poll: the server socket is `Closed` and `recv` fails (a reset was received), the client ledger entry is satisfied (`remote_endpoint()` is `None`) and `reap_aborted` frees the slot. Medium and feature availability of `Loopback` under the crate's smoltcp feature set (`alloc`, `socket-*`, `proto-ipv4`, no listed `medium-*`) is **not checked**; fallback if it does not compile: the admission harness `Net` with `VirtioNetDevice::push_rx` of a crafted ARP reply for the gateway and an `Established`-less proof (SynSent + RST frame counted in `tx_queue_len`) [INFERENCE].
2. `release_owner` end to end on the harness: owner with a TCP connect, a UDP socket, a listener, a pending DNS query and a ready-wait; after `release_owner` nothing of the owner remains except ledger entries; a second owner and a second generation untouched; the stale-owner sweep path (same CellId, new generation) releases the old one only.
3. `reap_aborted` fallback: with no frame possible, the entry is removed at its deadline and the table is back to baseline.
4. Graceful ledger entry of the dead owner is reaped only by `reap_graceful_closes` and its FIN is not pre-empted.
5. A pure helper extracted from the loop (`owner_for_root(&watched, sender)`) is tested for exact root-tid matching (worker tid does not match).

The `run()` loop itself is `#[cfg(target_os = "none")]` and not host-testable; its changed lines are the `release_owner` calls only.

## 10. Verification, lanes and run budgets

| # | Proof | What it proves | Budget |
|---|---|---|---|
| V1 | host: kernel and Net tests of section 9 | grant matrix, delivery helper, table reclaim, RST over smoltcp | build + test minutes each [INFERENCE; baseline counts not measured, capture before the first edit] |
| V2 | T3 `--stage diag` once, with the new outcome label (section 15) | the mechanism end to end on the original failure: pcap shows a single RST from `10.0.2.15` on flow A within ~0.1-1 s of the cancel mark, slirp/guest connection gone at snap-a, `yes` gone, host `ss` shows no SA/SB, no `owner death watch unavailable` line, the `[net] owner death:` line present, control still PASS | 1 boot; observed run times of the same stage: `driver-terminal` 162.3 s, elapsed 165.4 s (`cp2-dossier-part4.md` section 3) |
| V3 | T3 `--stage full` once | the claim: `ssh-guest-cancel-producer` PASS together with the 20 already-passing markers and the ledger rule | 1 boot; observed T3-D elapsed 127.1 s |
| V4 | regression: Phase 04 terminal lane on a private candidate (curl/wget/mqtt exits, `[selftest] boot-ceiling`) and the Phase 09 ssh lane's client-side cancel | no new `[net] owner death:` lines for normal clients, no change in the terminal lane check count (Q-era 466; S10-5 recorded 494/494 for the promoted bytes; **current baseline not re-measured**), refused-watch line gone | optional; each lane several minutes |
| V5 | new small smoke (D-NOD-9 B): a host Python TCP server on loopback reached through slirp `10.0.2.2`, a guest client killed by Ctrl-C, the server asserts RST/EOF within a bound | the fix independent of the SVM VMM and the guest | needs a new script and a terminal-driver scenario (more closure); **not recommended now**: V2 already carries the instruments |

Total QEMU budget proposed: **2 boots** (V2 + V3), plus 0-1 confirmation boot after an offline lane fix (as `cp1-approval.md` amendment allowed for T3-B/C). L5/L5f/S10 matched-candidate evidence is history: the changed kernel and Net bytes are not covered by candidate `4fae8b20…` and no row is upgraded by the old lanes (diagnosis section 7.1 consequence).

## 11. Lock handoff packet (form of `lock-handoff-k2-cp2.md` / `k3-cp1.md` section 11; DRAFT for the accountable owner, not submitted)

**11.1 Slice identity.** NET-OD: kernel owner-watch grant to the registered Net provider + `TryRecv` delivery of owner deaths + Net abort-on-owner-death with a drain ledger, host tests, `DEV_POSTURE`. Serves Cellbox CBX-23 cancel half and phase-09 item 5 (not an Intel C2C outcome; the owner is asked to confirm the same bounded-slice basis as Phase 02 and K2 CP2). Controlling plan `.agents/261011-1200-cell-contract-authority/plan.md` (status complete; completion unlocks nothing, procedure step 6, `locks.md:302`). Evidence reconciled: `cancel-diagnosis.md`, `cp2-dossier-part3/4.md`, `phase-04-s2n-cp2-dossier.md` addendum (owner-watch line pre-existing since `b0952aa90`), this document; no history erased.

**11.2 ADR-0024 contract delta (procedure step 2).**

| Principle | Effect of NET-OD |
|---|---|
| Cell principal/incarnation distinct from Task | the watch binds `(cell_id, generation)` and the root tid, attested from the live Cell generation (`resolve_live_cell_owner`); worker-thread exit stays distinct (section 2.1) |
| Shared per-Cell capability space | no new capability, handle or CapSet field; socket caps stay Net-local typed handles owned by one principal (resource authority stays with service-net, ADR-0024 decision 4) |
| Explicit delegation and attenuation | no delegation exists in Net; owner death releases every cap; a future delegation op must release references (R-8) |
| Revocation generations | the grant compares the registered Net **generation**; a stale generation cannot watch; the dead owner's generation never matches again |
| Kernel substrate vs ordinary policy Cells | the kernel attests a death and a registry-bound right; the TCP semantics (abort, drain) stay in service-net; no networking logic enters the kernel |
| Durable logical state vs execution | none (sockets are volatile; Net restart already drops them) |
| Caller/test/doc migration, concurrency/remote | callers: VFS unchanged (its `Recv` path untouched; `TryRecv` gains earlier owner-death delivery); Net is the only new watcher. Tests: section 9. Docs: `CHANGELOG.md`, `docs/network-api.md`. Concurrency: same lock order (SCHEDULER -> REGISTRY); one SocketSet owner stays. No remote impact |
| No ambient credential / remote shortcut | the right needs the registered Net binding **and** the live serving context; Net learns only the death of a principal that just sent it a message |

**11.3 Locked rows and overlapping plans (procedure step 3).** Read-only; `git status --short` empty at HEAD `296fecebc`; the last commits touching these paths are `47a9d5bda` (KSSH K3 CP2), `0812da113` (net DNS slots), `2f0bc0659`, `8fe158478`; the KSSH program is complete at `DEV_POSTURE` (`ca4b872e6`), so no KSSH slice is in flight under `kernel/src/task/**` or `cells/services/net/**`.

| Row / plan (`locks.md`) | Blocked scope | NET-OD relation | Needed from the owner |
|---|---|---|---|
| **L38** `261006-0934-httpd-single-cell-concurrency` | "all unfinished reactor, async IPC, completion-capacity and connection-lifecycle slices" | **Direct, same item.** Phase 03 step 6 (`phase-03-network.md:23`): "Reclaim owner-generation sockets/waits on cell death/restart through attested existing death mechanism (extend shared primitive only through governance if absent) ... Test root cell death separately from worker-TID exit", and its assumption "exact owner-death watch suitable for net is not yet verified; design it in Phase 01 under the shared ABI gate if absent". NET-OD is that primitive for the Net service; no ABI is involved. The same plan edits `socket_table.rs`/`service-runtime.rs` heavily (capacity budgets, graceful drain, abort on deadline), so it must **rebase on top of** NET-OD (rebase order below) | adoption as a bounded slice; one reciprocal note on phase-03 step 6 ("delivered by NET-OD; remaining: abort on graceful deadline, capacity budgets") |
| **L2** `260603-1803-phase-h-vfs-hardening` | kernel permission / VFS mutation authority | Direct for the kernel files (K3 named this row for kernel-permission slices) | file-bounded exception for the kernel paths of section 7 |
| **L23** `260809-0922-sas-lbi-revocable-vfs-access` | owner-scoped handles, caller migration, closure gates | **Direct for the mechanism**: the owner watch, `current_caller` rule and VFS lease quarantine were built here (phase-03 lifecycle checkpoint). NET-OD does not touch VFS lease or quarantine code, only the predicate and a second receive kind | owner confirms no in-flight edit claims `scheduler.rs`, `syscall.rs`, `tcb.rs`; no VFS behaviour change |
| **L35** `260927-0739-kernel-architecture-repair` (in-progress) | grant/domain, snapshot, qualification | check: same files, different functions | start-of-slice file-claim re-check |
| **L8** `260621-0830-cell-perms-p2-p5` | deferred syscall delegation, parameterised authority | check: adds no syscall, delegation or CapSet right | owner rules compatible |
| L21 `260801-parallel-midori-closure`, L36 `260927-1100-c2c-anywhere-tier-aware`, L33 `260827-1015-sas-caller-owned-range-ledger`, L9 `260624-0630-kernel-boundary-cleanup` | completion/peer-death, async IPC, owner-generation leases, kernel/driver ownership | adjacent: not edited; the `vfs-lifetime self-test PASS` text (quoted by `260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md:330` and matched by `scripts/x86/qemu-domain-test.sh`, `scripts/qemu-aarch64-test-hooks.sh`) stays byte-identical | note only |
| **L37** `261004-1957-x86-pc-lane` and L32 `260827-1004-hardware-independent-roadmap` (and L25, L36) | device authority; hardware-independent contract scheduling | **Adjacent, as recorded by the S10-10 probe** (`tier3-and-guards.md`: a Net-cell change "would touch the Net service authority (L37/L32 adjacency, protected)") and by `cp1-draft.md` section 9 (contingent handoff template for `cells/services/net/**`). Net service authority = per-owner caps and attested admission, unchanged | owner names the Net file-owner and rules the adjacency compatible |
| L11/L13/L18 (Tier-3 VMM), `260825-1726` and `260826-1605` (KMS), `260712-1900` (manifest v2), L25 `260821-0642-app-tiers-completion` | | no path of NET-OD lies in their scope | none |
| Plans not examined for overlap: `260712-1902-dice-attestation-identity` P03, `260825-sdk-delivery` P02, `260819-1409-cell-to-cell-anywhere-core` P02, controlling `migration-ledger.md` | | grep-level only (owner-death strings in `260905-1139-sas-lbi-outcome-closure/phase-02`, `261019-1310-unified-desktop/phase-01,03`, `261020-1750-cellbox-cli/phase-06`: VFS-side consumers, not Net) | owner confirms no overlap or names one |

**11.4 Retained gates (procedure step 4).** Law 1: not triggered (section 12); the owner is asked for an explicit "no ABI delta" acknowledgement, not two confirmations. Protected identity / non-rollback time/state: not applicable (no storage, no time). Independent human decisions: D-NOD-1..13. No hardware, AWS or production gate is touched; `DEV_POSTURE` only. A green host test does not replace these gates.

**11.5 Scheduling change (step 5; text only).** In one change: record "NET-OD" as a bounded adopted slice with acceptance ceiling "kernel grant + delivery + Net abort/ledger, host tests, T3 diag/full QEMU observation, `DEV_POSTURE`" in the controlling plan, `plan-portfolio.md` (the Tier-3 paragraph at `:129` already says "the fix ... needs its own CP1 and lock handoff") and `cell-contract-0024-locks.md` next to the K3/K5/K7/K4b paragraphs, as a file-bounded exception for the section-7 paths with this packet's digest; narrow only the applicable relations of L2 and L38 (step 6 of httpd phase 03), keep every whole-plan relation. WIP stays one implementation slice; single integrating owner.

**11.6 Execution boundary and rebase order once adopted.**

1. Baseline first: host test counts of `cellos-kernel` (default and `--features phase04-driver-profile`) and `service-net`; `git status` clean; start-of-slice overlap re-check (11.3) and digest re-check of section 7.
2. Edits only in the section-7 paths; pinned files re-verified; `kernel/src/main.rs` and every protected path untouched (STOP otherwise); no network, no key, no VM during implementation; no `Cargo.lock` change.
3. **Rebase order:** (a) KSSH K3-K8 and the Tier-3 lane work are already landed; (b) NET-OD lands next, as its own commit (the Tier-3 lane preflight refuses a dirty tree outside its closure, `qemu-tier3-ssh-guest-x86.sh` `BAD` check), kernel and Net in one change so that the pair is never mismatched; (c) then the follow-up T3 CP1 (section 15); (d) the httpd phase-03 slices rebase onto NET-OD when and if the owner releases them; (e) a canonical re-promotion (S10-5 style) is a separate decision: the promoted candidate `4fae8b20…` and the canonical `kernel/src/embedded-x86_64/**` keep the defect until then.
4. **Rollback:** restore the section-7 pre-edit hashes (source reversal; no persistent state). A new-kernel/old-Net pair is safe (the old Net never calls a granted watch again: refusal-tolerant); an old-kernel/new-Net pair is also safe (watch refused, the once-only log line, today's behaviour). Neither mismatch can leak less than today.

**11.7 Decisions requested from the accountable owner.** (1) Adopt NET-OD as a bounded slice with the section-7 file-bounded exception. (2) Name the Net file-owner and the kernel-permission owner; rule L37/L32/L8/L35 compatibility and L23/L38 as in 11.3. (3) Acknowledge that no `libs/api`/`libs/types` byte changes and that `TryRecv` mask 0 delivering owner deaths is a behaviour clarification of existing syscalls (section 12). (4) Approve the one scheduling record (11.5). Nothing in this packet authorises a source edit.

## 12. Law-1 analysis

`code-standards.md:42` scopes Law 1 to `libs/api/` and `libs/types/`. NET-OD changes **no** byte of either tree and no constant, payload or discriminant they define: syscall numbers 244-248 and `ViSyscall` (`libs/api/src/abi/syscall.rs`), `CellOwner`/`CellOwnerRequest` layouts and `CELL_OWNER_LEN`/`CELL_OWNER_REQUEST_LEN` (`cell_owner.rs`), the 32-byte trailer and flag bits (`caller_identity.rs`), `NetRequest`/`NetResponse` and the `0xFC`/`0xFF` codes (`services/ipc.rs`), `libs/ostd` syscall wrappers. The Net allowlist (`declare_syscalls!`) and manifest are unchanged. Therefore **no two-confirmation checkpoint is requested** and `libs/api` doc comments are left as they are (the doc of 244 describes Resolve and stays true; the doc of 245 says "same arguments as `ResolveCellOwner`", which is a pointer to the argument shape, not a permission statement).

Two behavioural points the owner should see, because they are observable by existing code even without a byte change:

1. **Who may hold a watch** widens from "the VFS cell" to "the VFS cell or the registered Net cell serving that principal". Kernel permission table, not ABI.
2. **`TryRecv(mask = 0)` now returns a queued owner death** the way `Recv` does. Only a task that holds a watch has such events, so today only VFS is affected, and for VFS this is the same branch handled at `vfs/src/main.rs:177-190` (earlier delivery, no new case). `TryRecv` callers without a watch see no change; `NotifyOnExit` deaths are not added to `TryRecv`.

If the owner reads (2) as an ABI reinterpretation, the alternatives are: a new syscall (Law 1, two confirmations, new number in `libs/api` and `libs/ostd`, rejected as heavier for the same effect), or making Net park in `Recv`/`RecvTimeout` (rejected: Net must also wake on NIC RX completions; `RecvTimeout` would lose that wake and rework the loop's fairness, `service-runtime.rs:699`). Neither is recommended; both would reopen D-NOD-2.

## 13. Other services that would need the same (out of scope; list only)

[INFERENCE: not audited beyond the greps listed here.] The shared kernel rule is the generalisation point: "any registered-service provider may watch the principal it is serving" would be one predicate (D-NOD-1 B) and would cover these in one step; each still needs its own consumer change and CP1.

| Service | Evidence of per-client state | Note |
|---|---|---|
| **DNS** | in-process in Net (`dns.rs`, `pending_dns`, one shared UDP socket): the dead owner's queries are cancelled by `release_owner` | covered by NET-OD, not a separate service |
| `cells/services/ai` | `Session{owner, last_activity}` reclaimed by a 30 s idle deadline because "A service Cell cannot watch another Cell's exit" (`ai/src/main.rs:91-96`) | would replace the deadline by prompt release |
| `cells/services/input` | per-holder sessions and frontends (`session.rs:185-217`), `Status::Stale` selectors | holders of a dead cell linger until superseded [INFERENCE] |
| `cells/services/net-broker` | per-caller windows, stale-reply ring, peer sessions (`local_queue.rs`, `c2c_dedup.rs`) | caller-death cleanup by queue/deadline [INFERENCE] |
| `cells/services/config` / `compositor` / `kms` | per-caller state keyed by generation; KMS K5 import/unlock slots are replaced by the next caller so "a dead client cannot wedge unlock" (`k5-cp1.md` section 9.1) | no leak of authority; state is bounded |
| `cells/services/httpd` | is a Net **client** with a `TcpCloseGraceful` lifecycle (`reactor.rs:568`); its own death is covered by NET-OD | plan L38 |
| `cells/services/silo`, `hypervisor`, `supervisor`, `placement-controller`, `workload-router`, `budget-authority` | not read for client-death handling | listed for completeness |

## 14. Risks

- **R-1 Data loss on a killed owner.** Abort discards queued, unacknowledged bytes; that is the chosen semantic (5.2), now stated, with the Linux-like hybrid as D-NOD-3 B.
- **R-2 Graceful-ledger sockets of a dead owner** still end silently at their deadline (5.4, 5.7 item 1); not worse than today, fixed by D-NOD-7's separate slice.
- **R-3 TX path cannot emit the RST** (neighbour expired, NIC queue full): the deadline fallback removes silently; the peer's next probe is answered by smoltcp's unknown-segment RST. Bounded, observable in V2.
- **R-4 Watch race (5.6)**: one socket may leak when the owner dies in a microsecond window; self-heals on the owner's next request or at Net restart.
- **R-5 `TryRecv` change reaches VFS** (earlier owner-death delivery): same handler; guarded by leaving `Recv` untouched and by test 9.1 item 2; the VFS lifetime boot self-test and its two lanes are not edited.
- **R-6 Digest pins**: forgetting to keep `targets.rs`/`mod.rs` byte-identical fails `qemu-phase04-terminal-test.sh:309-321`; this slice does not touch them.
- **R-7 Overlap with httpd phase 03** (same Net files): mitigated by the rebase order (11.6) and the reciprocal note.
- **R-8 Future delegation**: owner death must become reference release; recorded so the Net cap model does not grow a transfer op without it.
- **R-9 Promoted bytes**: the canonical/promoted image keeps the defect until a re-promotion slice; do not read a green T3 lane (a fresh lane-only build) as a statement about promoted bytes.
- **R-10 Stale-watch growth**: each distinct client generation costs one kernel map entry until its death or Net's death (`scheduler.rs:1491-1494`); bounded by live cells.
- **R-11 Other architectures**: aarch64/rv64 kernels share `scheduler.rs`/`syscall.rs`; the change has no `cfg` split, but only the x86-64 host target was reasoned about [INFERENCE: other builds not run].

## 15. Follow-up gate: the T3-D rerun needs a new CP1 (what it would contain)

Authority today: the approved Tier-3 CP1 forbids a rerun after a marker FAIL without a new CP1 (`cp1-draft.md` section 5; `cp1-approval.md`), the diag CP1 forbids an unchanged rerun (`cp1-diag-approval.md`), and `evidence/t3-d-runs/` holds `RUN_CONSUMED-1`, `DIAG_CONSUMED-1`, `DIAG_CONSUMED-2` (the lane claims tokens with `O_EXCL`, `qemu-tier3-ssh-guest-x86.sh:285-297`; `RUN_CONSUMED-2` is free but reserved for host errors). A rerun is therefore **not** authorised by NET-OD. The new CP1 ("T3-D3") would contain:

1. **Precondition record:** NET-OD landed as a commit (the lane preflight refuses a dirty tree outside its closure); new HEAD; section-7 post-edit digests; V1 results; protected-path tree digests (`kernel`, `cells/services` now differ from the T3-D baseline by design: the lane compares before/after **within** a run, `protected_digest`, so only the one-time record of the new baseline is needed).
2. **Authorised runs:** one `--stage diag` (V2) and one `--stage full` (V3); at most one further boot only for a host/environment error; hard windows unchanged (diag 3600 s, full 4 h; observed 165 s and 127 s). Budget accounting: either new token prefixes (a lane edit: `DIAG2_CONSUMED`, `RUN2_CONSUMED`) or an explicit statement that `RUN_CONSUMED-2` is claimed by the product-fix rerun; the diag stage needs a new prefix because both of its tokens are spent.
3. **Lane/judge edits (lane-only, inside the lane's allowed dirty list):** a new diag outcome label `O7` "reclaimed" (the classifier at `tier3_ssh_guest_judge.py:520-545` has no label for success and would print `O6`), with its criteria: control PASS; cancel marker PASS; **RST from the cell** on flow A after the mark (`cancel_A_fin_rst_from_cell` true, one RST, no zero-window probe ACKs after it); slirp/guest connection absent at snap-a (guest `/proc/net/tcp`, host `ss` SA/SB gone); `yes` gone; serial contains `[net] owner death:` and does **not** contain `owner death watch unavailable`. Do **not** require a specific sshd wording (`Connection closed by` vs `Connection reset`), only that the guest connection ended. Host tests of the judge extended for O7 and for the new STOP rule "watch-unavailable line present => the fix is not in the image". Optionally also lift the full stage's `ssh-guest-cancel-producer` from "may FAIL" to the required marker set (already required there).
4. **Claims allowed, same ceiling statement as the approved CP1** (SVM VMM, QEMU-TCG 10.2.0, one loopback hostfwd, password, private lane-only image; not Intel VMX, not KVM, not production, not matched-candidate bytes): replace the cancel-half wording of `cp2-dossier-part4.md` section 8 by "a Ctrl-C cancel of a guest `ssh --command` kills the client; the Net service aborts the dead owner's socket (RST), the guest sshd session ends and the producer stops (`evidence/t3-d3/`)" **only if V2 and V3 pass**; CBX-23's row wording then needs the owner's decision (D-T3-8 pattern); CBX-25 unchanged; the S9-6 `ssh-cancel-exec` record stays client-side only.
5. **Stop rules:** keep STOP-1..6; add STOP-7 "watch-unavailable line in the serial log"; no fix-and-rerun after a marker FAIL or an O6 without a new diagnosis; a second FAIL ends the program for this row.
6. **Not authorised by that CP1:** any further product edit, candidate import, promotion, Intel/KVM run, or network fetch (the apk set and QEMU 10.2.0 are already pinned and cached; the preflight checks them).
7. **Cleanup and evidence:** same as part 4 section 9 (keys, ISO, pcap deleted; text evidence secret-scanned and archived under `evidence/t3-d3/`).

## 16. Not checked

No compile, test, lane or QEMU run; current kernel and `service-net` host test counts and whether they are green at HEAD; whether the scheduler host fixture can `exit_task` a synthetic root (9.1 item 3); whether `smoltcp::phy::Loopback` and a medium feature are available to the Net crate under `cargo test` (9.2 item 1); the TX-queue behaviour of `VirtioNetDevice` and e1000 under a burst of RSTs; the wake latency of `sys_wait_completion(NET_RX, 10)` for a queued death (reasoned as at most one 100 ms maintenance turn); what slirp, QEMU hostfwd and the guest sshd log for an RST on flow A; `hypervisor-bridge` flavour behaviour; aarch64/rv64 builds; the other services of section 13 beyond the quoted lines; the plans listed as unexamined in 11.3; that init registers Net as `NET` in the Tier-3 lane's private plan (assumed from `cellbox`/guest connectivity working and from `service_table.rs:90` / `config/services.toml:57-62`); whether anything else reads `pending_owner_deaths` (grep: VFS self-test and `Recv`/`RecvTimeout` only).

## 17. Effort and entry gates

**Effort S-M** (about 1 week wall time including approvals): kernel about 50-80 production lines plus 150-250 test lines; Net about 80-140 production lines plus 200-300 test lines [INFERENCE]; docs small. Add 1 day for V2/V3 and the follow-up CP1 lane edits.

Entry gates: (1) decisions of section 18 answered and bound in a `net-owner-death-cp1-approval.md` (digest of this file at binding); (2) lock handoff of section 11 approved, including the Net/kernel owner names and overlap rulings; (3) baseline capture and start-of-slice re-check; (4) owner acknowledgement of section 12; (5) the follow-up T3 CP1 is a separate approval and is not a gate for landing NET-OD.

## 18. Decisions (OPEN until the user answers; recommendations in the last column)

| ID | Decision | Options | Recommendation |
|---|---|---|---|
| D-NOD-1 | Kernel grant predicate | **A** registry-bound Active `NET` provider cell + generation + attested serving context (this text); **B** any registered-service provider may watch the principal it serves (covers section 13 at once, wider authority); **C** new CapSet right + policy v9 + signer (K3 style: heavier, Law-adjacent signed-artifact change); **D** static `set_net_handler_cell` in `loader.rs` (VFS style) | **A** (smallest, no new state, unforgeable by name; B recorded for a later service-by-service CP1) |
| D-NOD-2 | Delivery of the death to Net | **A** `TryRecv` mask 0 delivers one owner death like `Recv` (kernel `syscall.rs`); **B** Net parks in `Recv`/`RecvTimeout` (loses NIC RX wake); **C** new syscall (Law 1) | **A** |
| D-NOD-3 | Wire semantics on owner death | **A** RST (`abort`) for every TCP socket not in the graceful ledger; **B** Linux-like hybrid (FIN after flush when buffers are empty, RST otherwise); **C** FIN only | **A** (section 5.2: killed owner, unread data, FIN cannot complete, no post-mortem effects) |
| D-NOD-4 | Sockets already in the graceful ledger | **A** leave to their reaper; **B** abort too | **A** (a cellbox-ssh exit right after `TcpCloseGraceful` must still flush) |
| D-NOD-5 | Refused watch | **A** fail-open as today (log once); **B** after a first success, refusal = dead owner, drop the request | **A**; B only if a leak is observed |
| D-NOD-6 | TLS handshake-in-flight sockets | **A** keep the silent drop (`tls/dispatch.rs` untouched); **B** abort them too (3-line edit in `tls/dispatch.rs`, one more file in the exception) | **A** |
| D-NOD-7 | `reap_graceful_closes` silent removal at its deadline | **A** separate follow-up (httpd phase-03 step 5); **B** include here | **A** |
| D-NOD-8 | Doc-only edit of `tcb.rs` comments | **A** include; **B** skip (stale comments) | **A** |
| D-NOD-9 | Proof | **A** host tests + T3 `diag` + `full` (2 boots); **B** also a new small QEMU smoke with a host Python server (V5); **C** host tests only | **A** |
| D-NOD-10 | Lock treatment | **A** bounded adopted slice with the section-7 file-bounded exception, narrowing L2 and L38 step 6, reciprocal notes; **B** wait for the L38 plan to be released and fold the work into it | **A** (L38 is pending/blocked and depends on a Law-1-gated async prerequisite; the kernel half is independent of it) |
| D-NOD-11 | Law-1 reading | **A** no `libs/api`/`libs/types` delta, owner acknowledges the behavioural clarification of 12; **B** treat `TryRecv` delivery as an ABI delta and run two confirmations | **A** |
| D-NOD-12 | Promoted/canonical bytes | **A** unchanged by this slice, re-promotion separate; **B** include a promotion | **A** |
| D-NOD-13 | T3-D rerun | **A** new CP1 after landing (section 15), 1 diag + 1 full; **B** full only; **C** none | **A** |
