# P11 census — drivers-apps (cells/drivers, cells/apps, cells/demos, cells/tools, cells/tests)

HEAD `0f81a6597`. Source: `grep 'sys_send\('` (excluding `target/`), with each hit's context read. One row per call site.
`cells/tools/cellbox/src/executor.rs:2947` matches only in a comment and is not counted.

## 1. Blocking `sys_send(` sites — drivers

| file:line | enclosing fn | class | peer (service / tid source) | note |
|---|---|---|---|---|
| cells/drivers/ahci/src/main.rs:237 | `handler` (Message/RawMessage arm) | REPLY | VFS / `sender_tid` from AppEvent | block reply; sender gated by `sender_is_vfs` (tid-only lookup, cached) |
| cells/drivers/nvme/src/main.rs:180 | `handler` | REPLY | VFS / `sender_tid` | same shape as ahci |
| cells/drivers/pvscsi/src/dispatch.rs:50 | `serve` | REPLY | VFS / `sender_tid` | doc comment says the VFS waits in a blocking receive |
| cells/drivers/virtio-blk/src/main.rs:123 | `handler` | REPLY | VFS / `sender_tid` | comment: "matches the NVMe cell reply path" |
| cells/drivers/virtio-scsi/src/main.rs:178 | `handler` | REPLY | VFS / `sender_tid` | |
| cells/drivers/dwc2-usb/src/main.rs:612 | `cell_main` (NIC request loop) | REPLY | NET / `sender_tid` (bound-checked via `sender_is_live_provider`) | one-byte `STATUS_NOT_READY` answer; nearby failure path uses `ipc::reply` or `sys_try_send` |
| cells/drivers/dwc2-usb/src/main.rs:825 | `reply_nic` | REPLY | NIC client / tid carried in the lan_ipc envelope | **hybrid**: `ipc::reply(op)` when an operation token exists, else legacy `sys_send` |
| cells/drivers/dwc2-usb/src/lan_worker.rs:55 | `cell_main` | REPLY | DWC2 host / `host_tid` (attach sender, bound-checked `NIC_DRIVER`) | relays the validated envelope back to the host that forwarded it (the host forwards with `sys_try_send`) |
| cells/drivers/dwc2-usb/src/usb_hid.rs:955 | `forward_device_event` | EVENT | INPUT / `sys_lookup_service(INPUT)` tid | HID `EV_DEVICE` frame |
| cells/drivers/dwc2-usb/src/usb_hid.rs:975 | `register_as_source` | EVENT | INPUT / lookup tid | `RegisterEventSource`; input (services/input/src/main.rs:675) sends no reply. Blocking Send is **deliberate**: TrySend is refused because input never parks in Recv |
| cells/drivers/xhci/src/input.rs:38 | `register_as_source` | EVENT | INPUT / lookup tid | same deliberate-Send rationale (module doc) |
| cells/drivers/xhci/src/input.rs:50 | `forward_device_event` | EVENT | INPUT / lookup tid | |
| cells/drivers/wasm/src/imports.rs:41 | `vi_send` (wasm host import `vi.send`) | UNCLEAR | guest-chosen raw `target` tid | the `vi.send` ABI exposes a raw blocking send to WASM guests; the result is ignored (always returns 0). Its class depends on the guest |

## 2. Blocking `sys_send(` sites — apps

| file:line | enclosing fn | class | peer (service / tid source) | note |
|---|---|---|---|---|
| cells/apps/haily-agent/src/tools.rs:46 | `ToolRouter::execute` | REQUEST | tool cell / tid from `sys_spawn_from_path` | wildcard `sys_recv(0)` loop, filtered by sender |
| cells/apps/hypha/core/src/main.rs:204 | `ask` | REQUEST | llm-gateway / spawn tid | wildcard recv drops non-gateway frames, such as input events |
| cells/apps/hypha/core/src/main.rs:265 | `invoke` | REQUEST | tool cell / spawn tid | same |

