# Phase 12 census: service main loops and the `serve` hook fit (at `f094e9df5`)

**Answer:** All 10 loops are hand-rolled. None uses `service_entry!`. Only the supervisor runs on `AppContext`, through `run_app!`. Six of the ten can move onto one `serve` loop. That loop needs a park/wait return value from `on_idle`, a choice of owner-watch policy, and one kernel addition: a receive that is both timed and attested. Without that addition, built-in heartbeat and built-in attestation cannot both hold in a loop that blocks. Net should migrate last, as the plan says. Input, net-broker and silo should stay hand-rolled, or move only partly; reasons are in §4.

## 1. Per-service anatomy

| Svc | Entry | Receive (call / mask / buffer) | Attested caller | Reply path | Owner death | Heartbeat | Idle / periodic work | Nested IPC while serving |
|---|---|---|---|---|---|---|---|---|
| config | `cell_main!` config/main.rs:105-108 | `sys_recv_attested(0, 4096)` :120; error → `yield_now` :131 | `from_recv_buf` :122 → `rights::check` rights.rs:43 | `Incoming::current(sender).reply` :126 | none | none (Heartbeat is in the allowlist :24 but never called) | none. `ViStateTransfer` is implemented :145 but the loop never routes snapshot/restore messages | none |
| kms | `cell_main!` kms/main.rs:23-25 | `sys_recv_attested(0, 4096)` :43; handles only `[..KMS_MESSAGE_LEN=128]` :53 | `from_recv_buf` :46; per-request `ServiceRegistrySnapshot` costs 3× `provider_tid` :47-51 | `ipc::current()` :45 → `ipc::reply(op)`, else `ipc::post` :55-59 | none | none (allowlist :13-21 has no Heartbeat) | none | Silo: `post` + masked `sys_recv_timeout(silo_tid, 8)` storage/provider/silo.rs:121-125; `VfsClient` storage/runtime.rs:9-15, ssh/persist/blob_io.rs:126-132 |
| ai | `cell_main!` ai/main.rs:114-116 | **unattested** `sys_recv(0, 4096)` :124 | **none**; the sender tid is the session owner :251, :316, :378 | `ipc::reply_to(sender)` :128 | none. Sessions idle-reaped instead (`SESSION_IDLE_TICKS=3000` :97, `reap_idle_sessions` :217-243), but only called when a request arrives :126 | none (allowlist :39-54 has no Heartbeat or RecvTimeout) | the reap above, tied to message arrival | VFS only at load :491-497 |
| supervisor | `run_app!(handler)` main.rs:199 → `AppContext::run_with_lifecycle` lib.rs:182-192 | `AppContext::wait_once` → `sys_recv_attested(0, 4096)` app.rs:233-235 (attested only when untimed, app.rs:229); copies the payload into a `Vec` app.rs:413-460 | `ctx.attest_callers()` main.rs:62; `ctx.caller()` + `sys_resolve_cell_image` :195-197; kernel sender 0 = power button :50, :154 | `Incoming::current(..).reply` :72-114, :178; relays `0xE1` from tid 1 as `post 0xE2` to compositor :127-138 | none | none (`run_app!` arms none) | none | hotswap yield-poll loops of `WAIT_TICKS=500` hotswap.rs:28, :59, :76, :199; power `settle()` uses `sys_recv_timeout(0, 64B, 5)` for 50 ticks and **drops every other message** power.rs:31-32, :132-145 |
| compositor | own `#[no_mangle] main` main.rs:88-91; startup spin-yield `connect_to_input` input_handler.rs:142-160 | `buf.fill(0)` + `sys_recv_attested(0, 4096)` main.rs:135-137; request window is the first 512 bytes | `owner_watch::classify`: trailer must match the sender, else it is a death or dropped, owner_watch.rs:194-204 | `reply_to(sender)` in message_handler/{grant,surface,view,window}.rs (e.g. window.rs:130-154); events via `sys_try_send` (window.rs:181, pointer_router/target.rs:84, stacking/reconcile.rs:103, appearance.rs:47) | **fail-open** watch on `CREATE_SURFACE`/`CREATE_VIEW` main.rs:183-205, `admit` owner_watch.rs:113; `take_death` :174; `reclaim` :209; supervisor `0xE2` path message_handler.rs:18, :69-77 | none | render after every turn main.rs:228-249; resolution check every 5 s main.rs:254, which runs only after a message (the receive is untimed) | input relay: events from `input_tid` op `0x10` main.rs:157-176; `forward_key` uses `sys_try_send` input_handler.rs:366-392 |
| vfs | `cell_main!` vfs/main.rs:135-137 | switches between `sys_try_recv_attested` and `sys_recv_attested` on `idle_work` :172-176; buffer re-zeroed :195, :213, :240 | `from_recv_buf` :180 → `Caller::from_attested` :198 | `ipc::current()` :179 → `ipc::reply`, else `post` :234-239, with `GLOBAL_VFS` unlocked first | message without a trailer → `handle_unattributed_owner_death(sender)` :181-195, manager/owned_state.rs:40-55; watch on **every** request, **fail-closed** (`Err(3)`) :200-213; `install_owner_watch` :223, owned_state.rs:8-35; cancel tokens :229-231 | none (Heartbeat in allowlist :83) | while `idle_work`: try_recv + `yield_now` spin; reconcile every 128 idle turns :159, :242-253 | blk_router masked **untimed** `sys_recv(tid)` blk_router.rs:80-83, router.rs:159-176; trust → input `session::service_call` trust/first.rs:35-40, ostd session.rs:547-613; dispatch runs under the `GLOBAL_VFS` lock :219-224 |
| net | own `#[no_mangle] main` net/main.rs:82-85 → `service_runtime::run` :703 | `buffer.fill(0)` + **only** `sys_try_recv_attested(0, 4096)` :926-927; parks in `sys_wait_completion(NET_RX, ≤10)` :1028-1070, after burst-grace yields :1024 | `from_recv_buf` :934; cell/gen 0 dropped silently :954; `rights::admit(flags)` :961, :462-470 | `reply_to` (`send_ready` :249, denial :467). Deferred: `PendingDns.op_id` → `ipc::reply`, else `sys_try_send` retried :155-237; `PendingReady.op_id` :258-327, :416; `ReplyTo{Async,Legacy}` interface.rs:237-240, :720-728; tls/dispatch.rs:64-75, :180-224 (legacy retry until `REPLY_TIMEOUT` :31) | `watched` map :758; **fail-open** watch on first sight :972-997; no trailer → `owner_for_root` + `release_owner` :936-951, :587-620; `reclaim_owner_state` :542 | `sys_heartbeat(500)` every turn :791 | per turn :792-924: MAC rebind, `pump_rx`, DHCP, link-local, `iface.poll`, `tls_pending.poll` :911, `poll_resolves` :918, graceful/abort reapers :919, `drain_ready_waits` :924 | NIC driver: `offer` (try_send, ≤20 ticks) + masked `sys_recv_timeout(tid, 20)` + drain interface.rs:28-34, :450-506 |
| input | own `#[no_mangle] main` input/main.rs:110 | trailer cleared, then `sys_try_recv_attested(0, 4096)` :156-157; sender `isize::MAX` = kernel UART relay :161; sender 0 = kernel push; registered sources send raw frames :467-487 | `from_recv_buf` read only for SessionV1 :639 and `focus_requester` :695-712 | `ipc::current()` → `ipc::reply`, else `sys_try_send` :661-665; deferred `PendingPoll.token` session.rs:181-190, answered in `maintain` session.rs:3003, :3110-3140 via `send` :852-868 | none | `sys_heartbeat(5000)` before the device probe :125 and every turn :142; the comment's `DISPATCH_HEARTBEAT` (:96) does not exist | `sessions.maintain`, `flush_mouse`, `drain_virtio`, key repeat, LEDs :143-247; idle = `SetTimer(1)` :425-429; buffer zeroized after each message :172, :188, :211 | `sys_try_send` to compositor dispatcher.rs:239-247; `sys_lookup_service_bound(COMPOSITOR)` per focus request :695-700 |
| silo | `cell_main!` silo/main.rs:50-52 → `ipc::run` :156 after `register(SILO)` :151 | `sys_recv_attested(0, fresh 4096)` ipc.rs:16-17; error → `continue` with **no yield** :19 | `from_recv_buf` → `PeerIdentity`, checked against `provider_tid(KMS)` ipc.rs:22-33; frame is `[..192]` | `ipc::current()` → `ipc::reply`, else `sys_try_send` ipc.rs:21, :35-39 | none | none (allowlist main.rs:21-35 has no Heartbeat or TryRecv) | none | guest vCPU runs inside `state.process` [not traced] |
| net-broker | `cell_main!` main.rs:115-117; `loop receive_once` :176-178 | main thread `sys_recv_attested(0, 4096)` local_runtime.rs:195-197; **this thread never heartbeats** | `from_recv_buf` :200 → `handle_ingress(sender, identity, parsed, op)` | Immediate → `send_or_queue` :214, :311; queued `QueuedReply.operation` → `ipc::reply`, else `sys_try_send` with Busy retained :295-307; reply-pump thread :256 | none | per-thread: worker/pump 1000 ticks (`WORKER_HEARTBEAT_TICKS` local_queue/state/types.rs:12, local_runtime.rs:234, :260); network 1000 :271, re-armed to 500 inside beacon beacon.rs:48, :341 | network thread polls beacon :266-292; roles spawned with `sys_spawn` :344 | beacon `call_bounded`: `sys_post` + `sys_recv_timeout(service_tid)` slices beacon.rs:222-300; VFS boot epoch :53-76 |

