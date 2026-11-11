# P11 census — area: services (`cells/services/**/*.rs`, no `target/`)

Baseline: branch `main`, HEAD `0f81a6597`. Read-only scan. Searched: `sys_send\(` (catches the bare `sys_send`, `ostd::syscall::sys_send`, and `sys::sys_send` forms, including multi-line calls), every `use … sys_send` import or alias, `lookup_service`, `sys_lookup_service_bound`, `LookupServiceBound`, and `ServiceBinding`.

Kernel fact that matters for REPLY rows (`kernel/src/task/async_ipc.rs:680-686`, `reply_current`): if a provider calls `sys_send` to the caller it is currently serving and that caller used `IpcSubmit`, the kernel turns the send into the reply for that operation. It does this only for the first send. The comment says services that send events to the same destination "must capture the token and use explicit replies instead of this bridge."

## A. Blocking `sys_send` sites (54)

| file:line | enclosing fn | class | peer (service / tid source) | note |
|---|---|---|---|---|
| ai/src/main.rs:127 | cell_main | REPLY | requester; `sender` from `sys_recv(0)` | serve loop; always uses the legacy send, no `ipc::current()` check (httpd reaches AI through `ipc::submit`, so its replies go through the kernel bridge) |
| compositor/src/message_handler/grant.rs:61 | attach_grant | REPLY | sender from `sys_recv_attested` (main.rs:137) | ATTACH_GRANT `\x01`/`\x00` |
| compositor/src/message_handler/grant.rs:101 | detach_replaced_grant | REPLY | sender | decode failure `\x00` |
| compositor/src/message_handler/grant.rs:105 | detach_replaced_grant | REPLY | sender | bad reg_id `\x00` |
| compositor/src/message_handler/grant.rs:112 | detach_replaced_grant | REPLY | sender | ack `\x01`/`\x00` |
| compositor/src/message_handler/grant.rs:141 | detach_grant | REPLY | sender | owner match |
| compositor/src/message_handler/grant.rs:150 | detach_grant | REPLY | sender | not owner `\x00` |
| compositor/src/message_handler/grant.rs:159 | screen_size | REPLY | sender | GET_SCREEN_SIZE |
| compositor/src/message_handler/surface.rs:39 | handle (CREATE_SURFACE) | REPLY | sender | cap |
| compositor/src/message_handler/surface.rs:42 | handle (CREATE_SURFACE) | REPLY | sender | 0 cap on error |
| compositor/src/message_handler/surface.rs:194 | destroy_surface | REPLY | sender | not owner `\x01` |
| compositor/src/message_handler/surface.rs:206 | destroy_surface | REPLY | sender | ok `\x00` |
| compositor/src/message_handler/view.rs:232 | send_response (wrapper) | REPLY | `recipient` = sender | 7 callers: view.rs:96,151,190; view_frame.rs:69,133,195,241 |
| compositor/src/message_handler/view.rs:239 | send_error (wrapper) | REPLY | `recipient` = sender | 16 callers: view.rs:26,76,100,106,154,160,193; view_frame.rs:20,72,87,136,151,162,198,213,244 |
| compositor/src/message_handler/window.rs:129 | acknowledge_configure | REPLY | sender | short frame `\x00` |
| compositor/src/message_handler/window.rs:133 | acknowledge_configure | REPLY | sender | decode error |
| compositor/src/message_handler/window.rs:137 | acknowledge_configure | REPLY | sender | unknown cap |
| compositor/src/message_handler/window.rs:141 | acknowledge_configure | REPLY | sender | not owner |
| compositor/src/message_handler/window.rs:146 | acknowledge_configure | REPLY | sender | serial mismatch |
| compositor/src/message_handler/window.rs:151 | acknowledge_configure | EVENT | sender (the requester) | surface-state event sent to the requester **before** the reply ("client consumes this asynchronous state while waiting for the reply") |
| compositor/src/message_handler/window.rs:153 | acknowledge_configure | REPLY | sender | `\x01` sent after the event at :151 |
| config/src/main.rs:124 | cell_main | REPLY | sender from `sys_recv_attested(0)` | no `ipc::current()` check |
| kms/src/main.rs:50 | cell_main | REPLY | sender from `sys_recv_attested(0)` | legacy fallback; `ostd::ipc::reply(operation, …)` when an operation is present |
| kms/src/storage/provider/silo.rs:119 | DevelopmentSiloProvider::call | REQUEST | SILO; tid-only `sys_lookup_service(SILO)` at :118 | followed by `sys_recv_timeout(silo_tid, …, 8)`; the send itself has no deadline |
| net/src/handlers.rs:60 | send_typed (wrapper) | REPLY | sender | 62 callers: handlers.rs (15), handlers/tcp.rs (40), handlers/udp.rs (7). Always the legacy send, even on paths where an operation may exist |
| net/src/handlers.rs:279 | handle_typed (L2Recv) | REPLY | sender | legacy fallback after an `ipc::current()` check |
| net/src/service-runtime.rs:254 | send_ready (wrapper) | REPLY | sender | 8 callers: :170,174,178,278,281,291,301,307 |
| net/src/service-runtime.rs:469 | admit_request | REPLY | sender | rights-denial reply |
| net/src/tls_handler.rs:44 | send_connect_reply (wrapper) | REPLY | sender | 3 callers: :124,134,147 (TLS connect refusal) |
| net/src/tls_handler.rs:83 | handle_tls_raw | REPLY | sender | parse error, empty payload |
| net/src/tls_handler.rs:104 | handle_tls_raw (Close) | REPLY | `ReplyTo::Legacy(sender)` | Async branch uses `ipc::reply` |
| net/src/tls_handler.rs:155 | handle_tls_raw (Send) | REPLY | sender | not owner |
| net/src/tls_handler.rs:169 | handle_tls_raw (Recv) | REPLY | sender | not owner |
| net/src/handlers/tcp.rs:60 | handle_tcp_request (TcpConnect) | REPLY | sender | legacy branch; result checked so an undelivered cap is reclaimed |
| net/src/handlers/udp.rs:156 | handle_udp_request (UdpRecv) | REPLY | sender | data reply |
| ocel-js/src/main.rs:58 | cell_main | REPLY | `caller_tid` from `sys_recv(0)` | no `ipc::current()` check |
| ocel-pdf/src/main.rs:212 | cell_main | REPLY | `owner` from `sys_recv(0)` | legacy fallback when there is no operation |
| ocel-quickjs/src/main.rs:147 | cell_main | REPLY | `caller_tid` from `sys_recv(0)` | only when `operation.is_none()` |
| supervisor/src/hostile_backend_recovery.rs:63 | reply (wrapper) | REPLY | `sender_tid` (AppEvent::RawMessage) | 8 callers in `handle`: :20,24,28,32,36,40,44,51; the requester is the hypervisor via `service_call_bounded` or bench |
| supervisor/src/main.rs:71 | handler (OP_HOTSWAP) | REPLY | `sender_tid` (AppEvent::Message) | rejected caller |
| supervisor/src/main.rs:75 | handler (OP_HOTSWAP) | REPLY | sender_tid | invalid request |
| supervisor/src/main.rs:81 | handler (OP_HOTSWAP) | REPLY | sender_tid | service not found |
| supervisor/src/main.rs:87 | handler (OP_HOTSWAP) | REPLY | sender_tid | success; sent after the whole `hotswap()` |
| supervisor/src/main.rs:91 | handler (OP_HOTSWAP) | REPLY | sender_tid | error |
| supervisor/src/main.rs:102 | handler (OP_SNAPSHOT) | REPLY | sender_tid | rejected caller |
| supervisor/src/main.rs:106 | handler (OP_SNAPSHOT) | REPLY | sender_tid | invalid request |
| supervisor/src/main.rs:109 | handler (OP_SNAPSHOT) | REPLY | sender_tid | `snapshot::run()` result |
| supervisor/src/main.rs:132 | handler (RawMessage 0xE1 from tid 1) | EVENT | COMPOSITOR; tid-only `sys_lookup_service` at :128 | relays the kernel's owner-exited notice as 0xE2; compositor message_handler.rs:68-69 accepts it only when the sender is SUPERVISOR |
| supervisor/src/main.rs:172 | reply_power (wrapper) | REPLY | sender_tid | 3 callers: :153,161,167 (refusals only; success powers off) |
| supervisor/src/transfer.rs:15 | send_snapshot_event | EVENT | old service tid from `sys_lookup_service(service_id)` (hotswap.rs:97) | AppMsg 0xAC/0xF0. No IPC reply; completion is polled through the kernel stash (`wait_for_stash_key`) |
| supervisor/src/transfer.rs:28 | send_restore_event | EVENT | new tid from `sys_spawn_replacement` (hotswap.rs:132) | AppMsg 0xAC/0xF1; readiness polled through `sys_query_hotswap_ready` |
| vfs/src/blk_router.rs:76 | `impl BlockSys for Syscalls`::send | REQUEST | block driver tid from `sys_lookup_block_route` (route record, not the service registry) | called from blk_router/router.rs:166 `exchange`, then masked `sys_recv(tid)` with no timeout (:80) |
| vfs/src/main.rs:216 | cell_main | REPLY | sender from `sys_recv_attested(0)` | owner-watch refusal; legacy fallback |
| vfs/src/main.rs:243 | cell_main | REPLY | sender | normal reply; legacy fallback |

