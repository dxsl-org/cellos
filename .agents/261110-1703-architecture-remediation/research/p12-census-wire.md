# Phase 12 service wire census at `f094e9df5`

All 13 wires are inventoried below. Of the 28 files that declare `const OP_*: u8`, 21 belong to a service wire and 7 do not (GPU, SCSI and AML opcodes). About **350 literal sites** sit outside a single owner module. Six wires already have one owner module: block, kms, silo, ai, vmm_session and compositor. The duplication is concentrated in five wires: supervisor, net errors, TLS, NIC and input-producer. This was a read-only pass: nothing was edited and no checks were run.

## Which wires are postcard and which are raw

| Kind | Wire | Owner today |
|---|---|---|
| Postcard enum | `VfsRequest` (46 variants, 0..45) / `VfsResponse` (0..17) | `libs/api/src/services/ipc.rs:40,537` |
| Postcard enum | `NetRequest` (0..20) / `NetResponse` (0..9) | `ipc.rs:796,909` |
| Postcard enum | `InputRequest` (0..4) / `InputResponse` (0..3) | `ipc.rs:942,1005` |
| Postcard enum | `ConfigRequest` (0..3) / `ConfigResponse` (0..4) | `ipc.rs:1025,1043` |
| Postcard enum | `AiRequest` (0..4) / `AiResponse` (0..4) / `AiError` | `libs/ai-proto/src/lib.rs:235,294,172` |
| Raw consts inside `ipc.rs` | `net_ready`, `NET_READY_OPCODE`, `REACTOR_WAKE_OPCODE`, `OP_HID_DEVICE_REMOVED`, `OP_SET_LEDS` | `ipc.rs:716-735,976,990` |
| Raw | supervisor, App SDK envelope, TLS, NIC, input producer frames, compositor, hostile-kill | spread out (tables below) |
| Raw, fixed frame, one owner | block (`libs/types/src/block_wire.rs`), kms 128 B (`libs/types/src/kms/*`), silo 192 B (`libs/types/src/silo/wire.rs`), vmm_session (`libs/api/src/services/vmm_session*`), compositor (`libs/api/src/services/display.rs:1130-1278`), hostile-kill (`libs/api/src/services/hostile_backend_recovery.rs`) | |
| Existing proto-like modules | `cells/services/supervisor/src/protocol.rs`, `cells/services/net/src/tls_wire.rs`, `libs/ostd/src/clients/vfs/read_file/wire.rs`, `cells/services/net-broker/src/{local_ingress,bench_oracle/commands,routing}.rs`, `libs/api/src/services/{session,input}.rs`, `libs/intel-gpu-core/src/wire.rs`, `cells/guests/silo-guest/src/layout.rs` (pulled into silo via `#[path]` at `silo/src/lib.rs:11`) | |

## Supervisor and the App SDK envelope

Frames: `[0xAC,0x00,op,…]`. Hotswap is 193 B (`protocol.rs:34-36`), power is 2 B (`:38`), the status reply is `[0x03,phase,result]` (`:148`).

