# P11 census — area: libs-kernel (HEAD 0f81a6597)

Scope: `libs/**` blocking sends + SDK wrappers (task 1), task-state readers outside the kernel (task 2), kernel `fast_ipc::` / legacy `Syscall::` arms / dead `Task` fields / kernel `Rename` (task 3). Read-only census; every row was opened and verified.

## 1a. Blocking sends under `libs/` (`sys_send(` and raw `ViSyscall::Send`)

| file:line | enclosing fn | class | peer (service / tid source) | note |
|---|---|---|---|---|
| libs/ostd/src/syscall.rs:1432 | `sys_send` | — (definition) | caller arg | `ViSyscall::Send` = opcode 0; parks in `TaskState::Sending`, no deadline |
| libs/ostd/src/app.rs:305 | `AppContext::send` (also via `send_msg` :315) | UNCLEAR | caller-supplied tid | Generic wrapper. All 11 in-tree callers are REPLY to `sender_tid` (hypha llm-gateway:82, tools/fs:72, tools/spawn:66, tools/sys:60, hotswap-demo-v1:133/143/145, hotswap-demo-v2:111/120/122, sdk-demo:40 via `send_msg`); `ActorCtx::call` (actor/mod.rs:153) uses it as REQUEST |
| libs/ostd/src/dispatch.rs:100 | `run_service` | REPLY | `sender_tid` from `sys_recv(0)` | `run_service`/`MessageHandler` have **0** in-tree callers (dead SDK path) |
| libs/ostd/src/input.rs:65 | `input::request_focus` | EVENT | INPUT via `sys_lookup_service` (tid-only) | `InputRequest::SetFocus`, documented fire-and-forget, no reply |
| libs/ostd/src/input.rs:85 | `input::release_focus` | EVENT | INPUT via `sys_lookup_service` (tid-only) | `ClearFocus`, fire-and-forget; :93/:96 are comments documenting a send/send deadlock workaround (`drain_pending_input_events`) |
| libs/ostd/src/io.rs:273 | `Stdin::read_line` | — (comment only) | — | No call here; real send is `crate::input::request_focus()` at io.rs:283 (INPUT tid-only lookups at :268/:280) |
| libs/ostd/src/tls.rs:52 | `tls_connect` | REQUEST | NET, caller-supplied `net_tid` | send result ignored; `recv_from(net_tid)` unbounded |
| libs/ostd/src/tls.rs:73 | `tls_write` | REQUEST | NET, caller-supplied | send result ignored; unbounded `recv_from` |
| libs/ostd/src/tls.rs:122 | `tls_read` | REQUEST | NET, caller-supplied | unbounded `recv_from` |
| libs/ostd/src/tls.rs:137 | `tls_close` | REQUEST | NET, caller-supplied | 1-byte ack, result ignored |
| libs/ostd/src/clients/kms.rs:182 | `KmsClient::call` (via `call_opcode`) | REQUEST | KMS tid cached by `KmsClient::connect` (kms.rs:64, `sys_lookup_service`, tid-only) | `sys_recv(self.tid)` unbounded; 14 public methods route here |
| libs/ostd/src/clients/kms.rs:288 | `KmsClient::exchange` (via `call_ssh`/`call_bounded`) | REQUEST | KMS tid-only (as above) | recv bounded by `sys_recv_timeout`, send itself unbounded; 8 SSH methods route here |
| libs/ostd/src/display/ipc.rs:11 | `send_lifecycle_request` | EVENT | compositor `comp_tid` (from `wait_for_compositor`, tid-only) | one-way: `set_title`, `set_appearance`, `set_icon`, `minimize/maximize/restore` (`request_state`), `respond_close` (lifecycle.rs:22/33/42/152/159) |
| libs/ostd/src/display/ipc.rs:23 | `create_surface` | REQUEST | compositor `comp_tid` | send result ignored; `sys_recv(comp_tid)` loop, unbounded |
| libs/ostd/src/display/ipc.rs:73 | `configure_ack` | REQUEST | compositor | `receive_status(…,0x01)` unbounded |
| libs/ostd/src/display/ipc.rs:81 | `detach_grant` | REQUEST | compositor | `receive_status` |
| libs/ostd/src/display/ipc.rs:89 | `detach_replaced_grant` | REQUEST | compositor | `receive_status` |
| libs/ostd/src/display/ipc.rs:117 | `stage_grant` (`attach_grant`) | REQUEST | compositor | recv loop, routes interleaved compositor events |
| libs/ostd/src/display/ipc.rs:156 | `destroy_surface` | REQUEST | compositor | `receive_status(…,0x00)` |
| libs/ostd/src/display/lifecycle.rs:185 | `impl Drop for ViSurface::drop` | REQUEST | compositor | DETACH_GRANT + `receive_status`, then `destroy_surface` (2 blocking round-trips inside `Drop`) |
| libs/ostd/src/display/surface.rs:133 | `ViSurface::damage` (+`damage_all`) | EVENT | compositor | display damage, result ignored |
| libs/ostd/src/display/surface.rs:155 | `ViSurface::move_to` | EVENT | compositor | result ignored |
| libs/ostd/src/display/surface.rs:163 | `ViSurface::raise` | EVENT | compositor | result ignored |
| libs/vimio/src/lib.rs:290 | `Selector::select` | REQUEST | NET via `sys_lookup_service` (:269, tid-only) | `NetRequest::TcpReady`; reply via `sys_recv_timeout(0, …)` **wildcard mask** |
| libs/vipolling/src/lib.rs:272 | `Poller::wait` | REQUEST | NET via `sys_lookup_service` (:251, tid-only) | same pattern as vimio (duplicated code) |
| libs/api/src/services/posix/net.rs:94 | `net_call` (raw `ViSyscall::Send`) | REQUEST | NET via raw `LookupServiceBound` (`net_provider`) | raw Send + raw `Recv(mask=tid)` unbounded; reply re-checked by `admit_net_reply` |
| libs/api/src/services/posix/sysio.rs:292 | `vfs_call` (raw `ViSyscall::Send`) | REQUEST | VFS via raw `LookupService` (sysio.rs:262), tid cached in `VFS_TID_CACHE` (tid-only) | raw `Recv(mask=vfs)` unbounded; cache dropped on failure |