**Wrapper call sites** (call wrappers only, not counted in A): `send_typed` 62, `send_error` 16, `send_response` 7, `send_ready` 8, hostile `reply` 8, `send_connect_reply` 3, `reply_power` 3. All of them are REPLY.

## B. Tid-only lookups (`sys_lookup_service`), 40 call sites

| file:line | enclosing fn | service | note |
|---|---|---|---|
| compositor/src/input_handler.rs:143 | wait_for_service | INPUT, COMPOSITOR (self) | via connect_to_input:155-156; yield loop |
| compositor/src/message_handler.rs:69 | handle_message | SUPERVISOR | sender authentication for OWNER_EXITED by tid |
| httpd/src/main.rs:92 | wait_for_service | NET, VFS | via cell_main:67-68; tid cached for the process lifetime |
| httpd/src/reactor.rs:852 | (reactor Phase::Infer step) | AI | re-looked-up each step; peer mismatch → ServiceNotFound |
| hypervisor/src/backend-fault-control.rs:16 | disconnect | SUPERVISOR | then `service_call_bounded` |
| hypervisor/src/net_backend.rs:45 | NetBackend::active_tid | NET | poison/recovery by tid |
| hypervisor/src/persistent_disk.rs:11 | open | VFS | |
| hypervisor/src/run_loop.rs:40 | run | COMPOSITOR | |
| hypervisor/src/run_loop.rs:54 | run | NET | |
| hypervisor/src/run_loop.rs:194 | run (Wfi) | COMPOSITOR | reconnect every exit |
| hypervisor/src/run_loop.rs:241 | run (Preempted) | COMPOSITOR | reconnect every exit |
| hypervisor/src/run_loop.rs:362 | forward_input_events | INPUT | board-rpi3 only |
| hypervisor/src/run_loop_x86.rs:54 | run | NET | |
| hypervisor/src/run_loop_x86.rs:60 | run | COMPOSITOR | |
| hypervisor/src/run_loop_x86.rs:122 | run | COMPOSITOR | reconnect |
| hypervisor/src/run_loop_x86.rs:158 | run | COMPOSITOR | reconnect |
| hypervisor/src/run_loop_x86.rs:331 | service_app_session_ipc | INPUT | cached once in `host_tids`; used as sender authentication |
| hypervisor/src/run_loop_x86.rs:332 | service_app_session_ipc | COMPOSITOR | same |
| hypervisor/src/virtio_blk.rs:166 | ensure_persistent_connected | VFS | |
| input/src/dispatcher.rs:181 | Dispatcher::dispatch_mouse | COMPOSITOR | event peer (sent with `sys_try_send`) |
| input/src/dispatcher.rs:207 | Dispatcher::flush_mouse | COMPOSITOR | same |
| input/src/main.rs:301 | EventSources::register | USB_HID_PRODUCER | sender authentication by tid |
| input/src/session.rs:61 | compositor_registered | COMPOSITOR | presence check only |
| input/src/session_trust.rs:183 | admit_trust | VFS | sender authentication by tid (selector 29) |
| kms/src/main.rs:40 | cell_main | NET_BROKER | `ServiceRegistrySnapshot` per request |
| kms/src/main.rs:41 | cell_main | SUPERVISOR | same |
| kms/src/main.rs:42 | cell_main | NET | used by `auth.rs:74` and `dispatch/relay.rs:123` to authorize |
| kms/src/storage/provider/silo.rs:118 | DevelopmentSiloProvider::call | SILO | REQUEST peer (A) |
| net-broker/src/routing.rs:132 | RemoteServiceProxy::new | NET_BROKER (self) | |
| net/src/interface.rs:106 | nic_driver_tid | NIC_DRIVER | cached in `NIC_DRIVER_TID`, invalidated on failure |
| silo/src/ipc.rs:29 | run | KMS | per-request peer authentication input |
| sshd/src/main.rs:73 | cell_main | VFS | presence wait |
| sshd/src/reactor.rs:54 | run | NET | tid cached for the process lifetime |
| supervisor/src/hostile_backend_recovery.rs:39 | handle | VFS or NET (`service_id`) | kill target |
| supervisor/src/hotswap.rs:97 | hotswap | `service_id` from `service_id_for_name` | swap target |
| supervisor/src/main.rs:128 | handler | COMPOSITOR | EVENT peer (A) |
| supervisor/src/power.rs:47 | shut_down | SUPERVISOR (self) | |
| supervisor/src/power.rs:111 | storage_stack | VFS, BLOCK_DRIVER (loop) | keep-alive set |
| vfs/src/access.rs:250 | default_service_lookup | injected: KMS (access/kms.rs:54), MACHINE_PIN (access/machine_pin.rs:47), HYPERVISOR_SERVICE_ID (access/guest_disk.rs:76 `live_hypervisor_matches`) | access-control authentication; also compares the caller's cell and generation |
| vfs/src/blk_router.rs:132 | wait_for_vfs_registration | VFS (self) | |