| name | value | meaning | declared at | note |
|---|---|---|---|---|
| APP_MSG_MAGIC | 0xAC | envelope byte 0 | `libs/ostd/src/app.rs:41`; `supervisor/src/transfer.rs:6`; `kernel/src/acpi/sci.rs:40`; `supervisor/src/power.rs:35`; `kernel/src/task/syscall/authority.rs:252`; `kernel/src/task/gpu-scanout.rs:192,266,274`; `sys-tools/src/bin/hotswap.rs:13`; `shutdown.rs:61`; bench `hotswap_supervisor.rs:12,49,52`, `native_stateful.rs:267,276,348,379`, `nonreceiving_client.rs:35` | dup ×14 |
| Message / Shutdown | 0x00 / 0xFF | envelope byte 1 | `app.rs:13,414`; shutdown reasons 1 = Watchdog, 2 = ParentDied at `app.rs:473-474` | no `[0xAC,0xFF]` sender found |
| Snapshot / Restore | 0xF0 / 0xF1 | hot-swap events; 10 B and 66 B | `app.rs:425,433`; `transfer.rs:7-8` | dup |
| CapRevoked | 0xF2 | `[0xAC,0xF2,mask_le4]` | `app.rs:440`; `authority.rs:253` | dup |
| OP_HOTSWAP | 0x01 | hotswap request | `protocol.rs:21`; `hotswap.rs:14`; `hotswap_supervisor.rs:10`; `native_stateful.rs:31` | dup ×4 |
| OP_SNAPSHOT | 0x02 | snapshot request | `protocol.rs:22`; `nonreceiving_client.rs:37` | dup |
| OP_STATUS | 0x03 | status reply opcode | `protocol.rs:23`; `hotswap.rs:15`; `shutdown.rs:65`; `hotswap_supervisor.rs:11`; `nonreceiving_client.rs:39`; `native_stateful.rs:396` | dup ×6 |
| OP_POWER | 0x04 | power request | `protocol.rs:24`; `shutdown.rs:63`; `sci.rs:40` (literal); test at `protocol.rs:250` | dup; same byte as `KILL_REQUEST_OPCODE`, kept apart only by the envelope |
| power actions | 1 / 2 | reboot / poweroff | `protocol.rs:40-41`; `sci.rs:40` | dup |
| phases | 1 / 2 / 6 / 0 / 0xFF | snapshot / power / hotswap success / pre-dispatch refusal / HotswapError | `protocol.rs:25,27`; server literals at `main.rs:92` (`6,0x00`) and `main.rs:96` (`0xFF`); `hotswap.rs:20-21,137-147`; `native_stateful.rs:396` | phase 6 and phase 0xFF have no named constant on the server |
| STATUS_OK / UNAVAILABLE | 0x00 / 0x01 | result | `protocol.rs:28-29` | |
| REJECTED_CALLER / SERVICE_NOT_FOUND / INVALID_REQUEST | 0xFD / 0xFE / 0xFF | result | `protocol.rs:30-32`; `hotswap.rs:137,140,143`; `hotswap_supervisor.rs:16`; `nonreceiving_client.rs:39` | dup; 0xFF is also the error phase |
| HotswapError | 0–8 | `#[repr(u8)]` result when phase is 0xFF | `supervisor/src/error.rs:5-24`; re-typed as text at `hotswap.rs:185-194` | dup |
| hostile-kill req / resp | 0x04 / 0x05; 3 B / 2 B | raw, feature-gated | `api/.../hostile_backend_recovery.rs:7-13`; dispatched at `main.rs:124` | **collision with supervisor statuses:** `KILL_STATUS` 0..5 at `:16-26` give 0x01 = INVALID_REQUEST and 0x02 = REJECTED_CALLER |
| init owner-exit relay | 0xE1 → 0xE2 | `[op, tid_le]` | `cells/tools/init/src/supervisor.rs:237`; supervisor `main.rs:131,135`; compositor `message_handler.rs:18`; `owner_watch.rs:297` | dup ×5 |
| KERNEL_SENDER | 0 | sender id of the power button | `sci.rs:43`; `supervisor/src/main.rs:50` | dup |

## Net (typed wire and its error bytes)