## 3. Blocking `sys_send(` sites — demos

| file:line | enclosing fn | class | peer (service / tid source) | note |
|---|---|---|---|---|
| cells/demos/async-echo/src/main.rs:101 | `cell_main` | REQUEST | NET / tid-only lookup | send, then `sys_recv(net_tid)` |
| cells/demos/tokio-smoke/src/main.rs:96 | `cell_main` | REQUEST | NET / tid-only lookup | |
| cells/demos/robot-demo/src/mqtt.rs:45 | `publish_telemetry` | REQUEST | NET / tid-only lookup | `sys_recv_timeout` follows |
| cells/demos/robot-demo/src/mqtt.rs:171 | `tcp_try_send` | REQUEST | NET | |
| cells/demos/robot-demo/src/mqtt.rs:193 | `tcp_send_raw` | REQUEST | NET | the doc says "fire-and-forget", but the reply is drained |
| cells/demos/robot-demo/src/mqtt.rs:209 | `tcp_recv_once` | REQUEST | NET | |
| cells/demos/robot-demo/src/mqtt.rs:230 | `close_cap` | REQUEST | NET | |
| cells/demos/recovery-demo/src/bin/authority/lifecycle.rs:155 | `Provider::report` | REPLY | caller / `from.tid` | called only from `dispatch` on `Msg::Report` (authority/main.rs:135) |
| cells/demos/recovery-demo/src/bin/common.rs:30 | `send` (shared wrapper) | UNCLEAR | any | wrapper used for all three classes; its callers are classified below |

### 3a. Callers of `recovery-demo common::send` (indirect blocking send)

| file:line | enclosing fn | class | peer | note |
|---|---|---|---|---|
| …/bin/authority/main.rs:142 | `dispatch` | REPLY | `from.tid` | |
| …/bin/worker/serve.rs:39 | `run` (Get) | REPLY | `rx.sender` | |
| …/bin/worker/serve.rs:66 | `run` (Crash) | REPLY | `rx.sender` | Ack |
| …/bin/worker/serve.rs:89 | `Worker::mutate` | REPLY | client | Done (replayed) |
| …/bin/worker/serve.rs:109 | `Worker::mutate` | REPLY | client | interim `Staged`; the worker then waits for `Go` from the client |
| …/bin/worker/serve.rs:125 | `Worker::mutate` | REPLY | client | Done |
| …/bin/worker/serve.rs:136 | `Worker::mutate` | REPLY | client | Done |
| …/bin/worker/serve.rs:153 | `Worker::refuse` | REPLY | client | Failed |
| …/bin/worker/serve.rs:76 | `Worker::call` | REQUEST | authority / `wait_lookup` tid | `recv_from` with timeout |
| …/bin/worker/main.rs:64 | `cell_main` | REQUEST | authority / `wait_lookup` tid | Attach |
| …/bin/worker/main.rs:75 | `cell_main` | EVENT | authority | `RestoreFailed`; authority returns without replying; worker exits |
| …/bin/worker/main.rs:83 | `cell_main` | EVENT | authority | `Restored`; authority does not reply (main.rs:111-124) |
| …/bin/client/driver.rs:66 | `Client::worker_call` | REQUEST | worker / lookup tid | |
| …/bin/client/driver.rs:73 | `Client::authority_call` | REQUEST | authority / `wait_lookup` tid | |
| …/bin/client/driver.rs:102 | (status/report helper) | REQUEST | authority | Report, then `recv_raw_from` |
| …/bin/client/faults.rs:98 | `fenced_writer` | REQUEST | old worker tid | `Go` releases the staged mutation; the answer is read via `recv_from(old)` |

## 4. Blocking `sys_send(` sites — tools