`sys_send_gather` / `sys_post` (syscall.rs:1478/1495, opcode 202) is a non-parking post — out of scope.

### Totals (1a, 25 real sites; definition + comment rows excluded)
- **REPLY: 1** (dispatch.rs:100, dead path)
- **EVENT: 6** (input.rs:65, :85; display/ipc.rs:11; surface.rs:133, :155, :163)
- **REQUEST: 17** (tls ×4, kms ×2, display/ipc ×6, lifecycle Drop ×1, vimio, vipolling, posix net, posix sysio)
- **UNCLEAR: 1** (app.rs:305 generic `AppContext::send`; callers are effectively REPLY)

## 1b. Public SDK functions wrapping a blocking send or a tid-only lookup

Caller counts = non-comment, non-definition lines in `cells/` / `libs/` (rg).

| SDK fn (definition) | wraps | cells callers | libs callers | note |
|---|---|---|---|---|
| `ostd::syscall::sys_lookup_service` (syscall.rs:1123) | tid-only lookup (opcode 206) | 130 lines / 85 files | 16 (ostd) + 3 (terminal/stream.rs:186, vimio:269, vipolling:251) | the dominant pattern |
| `ostd::service::lookup` (service.rs:31) | tid-only | 7 | 3 (fs.rs:81 `File` vfs_tid, syscall.rs:594 spawn-from-path, clients/tls_stream.rs:61) | |
| `AppContext::lookup_service` (app.rs:327) | tid-only | 0 | 1 (`ActorCtx::lookup`, actor/mod.rs:201 — itself 0 callers) | |
| `ostd::display::wait_for_compositor` (display.rs:38) | tid-only, spins forever | 13 | 3 (port-platform:52, viui elm.rs:444, viui window.rs:410) | feeds every ViSurface send |
| `ostd::input::request_focus` (input.rs:53) | tid-only + blocking send (EVENT) | 17 | 6 (app.rs:337, clients/input.rs:31, io.rs:283, repl.rs:173, port-platform:57, viui input_bridge.rs:33) | `InputClient::request_focus` and `AppContext::request_focus` delegate here |
| `ostd::input::release_focus` (input.rs:76) | tid-only + blocking send (EVENT) | 2 (ocel:2665, cellbox executor:2956) | 0 | |
| `ostd::input::drain_pending_input_events` (input.rs:98) | tid-only (try_recv) | 1 (cellbox executor:2951) | 0 | exists only to break a Sending↔Sending deadlock |
| `AppContext::send` / `send_msg` (app.rs:304/315) | blocking send | 10 / 1 | 1 (actor) | all cell uses are replies |
| `ActorCtx::send_msg` / `reply` / `call` (actor/mod.rs:121/139/148) | blocking send (+ unbounded masked recv in `call`) | 1 (`reply`, tests/backend lib.rs:87) | 0 | `call`, `send_msg` unused |
| `dispatch::run_service` (dispatch.rs:73) | blocking reply | 0 | 0 | dead |
| `tls::tls_connect/write/read/close` (tls.rs:39/64/109/133) | blocking request | 4 / 2 / 2 / 6 (llm-gateway transport.rs, https-demo) | 1 each (clients/tls_stream.rs) | |
| `clients::TlsStream::connect` (tls_stream.rs:60) | tid-only + tls_* | 1 (http-smoke:120) | 0 | |
| `clients::KmsClient::connect` (kms.rs:62) + 22 methods | tid-only + blocking request | 6 (net-broker kms_dh:140, net-broker main:99, phase04 kssh:1107, silo-test:31, cellbox ssh:648, kms-ssh-admin:178) | 1 (test `scripted`) | |
| `ViSurface::create` + lifecycle (`set_title`, `set_icon`, `set_appearance`, `apply_configure`, `respond_close`, `minimize/maximize/restore`, `Drop`) | blocking EVENT/REQUEST | create 15 | create 5 | approximate method counts (files mentioning ViSurface): set_title 13, set_icon 10, set_appearance 1, apply_configure 3 (+2 viui), respond_close 2 (+3 viui), min/max/restore 1 |
| `ViSurface::damage/damage_all/move_to/raise` | blocking EVENT | ≈14 / ≈7 / ≈7 (name-collision caveat for `move_to`) | viui ≈5 | |
| `vimio::Selector::select`, `vipolling::Poller::wait` | tid-only + blocking request | dependents: haily-agent, async-echo, tokio-smoke | — | |
| posix `net_call` / `vfs_call` (libs/api/src/services/posix) | raw Send+Recv | linked into C/posix cells | — | `vfs_call` tid-only cached |
| `cluster_endpoint::LocalEndpoint::new` (cluster_endpoint.rs:70) | tid-only descriptor | 0 | 5 (libs/ostd/tests/cluster-endpoint.rs only) | `call` uses exact-op `service_call_typed` (not blocking send) |
| `LocalEndpoint::bind` (:94) | bound lookup | 1 (bench local_service_lifecycle.rs:1551) | 0 | |
| `LocalEndpoint::call` (:126) | exact-op | **0** | 0 | |
| `RemoteEndpoint::call` (:200) | none — always `RemoteCallError::NotSupported` | 0 | 1 (tests/cluster-endpoint.rs:52) | |
| `service::ServiceRef::call` (service.rs:168) + aliases `VfsRef/NetRef/InputRef/ConfigRef/CompositorRef` | bound lookup (`LookupServiceBound`) + exact-op | 33 refs (~21 files incl. bound lookups) | 23 refs (VfsClient/NetClient/InputClient wrap it) | already compliant |
| `ipc::service_call*` / `PendingCall` (ipc.rs) | exact-op (`IpcSubmit`/`IpcTake`) | — | — | compliant; caller passes a tid (often from tid-only lookup, e.g. fs.rs:289 ← fs.rs:81) |