| name | value | meaning | declared at | note |
|---|---|---|---|---|
| NET_ERR_DENIED | 0xFC | missing right | `ipc.rs:928`; `ostd/src/clients/net.rs:22` | |
| NET_ERR_ADDR_IN_USE | 0xFB | port in use | `ipc.rs:936`; `handlers.rs:302` | |
| Err(0xFF) | 0xFF | generic failure **and** TcpRecv EOF | 43 literal sites: `handlers.rs` ×10, `handlers/tcp.rs` ×21, `handlers/udp.rs` ×3, `service-runtime.rs` ×9 (including the 0xFE/0xFD rows below); the EOF case is `tcp.rs:105`; the client maps it to EOF at `ostd/src/clients/net.rs:285-287`; tests at `net.rs:334`, `net-broker/src/beacon.rs:576` | **collision** (also see the generic map below) |
| Err(0xFE) | 0xFE | socket not Established | `tcp.rs:164` | no name |
| Err(0xFD) | 0xFD | full / busy / wait refused | `handlers.rs:303`; `tcp.rs:284`; `service-runtime.rs:172,289,294,299` | no name; supervisor uses 0xFD for REJECTED_CALLER |
| net_ready bits | 1/2/4/8/16 | ACCEPT/READ/WRITE/EOF/ERROR | `ipc.rs:716-721` | |
| NET_READY_OPCODE | 0x11; frame 6 B | ready push | `ipc.rs:725,731` | **declared, never used** |
| REACTOR_WAKE_OPCODE | 0x12 | reactor self-wake | `ipc.rs:728`; `kernel/src/task.rs:2797,2816`; `libs/vimio/src/lib.rs:79,301,420`; `libs/vipolling/src/lib.rs:268,286` | 0x12 is also `NetRequest::TcpRecvReady` (index 18) |
| record / page sizes | 5 B; 32 | ready record; `READY_PAGE` | `ipc.rs:735`; `tcp.rs:321` | |

The generic error map `ostd/src/clients.rs:60-75` (0 → OOM … 10 → NotADirectory, everything else → Unknown) is what input and net clients decode with.

## TLS (raw, routed when postcard decode fails and the first byte is ≥ 0x15)

| name | value | layout / meaning | declared at | note |
|---|---|---|---|---|
| TLS_CLOSE_OP | 0x15 | `[op][cap u64]`, 9 B | `net/src/tls_wire.rs:7`; `ostd/src/tls.rs:32`; `apps/ocel/src/net/http.rs:1260`; routing threshold `handlers.rs:151`, `rights.rs:128` | dup ×3; **collision:** 0x15 is the next postcard `NetRequest` index (21) |
| TLS_CONNECT_OP | 0x30 | `[op][cap 8][ip 4][port 2][hn_len 2][host ≤495]` | `tls_wire.rs:8,63`; `tls.rs:29,50`; `ocel/http.rs:837`; `ocel/http_tests.rs:49` | dup ×3 |
| TLS_SEND_OP | 0x31 | `[op][cap][len 2][data ≤501]` | `tls_wire.rs:9,12`; `tls.rs:30,73`; `ostd/src/clients/tls_stream.rs:39`; `ocel/http.rs:846-847`; `http_tests.rs:53` | dup ×4 |
| TLS_RECV_OP | 0x32 | `[op][cap][want 4]`, ≤4094 | `tls_wire.rs:10,13`; `tls.rs:31,124`; `ocel/http.rs:854-856`; `http_tests.rs:59` | dup ×3 |
| connect reply | cap u64; 0 = fail | | `tls_handler.rs:40-44`; denial at `rights.rs:111-113` | |
| send reply | u32 count; `u32::MAX` = error | | `tls_handler.rs:155`; `tls/dispatch.rs:108` (`[0xff;4]`); `tls.rs:84` | the same sentinel written two ways |
| recv reply | `[len u16][data]`; len 0 = EOF; `u16::MAX` = error | | `tls_wire.rs:111-120`; `tls_handler.rs:169`; `dispatch.rs:109,421`; `tls.rs:104` | the same sentinel written two ways |
| close reply | `[0x00]` ok / `[0xFF]` fail / empty = parse error | | `tls_handler.rs:83,98` | |

## NIC driver wire (net ↔ NIC driver; status reply is 1 byte)

| name | value | declared at | note |
|---|---|---|---|
| OP_TX `[0,len u16,frame]` / OP_RX `[1]` / OP_GETMAC `[2]` | 0 / 1 / 2 | `net/src/interface.rs:61-63`; `dispatch.rs` in `e1000:20-22`, `e1000e:24-26`, `igb:17-19`, `igc:5-7`, `i40e:5-7`, `ixgbe:5-7`, `r8169:5-7`, `bcm57xx:5-7`, `vmxnet3:25-27`, `virtio-net:20-22`, `dwc2-usb:10-12` | dup ×12 (36 consts) |
| STATUS_OK / FAILED / NOT_READY | 0 / 1 / 2 | `interface.rs:68-69,162` (test); `dwc2-usb/dispatch.rs:21-25`; literal `Status(n)` 6× in each of 10 `dispatch.rs`; `reply(&[1u8])` 2× in each of 10 drivers (e.g. `e1000/main.rs:157,174`) | 80 literal sites |
| REPLY_BUF / MIN_FRAME | `2 + BUF_SIZE` / 14 | each `dispatch.rs` (+3 lines after the OP consts); `interface.rs:58` | |
| dwc2 LAN shim | 0xD0 / 0xD1 / 0xD2 | `dwc2-usb/src/lan_ipc.rs:7-9` | internal to the driver |