| file:line | enclosing fn | class | peer (service / tid source) | note |
|---|---|---|---|---|
| cells/tools/cellbox/src/cmd_fs.rs:933 | `find_recursive` | REQUEST | VFS / `vfs_endpoint()` (tid-only, spin) | send result ignored |
| cells/tools/cellbox/src/cmd_fs.rs:978 | `vfs_list_dir_details` | REQUEST | VFS / `vfs_endpoint()` | |
| cells/tools/cellbox/src/cmd_sys.rs:227 | `cmd_ifconfig` | REQUEST | NET / tid-only lookup | |
| cells/tools/cellbox/src/config_client.rs:68 | `ConfigClient::get` | REQUEST | CONFIG / tid-only `endpoint()` | masked recv |
| cells/tools/cellbox/src/config_client.rs:96 | `ConfigClient::set` | REQUEST | CONFIG / tid-only | |
| cells/tools/cellbox/src/text_tools/grep/io/vfs.rs:77 | `request_vfs` | REQUEST | VFS / tid-only lookup (spin) | |
| cells/tools/init/src/supervisor.rs:239 | `relay_hypervisor_exit` | EVENT | SUPERVISOR / tid-only lookup | death notice (`0xE1` + dead tid). **Init** can park in Sending if the supervisor is not receiving |
| cells/tools/sys-tools/src/bin/hotswap.rs:114 | `send_request` | REQUEST | SUPERVISOR / tid-only lookup | `recv_status` with timeout |
| cells/tools/sys-tools/src/bin/shutdown.rs:92 | `request_from_supervisor` | REQUEST | SUPERVISOR / tid-only lookup | `sys_recv_timeout` |
| cells/tools/sys-tools/src/bin/kill.rs:49 | `cell_main` | EVENT | arbitrary user tid (argv) whose ProcessInfo state==2 | cooperative `0xFF` shutdown sentinel. Blocks if the target left Recv between `get_procs` and the send |
| cells/tools/net-tools/src/bin/curl.rs:84 | `cell_main` | REQUEST | NET / tid-only lookup @51 | TcpConnect |
| cells/tools/net-tools/src/bin/curl.rs:134 | `cell_main` | REQUEST | NET | TcpSend loop |
| cells/tools/net-tools/src/bin/curl.rs:170 | `cell_main` | REQUEST | NET | TcpRecv poll |
| cells/tools/net-tools/src/bin/curl.rs:188 | `cell_main` | REQUEST | NET | final drain |
| cells/tools/net-tools/src/bin/curl.rs:250 | `query_state` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/curl.rs:267 | `close_socket` | REQUEST | NET | reply drained and ignored |
| cells/tools/net-tools/src/bin/curl.rs:309 | `resolve_host` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/httpd.rs:90 | `close_cap` | REQUEST | NET / lookup @288 | |
| cells/tools/net-tools/src/bin/httpd.rs:100 | `query_state` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/httpd.rs:181 | `tcp_send` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/httpd.rs:216 | `drain_request` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/httpd.rs:310 | `cell_main` | REQUEST | NET | TcpListen |
| cells/tools/net-tools/src/bin/httpd.rs:335 | `cell_main` | REQUEST | NET | accept loop |
| cells/tools/net-tools/src/bin/mqtt.rs:90 | `cell_main` | REQUEST | NET / lookup @65 | |
| cells/tools/net-tools/src/bin/mqtt.rs:177 | `MqttConn::fill` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/mqtt.rs:391 | `tcp_send` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/mqtt.rs:416 | `close_socket` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/mqtt.rs:450 | `resolve_host` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:89 | `cell_main` | REQUEST | NET / lookup @40 | |
| cells/tools/net-tools/src/bin/nc.rs:122 | `cell_main` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:154 | `cell_main` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:183 | `server_mode` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:217 | `server_mode` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:239 | `server_mode` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:273 | `serve_connection` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:293 | `serve_connection` | REQUEST | NET | echo TcpSend |
| cells/tools/net-tools/src/bin/nc.rs:318 | `query_state` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:334 | `close_socket` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/nc.rs:351 | `resolve_host` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/wget.rs:108 | `cell_main` | REQUEST | NET / lookup @76 | |
| cells/tools/net-tools/src/bin/wget.rs:164 | `cell_main` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/wget.rs:199 | `cell_main` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/wget.rs:214 | `cell_main` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/wget.rs:344 | `vfs_call` | REQUEST | VFS / lookup @83 | |
| cells/tools/net-tools/src/bin/wget.rs:363 | `query_state` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/wget.rs:379 | `close_socket` | REQUEST | NET | |
| cells/tools/net-tools/src/bin/wget.rs:422 | `resolve_host` | REQUEST | NET | |