## 2. Task-state readers outside the kernel

### Encodings (one kernel encoder for all three ABIs)
`kernel/src/task/syscall/misc.rs:487 encode_task_state`: `Ready→0`, `Running→1`, `Terminated|Retiring→3`, **every other variant→2** (Sleeping, Sending, Recv, PipeRead/Write, FutexWait, Waiting, Polling, WaitEvent, WaitCompletion, WaitIpc, Frozen, WaitIrq).

| record | syscall (opcode) | field | encoder site | documented encoding |
|---|---|---|---|---|
| `api::syscall::ProcessInfo` (libs/api/src/abi/syscall.rs:437) | GetProcs (30) | `state: usize` | spawn.rs:201-204 `snapshot_process_info` | comment "0=Ready,1=Running,2=Waiting,3=Terminated" |
| `api::syscall::ProcessInfoV2` (:445) | GetProcs2 (239) | `state: u32` | spawn.rs:418-421 | same comment |
| `api::telemetry::TelemetryTaskV1` (telemetry.rs) | TelemetrySnapshot (430) | `state: u32` | kernel/src/task/telemetry.rs:364-379 | `task_state::{READY=0,RUNNING=1,WAITING=2,RETIRING=3}` (telemetry.rs:98-103); validator telemetry.rs:383 rejects >3 |