## 2. SDK and init facts

- **`AppContext`** (app.rs:152-168): one 4096-byte buffer; lazy vfs/net/input clients; `attest_callers` :191; `wait_once` :219-252.
  - With a heartbeat armed it waits at most `h/2` and re-arms each turn, **but then the receive is timed and therefore unattested**: `caller()` returns `None` (doc :188-190, code :229).
  - `reply` routes through the `Incoming` captured at receive :319-325.
  - Every event's payload is copied into a `Vec` :413-460, which is an avoidable allocation per message.
  - Message routing depends on the `0xAC` envelope. The kernel's power request (`acpi/sci.rs`) and the supervisor's `OP_*` messages rely on it.
- **`CellRuntime`** runtime.rs:142-210: default heartbeat 500 ticks; `run` arms it and then runs `run_with_lifecycle`.
- **`app_entry!`** :241; users are demos only (hello-cell, cfi-test, hotswap-demo, http-smoke).
- **`service_entry!`** :412-461: still defined, **no users**. It adds `WaitForEvent` to the profile through `service_syscall_set` :113-118.
- **`run_app!`** lib.rs:182-195: users are supervisor and platform. It emits no allowlist.
- **Receive syscalls:**
  - Attestation exists only on `Recv` and `TryRecv` (`RECV_ATTEST_CALLER` in a3; decode.rs:870-881).
  - `RecvTimeout` uses a3 for the deadline (:893-897), so **no timed attested receive exists** (syscall.rs:1543, :1585, :1608).
  - `SetTimer` parks as `WaitKind::Sleep` (kernel ipc.rs:1197-1209). Whether an incoming message wakes it is [not traced].