## 5. Indirect blocking sends via `AppContext::send` / `send_msg` (libs/ostd/src/app.rs:304,315 → `sys_send`)

`grep sys_send(` does not find these sites. They are listed for completeness and are not included in the totals.

| file:line | enclosing fn | class | peer | note |
|---|---|---|---|---|
| cells/apps/hypha/llm-gateway/src/main.rs:82 | `handle` | REPLY | `sender` | |
| cells/apps/hypha/tools/fs/src/main.rs:72 | `handle` | REPLY | `sender` | |
| cells/apps/hypha/tools/spawn/src/main.rs:66 | `handle` | REPLY | `sender` | |
| cells/apps/hypha/tools/sys/src/main.rs:60 | `handle` | REPLY | `sender` | |
| cells/demos/hotswap-demo-v1/src/main.rs:133,143,145 | handler (Message arm) | REPLY ×3 | `sender_tid` | |
| cells/demos/hotswap-demo-v2/src/main.rs:111,120,122 | handler | REPLY ×3 | `sender_tid` | |
| cells/demos/sdk-demo/src/main.rs:40 | handler | REPLY | `sender_tid` | echo via `send_msg` |

## 6. cells/tests — per-file summary (85 sites, 31 files)

| file (lines) | sites | class breakdown | note |
|---|---|---|---|
| bench/src/bench-probe.rs (120) | 1 | REPLY 1 | ACK to orchestrator |
| bench/src/echo.rs (20) | 1 | REPLY 1 | legacy echo provider |
| bench/src/main.rs (295) | 1 | REQUEST 1 | probe start, then `sys_recv(probe_tid)` |
| bench/src/scenarios/async_lifecycle.rs (265,299,487,583,585,626,709,718,910,937,984,1027,1073,1100,1165,1174,1176,1224,1268,1297) | 20 | EVENT 15 (control/release frames), REPLY 4 (487, 1073, 1174, 1268), REQUEST 1 (984) | **1174 is a deliberate late legacy Send to a still-served caller (the "immediate-reply bridge"), which the test expects to be quarantined.** 487 is a raw source replying outside RPC context |
| bench/src/scenarios/c2c_broker_oracle_client.rs (40,69,119,129) | 4 | REQUEST 2 (broker), REPLY 1 (ready→parent), EVENT 1 (posted progress) | 119 pipelines sends before collecting replies |
| bench/src/scenarios/c2c_broker_oracle_orchestrator.rs (123,125,127,185,191) | 5 | REQUEST 4 (drain→summary), EVENT 1 (start) | fan-out to child tids |
| bench/src/scenarios/c2c_broker_oracle_client/calibration.rs (125) | 1 | REQUEST 1 | broker |
| bench/src/scenarios/c2c_broker_oracle_client/support.rs (76) | 1 | REPLY 1 | summary answering drain, then exit |
| bench/src/scenarios/c2c_broker_oracle_orchestrator/lifecycle.rs (41,48,96) | 3 | REQUEST 3 | 48 deliberately sends to a stale broker tid after a restart request |
| bench/src/scenarios/c2c_broker_oracle_orchestrator/support.rs (27,63) | 2 | REQUEST 2 | config→ready; broker snapshot |
| bench/src/scenarios/control_loop.rs (111,113) | 2 | REPLY 2 | result to orchestrator |
| bench/src/scenarios/cpu_accounting.rs (167) | 1 | EVENT 1 | release |
| bench/src/scenarios/hotswap_supervisor.rs (62,100,135,137) | 4 | REQUEST 4 | 62 expects `Err` on the post-cutover old tid; 135/137 queue inc/get on the old tid during cutover |
| bench/src/scenarios/ipc_fastpath.rs (67) | 1 | REQUEST 1 | echo bench |
| bench/src/scenarios/ipc_send_recv.rs (83) | 1 | REQUEST 1 | echo bench |
| bench/src/scenarios/local_service_lifecycle.rs (206,209,261,484,490,498,519,522,1307) | 9 | REPLY 3, REQUEST 2 (484, 519 gate handshakes), EVENT 4 | 209 is a deliberate duplicate reply (quarantine test) |
| bench/src/scenarios/native_stateful.rs (187,267,276,354,367,388) | 6 | REQUEST 6 | 354/367: the cached-sender probe sends `inc` and exits without reading the reply |
| bench/src/scenarios/preempt_latency.rs (34,47) | 2 | REPLY 1, REQUEST 1 | |
| bench/src/scenarios/smp.rs (101,106,232) | 3 | EVENT 1, UNCLEAR 1, REQUEST 1 | **106 deliberately parks in Sending on a peer that is then killed, expecting `Err`.** This tests the legacy blocking semantics directly |
| bench/src/scenarios/vfs_getfile_breakdown.rs (75,78,220) | 3 | REPLY 2, REQUEST 1 | fake VFS server + client |
| edu-holder/src/main.rs (199) | 1 | REPLY 1 | **hybrid** `answer()`: `ipc::reply` if op, else `sys_send` |
| phase04-terminal-driver/src/ssh/kssh.rs (1172) | 1 | REQUEST 1 | raw KMS probe, tid-only lookup |
| pipe-peer/src/main.rs (70,90) | 2 | EVENT 2 | to parent |
| pipe-test/src/main.rs (115) | 1 | EVENT 1 | handle to writer child |
| silo-test/src/main.rs (91) | 1 | REQUEST 1 | SILO (denial probe) |
| srv-test/src/main.rs (49) | 1 | REQUEST 1 | the reply is read with a **wildcard** `sys_recv(0)` |
| tier2-grant-owner/src/main.rs (211,215) | 2 | REPLY 2 | |
| tier2-grant-receiver/src/main.rs (166) | 1 | REQUEST 1 | protocol byte, then `recv_byte_from` |
| tier2-rpc-driver/src/main.rs (129,140) | 2 | UNCLEAR 2 | kernel-ABI negative probes (oversize / invalid buffer) to a bound provider tid; expected to be refused, never delivered |
| tier2-rpc-provider/src/main.rs (131) | 1 | REPLY 1 | |
| vfs-test/src/main.rs (79) | 1 | REQUEST 1 | |