`TaskState` itself is not exported: no `TaskState` type in libs/api/libs/ostd/cells; only comments (supervisor hotswap.rs:91, cellbox async_utils.rs:15/37) and text-engine's own presentational enum (libs/text-engine/src/top/sample.rs:70).

### Readers
| file:line | fn | record | interpretation |
|---|---|---|---|
| cells/tools/sys-tools/src/bin/ps.rs:23 | `cell_main` | ProcessInfo | 0 Ready,1 Running,2 Waiting,3 Dead |
| cells/tools/sys-tools/src/bin/kill.rs:44-49 | `cell_main` | ProcessInfo | **2 ⇒ `sys_send(tid,[0xFF])` (kill.rs:49) blocking "shutdown sentinel"**; 0/1 ⇒ ForceExit; 3 ⇒ already dead |
| cells/services/sshd/src/shell.rs:59 | `execute` "ps" | ProcessInfo | **wrong map**: 1 Ready, 2 Waiting, 3 Exited, 4 Running, 0 ⇒ "Unknown" |
| cells/apps/hypha/tools/sys/src/main.rs:82 | `handle` list_cells | ProcessInfo | 0 ready,1 running,2 waiting,_ dead |
| cells/services/supervisor/src/power.rs:28,57 | power settle | ProcessInfo | `STATE_TERMINATED = 3` skip |
| cells/tools/init/src/activation.rs:512,521 | `task_alive` | ProcessInfo | alive ⇔ state≠3 |
| cells/tools/init/src/boot.rs:61 | `task_terminated` | ProcessInfo | dead ⇔ absent or 3 |
| cells/tools/init/src/boot.rs:185-188 | `platform_wait_step` | ProcessInfo | >3 ⇒ Ambiguous; 3 ⇒ done |
| cells/tests/tier2-grant-owner/src/main.rs:183 | `is_receiver` | ProcessInfo | ≠3 |
| cells/tests/tier2-grant-receiver/src/main.rs:155 | poll loop | ProcessInfo | ≠3 |
| cells/tests/bench/src/scenarios/parked_footprint.rs:58 | `all_live` | ProcessInfo | ≠3 |
| cells/tests/s2tw-probe/src/main.rs:139-191 | `get_procs/get_procs2/telemetry_snapshot` | all three | counts/authority only, no state decode |
| cells/services/compositor/src/input_handler.rs:349 | `find_shell_tid` | ProcessInfo | name only (finds "cellbox" tid by name) |
| cells/tools/init/src/main.rs:179 | `own_tid` | ProcessInfo | name only |
| libs/text-engine/src/top/sample.rs:639-643 | `task_sample` | TelemetryTaskV1 | READY/RUNNING/WAITING, `_ ⇒ Retiring`; consumed by cellbox `top` (cells/tools/cellbox/src/top.rs:83-85) |
| libs/api/src/abi/telemetry.rs:383 | `TelemetryTaskV1::valid` | TelemetryTaskV1 | state ≤ RETIRING |