## VFS (postcard; `VfsResponse::Err(u8)` is index 4)

| name | value | meaning | declared at | note |
|---|---|---|---|---|
| ERR_IO | 1 | I/O error or missing path | `vfs/src/paths.rs:14`; `ostd/.../read_file/wire.rs:4`; `runtimes/lua/src/bindings_vfs_handle_read.rs:170`; `httpd/src/reactor.rs:173,1199`; `tools/init/src/configuration.rs:73`; `net-tools/src/bin/httpd.rs:124`, `httpd/tests.rs:51` | dup ×6; **collides** with generic map 1 = InvalidArgument |
| ERR_QUOTA | 2 | quota exceeded | `paths.rs:16`; `wire.rs:5`; `lua:171` | generic map 2 = NotFound |
| ERR_DENIED | 3 | permission denied | `paths.rs:12`; `wire.rs:6`; `lua:172`; `vfs/src/main.rs:205` (literal); `tests/vfs-test/src/grant_io.rs:91` | net and config use 0xFC for "denied" |
| ERR_HANDLE | 4 | bad handle | `paths.rs:18`; `wire.rs:7`; `lua:173`; `httpd/tests.rs:66` | generic map 4 = AlreadyExists |
| ERR_UNSUPPORTED | 6 | backend lacks the operation | `paths.rs:20`; `wire.rs:8`; comment `ipc.rs:225` | Lua map lacks 6 |
| malformed | 0xFF | undecodable request | `dispatch.rs:46,242`; `dispatch_dirs.rs:118`; `trust.rs:89`; `trust/first.rs:147,191` | no name |
| test-local codes | 0xFC–0xFE | synthetic | `tests/srv-test/src/main.rs:54-55`; `tests/vfs-test/src/main.rs:69,80,90,92,93` | overlap net's bytes |
| ssh_trust | 0–7 | trust outcomes | `ipc.rs:351-369` | |
| save_refusal | 1–4 | why a save was refused | `ipc.rs:372-381` | |
| vfs_dir_page | COMPLETE 1, CHANGED 2, entry header 2 B, EntryKind 0–3 | directory page format | `ipc.rs:597-640` | |
| sizes | `IPC_BUF_SIZE` 4096, `VFS_GRANT_CHUNK` 64 KiB, `VFS_SAVE_CHUNK` 512 | | `ipc.rs:23,34,305` | |
| kernel client | `VfsRequest::Rename` | | `kernel/src/task/syscall/fs.rs:543,571` | |
| legacy | OP_GET_FILE 1 … OP_WRITE 4 | | `scripts/write-vfs-main.py:36-39` | stale generator script |

The handoff's citation `blk_router.rs:82` (receive on the block driver with no timeout) is confirmed.

## Block driver wire (`libs/types/src/block_wire.rs`)

| name | value | declared at | note |
|---|---|---|---|
| OP_READ (10 B), OP_WRITE (522 B), OP_FLUSH (2 B), OP_READ_RUN (16 B) | 0 / 1 / 2 / 3 (u16 LE) | `block_wire.rs:23-29,55`; used via the module in `ahci/dispatch.rs:27-29`, `nvme:23-25`, `virtio-blk:23-25`, `virtio-scsi:27-29`, `pvscsi:64-66`; VFS `blk_router/requests.rs:55,142,165` | single owner, no literal copies |
| BlockOutcome | Completed 0x00 / NotIssued 0x01 / other = Indeterminate | `block_wire.rs:136-153` | |
| limits | `MAX_RUN_SECTORS` 7, `SECTOR_SIZE` 512, `REPLY_SIZE` 513, `MAX_REPLY_SIZE` | `block_wire.rs:32-39` | |