## 7. Service lookups

### 7a. Tid-only (`sys_lookup_service` / `ostd::service::lookup` / local wrappers) — non-test

| file:line | enclosing fn | service | note |
|---|---|---|---|
| cells/drivers/ahci/src/main.rs:88 | `sender_is_vfs` | VFS | result cached in `VFS_TID` and never cleared |
| cells/drivers/nvme/src/main.rs:70 | `sender_is_vfs` | VFS | same |
| cells/drivers/pvscsi/src/dispatch.rs:28 | `sender_is_vfs` | VFS | same |
| cells/drivers/virtio-blk/src/main.rs:64 | `sender_is_vfs` | VFS | same |
| cells/drivers/virtio-scsi/src/main.rs:60 | `sender_is_vfs` | VFS | same |
| cells/drivers/dwc2-usb/src/main.rs:411 | `cell_main` | INPUT | |
| cells/drivers/dwc2-usb/src/main.rs:424 | `cell_main` | INPUT | relookup only when tid==0 |
| cells/drivers/xhci/src/main.rs:180 | `init` | INPUT | |
| cells/drivers/xhci/src/main.rs:201 | `poll` | INPUT | relookup only when tid==0 |
| cells/apps/chrome/src/main.rs:670 | `cell_main` | HYPERVISOR | |
| cells/apps/desktop/src/taskbar.rs:106,107,108 | `update_daemons` | NET, VFS, literal `15 /* AI */` | presence check only |
| cells/apps/haily-agent/src/main.rs:81 | `cell_main` | NET_BROKER | |
| cells/apps/hypha/llm-gateway/src/transport.rs:96 | `tls` | NET | |
| cells/apps/hypha/tools/sys/src/main.rs:124 | `dispatch` | caller-named | exposes the tid to the LLM tool result |
| cells/apps/ocel/src/lease.rs:34 | `KernelTransport::lookup` (Transport impl) | any | used at lease.rs:156, 220, 221, 242 (OCEL_ACTIVATOR + engine), compared against held tids |
| cells/apps/ocel/src/net/http.rs:550 | `KernelTransport::endpoint` | NET | |
| cells/apps/vm-runner/src/main.rs:122 | `cell_main` | HYPERVISOR | |
| cells/apps/c2c-render/src/main.rs:937 | `save_ppm` | VFS | via `ostd::service::lookup` |
| cells/demos/async-echo/src/main.rs:78 | `cell_main` | NET | |
| cells/demos/https-demo/src/main.rs:25 | `cell_main` | NET | |
| cells/demos/robot-demo/src/mqtt.rs:26 | `publish_telemetry` | NET | |
| cells/demos/tokio-smoke/src/main.rs:85 | `cell_main` | NET | |
| cells/demos/recovery-demo/src/bin/common.rs:68 | `lookup` (wrapper; `wait_lookup` @72 polls it) | any | callers: client/driver.rs:41,44 (wait_lookup), 139, 156; client/cycles.rs:30; client/extra.rs:83; worker/fault.rs:58; worker/main.rs:61 (wait_lookup) |
| cells/tools/cellbox/src/async_utils.rs:42,43 | `read_line` | INPUT, COMPOSITOR | presence check |
| cells/tools/cellbox/src/cmd_fs.rs:16 | `vfs_endpoint` | VFS | unbounded spin |
| cells/tools/cellbox/src/cmd_machine.rs:126 | `run` | MACHINE_PIN | |
| cells/tools/cellbox/src/cmd_sys.rs:217 | `cmd_ifconfig` | NET | |
| cells/tools/cellbox/src/config_client.rs:34 | `ConfigClient::endpoint` | CONFIG | |
| cells/tools/cellbox/src/main.rs:220 | `run_legacy_shell` | COMPOSITOR | presence check |
| cells/tools/cellbox/src/shell_test.rs:20 | `wait_for_vfs` | VFS | presence |
| cells/tools/cellbox/src/text_tools/grep/io/vfs.rs:72 | `request_vfs` | VFS | unbounded spin |
| cells/tools/init/src/activation.rs:202 | `acquire` | engine service | liveness compare vs `provider.tid` |
| cells/tools/init/src/activation.rs:254 | `acquire_watched` | engine service | compare |
| cells/tools/init/src/activation.rs:317 | `start_engine` | engine service | readiness compare |
| cells/tools/init/src/activation.rs:347 | `spawn_via_vfs` | VFS | |
| cells/tools/init/src/configuration.rs:17 | `wait_for_vfs` | VFS | compare |
| cells/tools/init/src/configuration.rs:55 | `load` (`read_required` path) | VFS | |
| cells/tools/init/src/machine_pin.rs:180 | `gate` | MACHINE_PIN | registration self-check |
| cells/tools/init/src/service_table.rs:67 | `Service::available` | spec id | compare |
| cells/tools/init/src/service_table.rs:88 | `spawn_path` | VFS | |
| cells/tools/init/src/service_table.rs:129 | `spawn` | spec id | registration self-check |
| cells/tools/init/src/service_table.rs:161 | `wait_for_exact_registration` | spec id | |
| cells/tools/init/src/supervisor.rs:238 | `relay_hypervisor_exit` | SUPERVISOR | feeds the EVENT send @239 |
| cells/tools/net-tools/src/bin/curl.rs:51 | `cell_main` | NET | held for the whole session |
| cells/tools/net-tools/src/bin/httpd.rs:288,295 | `cell_main` | NET, VFS | held for the server lifetime |
| cells/tools/net-tools/src/bin/mqtt.rs:65 | `cell_main` | NET | |
| cells/tools/net-tools/src/bin/nc.rs:40 | `cell_main` | NET | |
| cells/tools/net-tools/src/bin/wget.rs:76,83 | `cell_main` | NET, VFS | |
| cells/tools/sys-tools/src/bin/hotswap.rs:46 | `cell_main` | SUPERVISOR | |
| cells/tools/sys-tools/src/bin/shutdown.rs:79 | `request_from_supervisor` | SUPERVISOR | |