## 3. Kernel

### 3a. `fast_ipc::` references (kernel/src)
`is_registered_vfs_cell(c)` ⇔ `VFS_HANDLER_CELL == c` (fast_ipc.rs). The owner is set by the `block_io` launch route, not by any fast path.

| file:line | call | what it gates |
|---|---|---|
| kernel/src/main.rs:56 | `pub mod fast_ipc` | module |
| kernel/src/loader.rs:111 | `set_vfs_handler_cell` | `commit_launch_routes`: any root with `routes.block_io` becomes "the VFS cell" (last writer wins, only a warn) |
| kernel/src/task.rs:1576 | `clear_vfs_if_cell` | root retirement clears the owner |
| kernel/src/sched/scheduler.rs:408 | `is_registered_vfs_cell` | `may_watch_owner_deaths` → owner-death subscriptions (WatchCellOwner 245 / WatchCellOwnerRecord 248) |
| kernel/src/task/async_ipc.rs:782 | `is_registered_vfs_cell` | on async reply, do NOT clear VFS's caller context (keeps outer grant authority) |
| kernel/src/task/tcb.rs:917 | `is_registered_vfs_cell` | `set_received_request_context`: VFS keeps outer caller across nested IPC receives |
| kernel/src/task/tcb.rs:932 | `is_registered_vfs_cell` | `begin_receive_context`: wildcard Recv by VFS drops request context/lease (called from ipc.rs:709/943, mod.rs:1179, scheduler.rs:2560) |
| kernel/src/task/syscall/authority.rs:711 | `is_registered_vfs_cell` | ResolveCellOwner (244): VFS principal may resolve its current caller |
| kernel/src/task/syscall/authority.rs:768 | `is_registered_vfs_cell` | ResolveCellOwnerRecord (247): VFS-only |
| kernel/src/task/syscall/authority.rs:804 | fn ptr → `cell_image::resolve_for_caller` (cell-image.rs:45) | ResolveCellImage (441): VFS gets unfiltered image identity |
| kernel/src/task/syscall/grant.rs:945 | `is_registered_vfs_cell` | `current_vfs_grant_lookup` → GrantSlice (210) lease (grant.rs:2199) and lease release on `Send` (ipc.rs:192) / `IpcReply` (ipc.rs:165) |
| kernel/src/task/syscall/grant.rs:1481 | `is_registered_vfs_cell` | `install_vfs_lease_if_context_live` (grant.rs:1594) |
| tests only | 21 in task/vfs_lifecycle_selftest.rs, 18 in loader/atomic_publication_tests/baseline.rs, 2 in …/snapshot.rs, 2 in task/syscall/tests.rs:2183/2202 | fixtures |

Dead inside fast_ipc.rs: `register_vfs` (:57) and `call_vfs` (:179) are `#[no_mangle]` with no caller outside fast_ipc.rs tests and atomic_publication_tests (file header: "retained only as design scaffolding"). SDK twin `libs/ostd/src/fast_ipc.rs` `register_vfs` is called by cells/services/vfs/src/main.rs:158 into a private per-ELF static; `ostd::fast_ipc::call_vfs` (:155) has 0 callers.