## Input

| name | value | meaning | declared at | note |
|---|---|---|---|---|
| EV_KEY / EV_REL / EV_ABS | 0 / 1 / 2 | producer frame `[op][code 4][value 4]` | `input/src/main.rs:81` (KEY only; 1 and 2 are bare literals at `:577,582`); `drivers/hid/src/lib.rs:47-51`; `kernel/src/dev/ps2_keyboard.rs:299,395,402,416,454,459,465,470,478` (literals) | dup; the same names mean 1/2/3 in vmm_session |
| EV_ASCII | 0x04 | UART relay | `input/main.rs:84`; `kernel/src/dev/console_drv.rs:250` | dup |
| EV_DEVICE | 0x05; 14 B | HID event with device id | `input/main.rs:88`; `hid/lib.rs:53,62` | dup |
| OP_HID_DEVICE_REMOVED | 0x07 | `[op][dev u32]` | `ipc.rs:976`; checked at `input/main.rs:480` | **no sender found** |
| OP_SET_LEDS / led_bits | 0x03; bits 1/2/4 | input → producer | `ipc.rs:990,997-1000`; sent at `input/main.rs:313`; received at `dwc2-usb/main.rs:567` | 0x03 is also a producer opcode; kept apart only by sender |
| INPUT_EVENT_OPCODE | 0x10 + 64 B | event delivered to the focused cell | `api/src/services/input.rs:179,184`; `input/src/dispatcher.rs:33`; `compositor/src/main.rs:46` | dup ×3; **collides** with `GET_SCREEN_SIZE` 0x10, kept apart only by sender (`compositor/main.rs:158-160`) |
| EVDEV_* | 1 / 2 / 3 | Linux event types | `input/main.rs:91-93`; `input/src/mouse_state.rs:4-15` | duplicates the vmm_session codes |
| session wire | event_kind 1–10, mode bits | carried inside `SessionV1` | `libs/api/src/services/session.rs:10-60` | single owner |

## Compositor (`libs/api/src/services/display.rs`)

| name | value | declared at | note |
|---|---|---|---|
| compositor_ops | 0x01–0x13, 0x20–0x28, 0xFE | `display.rs:1135-1265` | single owner. `STAGE_GROUP_CONFIGURE` 0x25, `COMMIT_GROUP_CONFIGURE` 0x26 and `DUMP_FB` 0xFE have no handler and no user |
| compositor_events | 0xA0–0xA3 | `display.rs:1271-1277` | |
| wire sizes | 72 / 12 / 12 / 8 / 16 / 28 / 12 / 12 / 20 / 64 B | `display.rs:259-276,600-602` | |
| ViewOpResult / DisplayProtocolError | 0–8 / 1–7 | `display.rs:623-633,281-295` | |
| bool replies `\x00` / `\x01` | 0 / 1 | literals: `message_handler/grant.rs:62,102,106,113,151`; `message_handler/surface.rs:194,206` | no name |
| owner-exited | 0xE2 | see the supervisor table | |
| COMPOSITOR_ENDPOINT | 5 | `display.rs:1283` | hard-coded tid |

## Config, kms, ai, silo