### 7b. Bound lookups — non-test

| file:line | enclosing fn | service | form |
|---|---|---|---|
| cells/demos/recovery-demo/src/bin/client/driver.rs:58 | (`note_incarnation`) | WORKER_SERVICE | direct `sys_lookup_service_bound` |
| cells/drivers/{bcm57xx/main.rs:131, e1000/main.rs:156, e1000e/dispatch.rs:100, i40e/main.rs:131, igb/main.rs:183, igc/main.rs:130, ixgbe/main.rs:131, r8169/main.rs:130, virtio-net/main.rs:84, vmxnet3/dispatch.rs:101} | request handler | NET | `ostd::service::sender_is_live_provider` (→ `sys_lookup_service_bound`, libs/ostd/src/service.rs:247) |
| cells/drivers/dwc2-usb/src/main.rs:574 | `cell_main` | NET | `sender_is_live_provider` |
| cells/drivers/dwc2-usb/src/lan_worker.rs:33 | `cell_main` | NIC_DRIVER | `sender_is_live_provider` |

### 7c. Lookups in cells/tests

- **Tid-only (36):** bench/scenarios/base_tray_handoff.rs:463; hotswap_cli_probe.rs:18,30,47; hotswap_supervisor.rs:20,23,34,120,134; init_giveup.rs:73,86,127,157; lab_carrier_transfer.rs:49; local_service_lifecycle.rs:1383,1402; native_stateful.rs:89,92,95,353,365,401,414; stationary_assembly.rs:281; c2c_broker_oracle_client/support.rs:55; c2c_broker_oracle_orchestrator/lifecycle.rs:35,49,81; c2c_broker_oracle_orchestrator/support.rs:59; export-client/src/main.rs:83; phase04-terminal-driver/src/ssh/kssh.rs:1220,1263; silo-test/src/main.rs:81; srv-test/src/main.rs:34; vfs-test/src/main.rs:50; c-spawn/src/main.rs:52 (`ostd::service::lookup`).
- **Bound, direct (4):** local_service_lifecycle.rs:1385, 1403, 1481; tier2-rpc-driver/src/main.rs:68 (`live_binding`).
- **Bound via SDK `ServiceRef` (3):** local_service_lifecycle.rs:1483; tier2-rpc-driver/src/main.rs:73; tier2-rpc-provider/src/main.rs:146.