- **Owner-death delivery:**
  - The kernel queues `pending_owner_deaths` (scheduler.rs:1984).
  - They are delivered only to wildcard-mask receives, including TryRecv (kernel ipc.rs:45-52, :70-80), as a message whose sender is the dead root tid and which carries no trailer.
  - `sys_watch_cell_owner` is granted only during the receive turn of the principal being watched (owner_watch.rs:33-37). It has no allowlist bit (syscalls.toml:1903).
- **Init / watchdog:** init has **no** heartbeat expectations.
  - It supervises through `NotifyOnExit` (service_table.rs:118, activation.rs:105) plus `task_alive` and provider-binding checks (service_table.rs:58-72). The supervision loop is supervisor.rs:100-200.
  - A heartbeat miss is handled by the kernel sweep (scheduler.rs:2576-2600, armed per task by ipc.rs:1179-1189). The kill then reaches init as an ordinary death, which follows the restart policy (VFS: `Always`, service_table.rs:50).
  - No default deadline exists (`heartbeat_deadline: None`, tcb.rs:648). Heartbeat stays opt-in per spec 12 :86-87.

## 3. Hook-model misfits

1. **Heartbeat vs attestation.** No timed attested receive exists. Today's heartbeating loops get both by polling `TryRecv` and parking some other way: net uses `WaitCompletion(NET_RX)` service-runtime.rs:1070; input uses `SetTimer(1)` main.rs:425. VFS forbids `SetTimer` (main.rs:115-120). Built-in heartbeat therefore needs either an additive attested `RecvTimeout`, which is a Law-1 ABI item, or try_recv+park with a latency floor.
2. **Net's pump loop.** Per-turn work runs *before* each receive, never blocks on IPC, and uses burst-grace yields (:1024) and a NET_RX completion wait sized by `iface.poll_at` (:1028-1037). This needs `on_idle` to return a wait spec (sources + ticks), plus a per-turn hook that runs whether or not a message arrived.
3. **Bounded driver waits inside a request.** Net's inline `command` (interface.rs:487-506) and kms's silo exchange (silo.rs:121-125) use masked timed receives. VFS uses a masked **untimed** `sys_recv(tid)` (blk_router.rs:82). A masked receive replaces the kernel's current-op record, so `serve` must capture `Incoming` before dispatch, as VFS does at :179. These masked receives also never deliver owner deaths, because deaths go only to wildcard masks.
4. **Deferred replies.** Net (`PendingDns`, `PendingReady`, `ReplyTo{Async,Legacy}`, the TLS legacy queue), input (`PendingPoll.token`) and net-broker (`QueuedReply.operation`) all keep tokens across turns. Legacy senders need `try_send` retried until a deadline. The request hook must be able to return "deferred" with a storable token.
5. **Owner-watch policy differs.** VFS watches every request and fails closed (`Err(3)`). Compositor watches only creating opcodes and fails open. Net watches on first sight and fails open. All three reclaim stale generations and map a death by root tid. Compositor also keeps per-cell tid sets (owner_watch.rs:9-12).
6. **Senders without a trailer that are not deaths.** Input: kernel push from sender 0, UART relay from `isize::MAX`, registered raw producers (main.rs:161, :475-487). Supervisor: power button from sender 0 and relay from tid 1. Compositor: `0xE2` from the supervisor, validated by tid only (message_handler.rs:69). `serve` must expose a raw/unattributed hook rather than deny these.
7. **Compositor input relay and render.** Input events arrive in the same attested queue (main.rs:157-176). Render and the 5 s resolution check run after **every** turn (:228-266), so they need a post-turn hook, not `on_idle`.
8. **Long blocking handlers.** Supervisor hotswap loops up to 3×500 ticks and the power settle (50 ticks) does a wildcard `RecvTimeout` that drops other messages (power.rs:132-145). With a heartbeat these need a `keepalive()` from the context. The settle drop also breaks owner-death and request ordering.
9. **Multi-threaded net-broker.** Heartbeat is per task. The ingress thread blocks unheartbeated while three spawned roles heartbeat themselves (local_runtime.rs:195-292).
10. **Input secret hygiene.** The buffer is zeroized after every message and device batch (main.rs:172-211), so `serve` would need a scrub-after-dispatch option.
11. **Per-turn lookups.** kms does 3× `provider_tid` per request (main.rs:47-51) and input does `lookup_service_bound` per focus request. These are fine as handler code; noted for cost only.
12. **VFS global lock.** Dispatch runs under `GLOBAL_VFS` and the lock is released before replying (:219-239). The handler must own this, so `serve` replies only after `on_request` returns.