### 3b. Dead / legacy `Syscall::` arms (opcodes from libs/api/abi/syscalls.toml)
| variant | opcode | dispatch | behaviour |
|---|---|---|---|
| `Lend` | none (`[[internal]]`, toml:2138) | mod.rs:1335 → grant.rs:1732 → task.rs:2210 | never constructed (no decode arm); only golden_tests.rs:181 |
| `BorrowRead` | none (toml:2141) | mod.rs:1342 → grant.rs:1744 → task.rs:2933 | same — unreachable |
| `BorrowWrite` | none (toml:2144) | mod.rs:1348 → grant.rs:1756 → task.rs:2966 | same |
| `Grant` | none (toml:2147) | mod.rs:1284 → grant.rs:1715 → task.rs:2996 | same (also priority row mod.rs:1152) |
| `Map` | none (toml:2150) | mod.rs:1290 → grant.rs:1727 → task.rs:3012 | same (mod.rs:1153) |
| `AudioPlay` | 218 | mod.rs:1560 → misc.rs:557 | always `Err(Unknown)` + warn "G2 TODO" |
| `NetTx` | 310 | mod.rs:1578 → misc.rs:566 | network check, audit, reads frame, discards, `Ok(0)` |
| `NetRx` | 311 | mod.rs:1582 → misc.rs:581 | network check, validate buf, `Ok(0)` (never data) |
| `Exec` | 6 | mod.rs:1398 → spawn.rs:700 | reads path, always `Err(NotSupported)` |
| `MkDir` | raw 110 (toml:2091) | mod.rs:1397 → fs.rs:596 | reads path, always `Err(PermissionDenied)` |
| `Create` | raw 111 (toml:2098) | mod.rs:1533 → fs.rs:897 | reads path, always `Err(PermissionDenied)` |
| `FileOp` op=1 | 107 | mod.rs:1556 → fs.rs:914-916 | "Rename - Stub" → `Err(NotSupported)` (op 0 = remove works) |
| `ServiceLookup` | **2 (`Call`)** and raw 100 | decode.rs:966 maps `ViSyscall::Call` → `ServiceLookup`; mod.rs:1357 | legacy name-lookup alias; opcode 2 "Call" is not a call |
| `Reply` | 3 | mod.rs:1333 → ipc.rs:1169 → task.rs:2913 `ipc_reply` | legacy: forces `caller.current` to `Ready` + sets `reply_value` without checking its state; no libs/cells caller, still in ostd base allowlist (runtime.rs:52) |
| `Yield` | 104 | mod.rs:1356 | "Legacy" section, functional |
| arch-gated (not dead) | AllocPcieIrq (non-x86, dev.rs:189), SystemPower 436 (non-x86, dev.rs:226), ProbeVirtioMmio (x86, dev.rs:1214) | | fixed `NotSupported` on the listed arch only |

### 3c. `Task` fields (kernel/src/task/tcb.rs:325, struct is `#[allow(dead_code)]`)
| field (tcb.rs line) | status |
|---|---|
| `leases` (385), `next_lease_id` (387) | only via `add_lease/get_lease/revoke_lease` (tcb.rs:817-843) ← `ipc_lend/borrow_*` ← unreachable internal syscalls ⇒ dead |
| `grant_table` (391), `next_grant_id` (392) | only via `add_grant/get_grant/remove_grant` (tcb.rs:846-872) ← `ipc_grant/ipc_map` ← unreachable ⇒ dead |
| `pending_future` (449) | never set to `Some` (init None tcb.rs:709; cleared scheduler.rs:2703); read at scheduler.rs:2679 only under `TaskState::Polling`, which nothing ever enters ⇒ dead (with `SyscallFuture`, tcb.rs:246) |
| `cluster_mode`, `cluster_id` | written launch.rs:190-191/560-561; read only in test fixture loader/atomic_publication_tests/snapshot.rs:66 ⇒ never read in production |
| `allowed_drivers` (383) | set at construction, copied parent→thread (scheduler.rs:1234-1236), p01 heap accounting; never consulted for a decision |
| `pku_key` | production read only to copy to threads (scheduler.rs:1242); enforcement uses `pku_value`; else test reads |
| `reply_value` (403) | live but legacy: written by `Sending` park (task.rs:2355), `ipc_reply` (task.rs:2920), exit wakes (scheduler.rs:1898/2112); read ipc.rs:212/1088 |
All other fields have at least one production read.