## 8. Totals

| scope | REPLY | EVENT | REQUEST | UNCLEAR | total |
|---|---|---|---|---|---|
| drivers | 8 | 4 | 0 | 1 | 13 |
| apps | 0 | 0 | 3 | 0 | 3 |
| demos | 1 | 0 | 7 | 1 | 9 |
| tools | 0 | 2 | 45 | 0 | 47 |
| **non-test subtotal** | **9** | **6** | **55** | **2** | **72** |
| tests (31 files) | 20 | 26 | 36 | 3 | 85 |
| **all `sys_send(` sites** | **29** | **32** | **91** | **5** | **157** |
| (indirect) recovery `common::send` callers | 8 | 2 | 6 | 0 | 16 |
| (indirect) `AppContext::send/send_msg` | 11 | 0 | 0 | 0 | 11 |

The 55 non-test REQUEST sites are cellbox 6, net-tools 37, sys-tools 2, robot-demo 5, async-echo/tokio-smoke 2, and hypha/haily 3.

Lookups:
- Non-test: 55 direct tid-only calls, plus the recovery-demo wrapper's 8 callers and ocel's `Transport::lookup` callers (4). Bound: 1 direct and 12 via `sender_is_live_provider`.
- Tests: 36 tid-only, 4 bound direct, 3 `ServiceRef`.