## 4. Proposed `serve` surface

```rust
// libs/ostd/src/service/serve.rs  (service.rs becomes service/mod.rs)
pub struct ServeConfig {
    heartbeat_ticks: u64,      // 0 = none; >0 requires an attested timed receive (§3.1)
    watch: WatchPolicy,        // None | Lazy{opcode: fn(u8)->bool} (fail-open) | Every (fail-closed → on_watch_refused)
    scrub: bool,               // zero the buffer after dispatch (input)
}
pub enum Wait { Block, Ticks(u64), Poll, Completion { sources: u32, ticks: u64 } }
pub enum Reply<'a> { Now(&'a [u8]), Deferred, None }   // serve sends Now via the Incoming captured at receive
pub struct Request<'a> { pub caller: CallerIdentity, pub incoming: Incoming, pub payload: &'a [u8] } // no Vec copy
pub trait Handler {
    fn on_request(&mut self, req: Request<'_>, cx: &mut Cx) -> Reply<'_>;
    fn on_owner_death(&mut self, dead: Released, cx: &mut Cx) {}   // serve owns the owner table, stale-generation sweep and token cancel
    fn on_unattributed(&mut self, sender: usize, payload: &[u8], cx: &mut Cx) {} // kernel 0, isize::MAX, raw producers, 0xE2
    fn on_turn(&mut self, cx: &mut Cx) {}           // after every receive attempt (compositor render, net pump)
    fn on_idle(&mut self, cx: &mut Cx) -> Wait { Wait::Block }
}
// Cx: keepalive() (re-arm heartbeat inside long handlers), DeferredReply::{reply, legacy(try_send+deadline)}, watch_refused()
```