Not counted: `use` import lines (compositor input_handler.rs:15, message_handler.rs:10, httpd main.rs:14, reactor.rs:17, hypervisor net_backend.rs:12, run_loop.rs:28, input dispatcher.rs:23, main.rs:53, session.rs:14,24-25, kms silo.rs:3, net-broker routing.rs:34, net interface.rs:14, silo ipc.rs:5, sshd reactor.rs:6, supervisor hostile_backend_recovery.rs:9, hotswap.rs:17, main.rs:33, power.rs:14). Also not counted: test fakes in input/src/session_tests.rs:45 (`sys_lookup_service`) and :48 (`sys_lookup_service_bound`).

## C. Bound lookups (`sys_lookup_service_bound`), 3 call sites

| file:line | enclosing fn | service | note |
|---|---|---|---|
| input/src/main.rs:698 | focus_requester | COMPOSITOR | checks the sender's tid, cell_id, and generation against the attested caller |
| input/src/session.rs:380 | Sessions::binding | INPUT (self) | issuer binding for SessionV1 |
| vfs/src/prepared_cwd.rs:368 | reserve | VFS (self) | issuer binding |

Manifest allowlist mentions only (not calls): input/src/main.rs:73, vfs/src/main.rs:107 (`LookupServiceBound`).

## Totals