| svc | name | value | declared at | note |
|---|---|---|---|---|
| config | CONFIG_ERR_DENIED | 0xFC | `ipc.rs:1059`; `config/src/rights.rs:54,121,148` | |
| config | ERR_MALFORMED | 0xFF | `config/src/main.rs:32` (private); `cellbox/src/config_client.rs:149` (test) | not in api |
| kms | frame | 128 B: header 16 B + payload 112 B; request `[ver,op,flags u16,req_id u32,len u16,rsv…]`; response `[ver,op,status,rsv,req_id,code u16,len…]` | `libs/types/src/kms.rs:30-34`; `frame/request.rs:60-70`; `frame/response.rs:10-20,62-75` | single owner |
| kms | KmsOpcode | 1–24 | `kms/model.rs:27-76` | test literal header at `ostd/src/clients/kms/ssh.rs:556` |
| kms | status / KmsErrorCode | Ok 0 / Error 1; 1–38 (u16) | `model.rs:81,169-224` | |
| ai | AiRequest / AiResponse | postcard 0..4; logical opcodes 0x0600–0x0604 are not on the wire | `ai-proto/src/lib.rs:77-85,235,294` | no byte-pin tests; `ai` reads no caller identity |
| silo | frame | 192 B, magic `DSLO`, version 1, payload at offset 24 | `libs/types/src/silo.rs:10,16-18` | |
| silo | OP_STATUS … OP_PROMOTE_ENROLLMENT | 1–6 | `silo/wire.rs:9-14` | |
| silo | status | ok 1, destroyed 3, error = `err + 1` with opcode 0 | `wire.rs:176-208`; `silo.rs:68-84` | **collision:** status 3 means both "destroyed" and Sequence error; only the opcode tells them apart |
| silo guest | COMMAND_* | 1–6, offsets 0/8/16/17/18/24 | `guests/silo-guest/src/layout.rs:12-29` | |
| silo guest | status bytes | Ready 1 / Sig 2 / Ack 3 / Absent 4 / Fault 0xFF | literals: `guests/silo-guest/src/main.rs:42-47`; `services/silo/src/guest.rs:75,100,127,152,173,194-195`; `layout.rs:66` | no named constant; 13 sites |

## net-broker and hypervisor vmm_session

| svc | name | value | declared at | note |
|---|---|---|---|---|
| net-broker | local ingress | request `[seq u64][len u16]` (10 B); reply `[0x7F][status][req_id 8][seq 8][len 2]` (20 B) | `net-broker/src/local_ingress.rs:3-8,76-81` | |
| net-broker | ReplyStatus | 0–3 (Success/Busy/Indeterminate/NotSupported) | `local_ingress.rs:11-15` | bench imports it (`c2c_broker_oracle_client/support.rs:8`) |
| net-broker | bench oracle ops | 0x01 / 0x02 / 0x03 / 0x7E / 0x81 | `bench_oracle/commands.rs:24-30` | 0x81 = `VMM_EVT_SCANOUT` on a different wire |
| net-broker | LookupRemote | 0x01; Found 0x00 / NotFound 0x01 / Err 0x02 | `routing.rs:38-42` | dead code (`routing.rs:5`) |
| net-broker | network formats | gossip 0x01/0x02, kms_dh tags 1/2, c2c v1 (112 B header), beacon `VCLS` v1 | `gossip.rs:33-34`; `kms_dh.rs:18-19`; `c2c_envelope.rs:7-8`; `beacon.rs:35-36` | not IPC |
| vmm | VMM_OP_* | 1–5 at byte 0 | `api/src/services/vmm_session.rs:20-24`; users `hypervisor/src/app_session_x86.rs:127,134`, `apps/chrome/src/guest.rs:8` | single owner |
| vmm | VMM_STATUS_* | 0–5 (u32) | `vmm_session.rs:26-31`; `app_session_x86.rs:62,64`; `apps/vm-runner/src/main.rs:158` | |
| vmm | VMM_EVT_* | 0x81–0x83, 20 B | `vmm_session/events.rs:13-17,86` | |
| vmm | frames | launch 44 B, launch reply 20 B, input 16 B, resize 16 B | `vmm_session.rs:72,137,179`; `events.rs:43` | |
| vmm | EV_* / REL_* / ABS_* / BTN_* | 1–3, 0/1/8, 0/1, 0x110–0x114 | `vmm_session.rs:34-58`; `hypervisor/src/virtio_input.rs:27-37` | dup |

## Summary

**Literal sites outside an owner module, about 350.** Each count comes from the greps cited in the rows above:

| Wire | Sites |
|---|---|
| NIC | 121 |
| supervisor | 48 |
| net Err (server 43 + client/tests 3) | 46 |
| VFS errors | 32 |
| TLS | 21 |
| input-producer | 21 |
| vmm/evdev | 20 |
| silo-guest | 13 |
| App envelope | 10 |
| compositor | 9 |
| net-broker (dead routing) | 4 |
| config | 2 |
| kms | 1 |
| ai, block | 0 |