### 3d. Kernel `Rename` path — `kernel/src/task/syscall/fs.rs:520-560` (Rename = opcode 255; dispatch mod.rs:1377)
1. Copies both paths, resolves VFS with `crate::cell::service_registry::lookup(service::VFS)` — **bare tid** (fs.rs:529).
2. `ipc_send_kernel(caller, vfs_tid, encoded)` (task.rs:2267): `Ok(0)` delivered/queued; `Ok(1)` ⇒ caller parked in `TaskState::Sending{target, delivery_id}` (task.rs:2356) **no deadline**, then `yield_cpu()`; `Backpressure ⇒ TryAgain`; `TargetGone ⇒ InvalidCommand`.
3. `loop { ipc_recv_kernel(caller, mask=vfs_tid) }` (fs.rs:547-558): on no message the caller is set to `TaskState::Recv{mask: vfs_tid, deadline: None}` (task.rs:2546-2551) and the handler `yield_cpu()`s and re-polls. `ipc_recv_kernel` only errors if the caller task/scheduler vanishes or the reply is oversized; it never checks VFS liveness.
4. **No expiry anywhere**: no tick budget, no deadline, no VFS-death exit — if VFS dies or never answers, the syscall loops forever. Any reply ≠ `VfsResponse::Ok` (including a decode failure) maps to `PermissionDenied`.
5. It is the only production user of `ipc_send_kernel`/`ipc_recv_kernel` (others: ipc_wire_tests.rs:130, ipc_pending_selftest.rs:153; `ipc_send` wraps it at task.rs:2258).

## Totals
- 1a blocking sends in libs: **REPLY 1 · EVENT 6 · REQUEST 17 · UNCLEAR 1** (25 sites).
- 1b SDK wrappers listed: 24 rows; tid-only lookup call sites ≈150 (`sys_lookup_service` 130 cells + 19 libs; `service::lookup` 10; `wait_for_compositor` 16).
- 2: 3 state-bearing records (GetProcs 30, GetProcs2 239, TelemetrySnapshot 430), 1 shared encoder, 16 reader sites (11 decode state).
- 3a: 12 production `fast_ipc::` sites (9 `is_registered_vfs_cell` gates), 43 test sites.
- 3b: 5 unreachable internal arms, 7 fixed-result arms (AudioPlay, NetTx, NetRx, Exec, MkDir, Create, FileOp/1), 3 legacy aliases (Call→ServiceLookup, Reply, Yield).
- 3c: 7 dead/never-read fields (leases, next_lease_id, grant_table, next_grant_id, pending_future, cluster_mode, cluster_id) + 2 copy-only (allowed_drivers, pku_key).

## Surprises
1. `ostd::dispatch::run_service` — the SDK's only generic serve-loop REPLY — has zero callers.
2. `cluster_endpoint::LocalEndpoint::call` has zero callers; `LocalEndpoint::new` (tid-only) only in tests; `RemoteEndpoint::call` is a hard `NotSupported`.
3. `sshd` "ps" decodes ProcessInfo with a wrong map (1=Ready, 4=Running, 0=Unknown) — shell.rs:59.
4. `kill` sends a **blocking** `sys_send(tid,[0xFF])` whenever state==2; state 2 lumps Sending/Sleeping/Frozen/WaitIrq/Futex/etc., so `kill` can park forever on a task that never receives (kill.rs:49).
5. `ProcessInfo` docs say 3=Terminated, telemetry calls 3 RETIRING; the encoder maps both Terminated and Retiring to 3 and 13 distinct blocked states to 2 — `Sending` is invisible to every reader.
6. Opcode 2 is named `Call` but decodes to legacy `ServiceLookup`; opcode 3 `Reply` forces the served caller `Ready` regardless of state and has no user, yet stays in the ostd base allowlist.
7. Kernel `Rename` (255) is itself a blocking legacy client of VFS: bare-tid lookup, `Sending` with no deadline, then an unbounded yield-poll `Recv` loop with no VFS-death exit.
8. `NetRx` returns `Ok(0)` and `NetTx` silently discards frames instead of failing.
9. "VFS cell" identity in the kernel is whatever root last launched with `routes.block_io` (loader.rs:111) and lives in `fast_ipc`, a module whose actual fast path (`register_vfs`/`call_vfs`) is dead; the VFS cell still calls the SDK `register_vfs` into a private static.
10. `ViSurface::drop` performs two blocking compositor round-trips; `create_surface` and `tls_*` ignore the `sys_send` result before an unbounded receive.
11. vimio and vipolling duplicate the same TcpReady REQUEST and accept the reply with a wildcard receive mask.