## 9. Surprises

1. **Block drivers use a tid-only lookup for authorization.** ahci, nvme, pvscsi, virtio-blk and virtio-scsi check `sender_is_vfs` with `sys_lookup_service(VFS)`, then cache the match in `VFS_TID` permanently. A restarted VFS gets a new tid and passes the lookup; a stale cached tid also stays accepted. Every NIC driver (and dwc2) uses the bound `sender_is_live_provider` instead.
2. **Hybrid reply paths already exist.** `dwc2-usb reply_nic` (main.rs:825) and `edu-holder answer` (main.rs:199) branch at runtime: they use `ipc::reply(op)` when the request carried an operation token, and fall back to legacy `sys_send` when it did not.
3. **Implicit reply bridge.** async_lifecycle.rs:477-479 and 1172-1174 state that a plain Send to the currently served caller *is* that operation's reply (Spec 20 §2.6). The block-driver `sys_send(sender_tid)` REPLY rows may therefore already complete exact operations through the kernel.
4. **Some EVENT sends must be blocking by design.** The input `RegisterEventSource` (dwc2 usb_hid.rs:975, xhci input.rs:38) and HID event frames must use blocking Send. TrySend is refused because the input service never parks in Recv, so these driver producers can park in `Sending` on the input service.
5. **Init has a blocking EVENT send.** `relay_hypervisor_exit` (init/supervisor.rs:239) sends to the supervisor with no deadline. `kill` (sys-tools/kill.rs:49) blocking-sends a `0xFF` sentinel to an arbitrary tid, based on a racy `get_procs` state check.
6. **WASM exposes raw blocking send.** The `vi.send` host import (drivers/wasm/src/imports.rs:41) gives guests an unrestricted raw blocking `sys_send` to any tid, and the result is discarded.
7. **The `sys_send(` grep misses sends.** `AppContext::send/send_msg` wrap `sys_send`, which hides 11 REPLY sites (hypha gateway and 3 tool cells, hotswap-demo v1/v2, sdk-demo). The recovery-demo `common::send` wrapper hides 16 more sites of all three classes.
8. **net-tools is the bulk of the REQUEST sites.** It accounts for 37 of the 55 non-test REQUEST sites: bare `sys_send` with the result ignored, then `recv_from`. Each tool resolves NET (and VFS) once by tid and holds it for the whole session.
9. **lan_worker is asymmetric.** The host forwards to the worker with `sys_try_send`, but the worker relays back with a blocking `sys_send` (lan_worker.rs:55).
10. **Wildcard receives after a REQUEST.** srv-test (main.rs:49→52) and hypha/haily use `sys_recv(0)` plus sender filtering, and discard unrelated frames such as input events.
11. **Magic service id.** desktop taskbar.rs:108 looks up service id literal `15 /* AI */`.
12. **Tests exercise the legacy semantics directly.** smp.rs:106 parks in Sending on a peer that is then killed. async_lifecycle.rs:1174 and local_service_lifecycle.rs:209 deliberately send late or duplicate legacy replies. tier2-rpc-driver.rs:129/140 are kernel-refusal probes. These sites are tests *of* `sys_send` and cannot simply migrate.