**Collisions:**
1. Net 0xFF means both failure and TcpRecv EOF (`tcp.rs:105` vs about 30 other sites; read as EOF at `ostd/src/clients/net.rs:286`).
2. 0x10 means both `GET_SCREEN_SIZE` and `INPUT_EVENT_OPCODE`; only a check on the sender separates them.
3. `TLS_CLOSE_OP` 0x15 is the next postcard `NetRequest` index; appending a variant would break routing.
4. VFS error codes 1/2/4 mean something else under the generic `vierr_from_code`.
5. "Denied" is 3 in VFS, 0xFC in net and config, 0xFD in supervisor; 0xFD also means busy in net.
6. Supervisor `STATUS_*` and `KILL_STATUS_*` disagree on 0x01 and 0x02; 0xFF is both an error phase and an invalid-request result.
7. Silo status 3 means both "destroyed" and Sequence error.
8. `EV_KEY` is 0 on the input wire and 1 in vmm/evdev.
9. 0x03 is both `OP_SET_LEDS` and a producer opcode, and 0x04 is both `OP_POWER` and `KILL_REQUEST`; the sender and the envelope keep them apart.

**Proposed proto modules** (`libs/api/src/services/proto/`, values pinned to today's bytes):
- **`supervisor.rs`**: from `protocol.rs` plus phase 6 and 0xFF, `HotswapError`, the 0xE1/0xE2 relay and the kill opcodes. Switch `sci.rs`, `sys-tools`, bench and init over to it.
- **`app_envelope.rs`**: 0xAC with 0x00/0xF0/0xF1/0xF2/0xFF and the shutdown reasons. Used by ostd `app.rs`, kernel `authority.rs` and `gpu-scanout.rs`.
- **`net.rs`**: a `NetError` `#[repr(u8)]` enum with 0xFF Failure (documented as also TcpRecv EOF), 0xFE NotEstablished, 0xFD Busy, 0xFC Denied, 0xFB AddrInUse. Move `net_ready`, `REACTOR_WAKE` and `NET_READY_*` here.
- **`tls.rs`**: the opcodes, limits 495/501/4094 and the sentinels. Replaces `tls_wire.rs` constants, ostd `tls.rs`/`tls_stream.rs` and the ocel literals.
- **`nic.rs`**: OP 0–2, Status 0–2 and the reply framing for 11 drivers plus `interface.rs`.
- **`vfs.rs`**: `VfsError` with 1/2/3/4/6/0xFF, plus `ssh_trust`, `save_refusal` and the dir-page format. Replaces `paths.rs`, ostd `wire.rs` and the Lua map.
- **`config.rs`**: Denied 0xFC and Malformed 0xFF.
- **`compositor.rs`**: move `compositor_ops`/`compositor_events` here or re-export them from `display.rs`.
- **`input.rs`**: EV_* 0/1/2/4/5, 0x07, 0x03, the LED bits and 0x10, for the kernel, hid and dwc2.
- **`kms.rs`, `ai.rs`, `block.rs`, `silo.rs`, `vmm_session.rs`**: re-export the existing owners. Add a silo guest status enum, and point the evdev copies in hypervisor and input at the vmm constants.
- **net-broker**: keep `local_ingress`, or move it to `proto/net_broker.rs`; delete the dead `routing.rs` constants.

**Not found:**
- `libs/ostd/src/cluster.rs`, cited at `routing.rs:10`.
- `cells/services/net/src/poll_driver.rs`, cited at `ostd/src/tls.rs:28`.
- Any sender of `OP_HID_DEVICE_REMOVED`, `NET_READY_OPCODE` or an `[0xAC,0xFF]` shutdown, searched across `kernel/src`, `cells` and `libs/ostd`.
- `libs/api/src/services/mod.rs`: the module root is `libs/api/src/services.rs`, which the handoff's `services/{ipc.rs,mod}` should name instead.
- `const` declarations in `api/services/hotswap.rs`.