**Coverage:**
- **Fits directly:** config, kms, ai, silo. ai becomes attested; it needs Heartbeat/RecvTimeout added to its allowlist (:39-54); its idle reap moves to `on_idle -> Ticks(..)`.
- **Fits with these features:**
  - supervisor: `on_unattributed`, `keepalive()`, and the `0xAC` envelope decode moved into its handler. The settle-time message drop must become an explicit decision.
  - compositor: `Lazy` watch, `on_turn` for render and resolution.
  - vfs: `Every` fail-closed watch, `on_idle -> Poll` while `idle_work`. Its block receive stays nested in the handler, but should become timed (plan step 4).
- **Migrate last, conditional:** net. It needs `on_turn` + `Wait::Completion{NET_RX}` + `Poll`, deferred replies and `Lazy` fail-open watch. It needs the bench (planned for step 7) before and after.
- **Stay hand-rolled:**
  - **input.** Raw producers, UART and kernel push, secret scrubbing, a `SetTimer` tick cadence and the session pump are its core. It is also outside the plan's Modify list.
  - **net-broker.** Multi-threaded roles with per-task heartbeats. Only the ingress thread could adopt `serve`, and that would add a heartbeat that thread lacks today. Out of plan scope.
- **Blocking prerequisite for CP1:** built-in heartbeat in an attested blocking loop needs a kernel receive that is both timed and attested (§3.1). The fallback is try_recv + park, with the latency cost that net and input already pay.

## Not found

- No `ostd::dispatch` (deleted, per the handoff).
- No `pending_owner_deaths` in userspace; it is kernel-only.
- No heartbeat expectation anywhere in `cells/tools/init/src`.
- No `declare_syscalls!` in the compositor or supervisor crates.
- No owner-death handling in config, kms, ai, supervisor, input, silo or net-broker.