- Blocking `sys_send` sites: **54**. REPLY **48**, EVENT **4**, REQUEST **2**, UNCLEAR **0**.
- Reply-wrapper call sites (all REPLY, not in the 54): **107**.
- Tid-only lookups: **40** call sites. Bound lookups: **3**.

## Surprising

1. **compositor/src/message_handler/window.rs:151.** Within one CONFIGURE_ACK request it sends an event to the requester before the reply (:153). The kernel bridge turns the *first* send to the current caller into the reply (`async_ipc.rs:680-686`). If a client sends CONFIGURE_ACK with `IpcSubmit`, the state event would become its reply and the real `\x01` would be quarantined [INFERENCE; client-side transport not checked].
2. **Several services never call `ipc::current()` and depend only on the kernel legacy-reply bridge:** ai, config, ocel-js, every compositor handler, supervisor, and net `send_typed`/`send_ready`/`send_connect_reply`. httpd calls AI with `ipc::submit`, so AI replies always go through the bridge.
3. **The two REQUEST sites have no deadline on the send.** `kms silo.rs:119` blocks without limit, even though the receive after it is bounded to 8 ticks. `vfs blk_router.rs:76` is followed by an unbounded masked `sys_recv(tid)`, so VFS can park on a block driver on both the send and the receive.
4. **The supervisor makes blocking sends to other services while handling a request:**
   - Snapshot and restore events (`transfer.rs:15,28`) go to the old and new service tids during OP_HOTSWAP, before the caller gets its reply.
   - The 0xE2 owner-exited relay (`main.rs:132`) goes to COMPOSITOR.
   - A hung compositor or old service therefore parks the supervisor in `Sending`.
5. **Tid-only lookups are used to authorize senders:**
   - `kms auth.rs:74` / `main.rs:42` (NET)
   - `compositor message_handler.rs:69` (SUPERVISOR)
   - `input main.rs:301` (USB_HID_PRODUCER)
   - `input session_trust.rs:183` (VFS)
   - `silo ipc.rs:29` (KMS)
   - `hypervisor run_loop_x86.rs:331-332`, where the tids are cached for the life of the loop and go stale after a restart
   - `vfs access.rs:250` (KMS / MACHINE_PIN / HYPERVISOR)

   Only `input main.rs:698` uses the bound form for this purpose.
6. **Some tids are cached with no re-lookup:** `httpd main.rs:67-68` (NET, VFS) and `sshd reactor.rs:54` (NET).
7. **VFS reaches block drivers through `sys_lookup_block_route`, not the service registry.** That is a third peer-resolution path besides the tid-only and bound lookups.
