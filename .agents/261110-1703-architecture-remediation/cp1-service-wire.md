# CP1 — shared service runtime and wire modules (Phase 12)

**Date:** 2026-11-13 · **Base:** `f094e9df5` · **Handoff:** [request](s6-handoff-request.md), [approval](s6-handoff-approval.md) · **Census:** [wires](research/p12-census-wire.md), [loops](research/p12-census-loops.md), [VFS](research/p12-census-vfs.md)

## 1. Census

### Wires

There are 13 wires. Six already have one owner module (block, kms, silo, ai, vmm_session, compositor). About **350 literal sites** sit outside an owner module:

| Wire | Literal sites | Where they are |
|---|---|---|
| NIC | 121 | the same `OP_TX/RX/GETMAC` and status bytes in 11 drivers plus `net/interface.rs` |
| supervisor + `0xAC` envelope | 58 | `protocol.rs`, `sys-tools` hotswap/shutdown, the kernel (`acpi/sci.rs:40`, `authority.rs:252`, `gpu-scanout.rs`), bench |
| net errors | 46 | `Err(0xFF/0xFE/0xFD)` with no name |
| VFS errors | 32 | `paths.rs`, ostd `read_file/wire.rs`, Lua, httpd, init |
| TLS | 21 | `tls_wire.rs`, ostd `tls.rs`/`tls_stream.rs`, ocel |
| input producer | 21 | input, hid, the kernel's ps2/console |
| others | about 50 | vmm/evdev copies, silo-guest status bytes, compositor bool replies, config |

**Collisions** (all of them stay on the wire; §2 W1 names them):
- net `0xFF` means both failure and TcpRecv EOF;
- `0x10` means both `GET_SCREEN_SIZE` and `INPUT_EVENT_OPCODE`;
- `TLS_CLOSE_OP` `0x15` equals the next `NetRequest` postcard index, so a new `NetRequest` variant would break TLS routing;
- supervisor `STATUS_*` and `KILL_STATUS_*` disagree on 1 and 2;
- silo status 3 means both "destroyed" and a sequence error;
- "denied" is 3 in VFS, `0xFC` in net and config, and `0xFD` in supervisor; in net, `0xFD` also means busy.

### Loops

- **All 10 service loops are hand-rolled.** `service_entry!` has no user.
- **Heartbeat:** net, input and net-broker heartbeat; the other seven never do.
- **ai** reads no caller identity at all.
- **Owner death is handled three ways:**
  - VFS watches every request and fails closed;
  - compositor watches only on create and fails open;
  - net watches on first sight and fails open.

**The census changes the plan in two places:**
1. **No timed receive is also attested.** `RecvTimeout` uses `a3` for the timeout; `Recv`/`TryRecv` use `a3` for the attest flag. A blocking `serve` loop cannot have both a heartbeat and attested callers. Net and input work around this by polling `TryRecv` and parking on `WaitCompletion`/`SetTimer`. `AppContext` drops attestation whenever a heartbeat is armed (`app.rs:229`).
2. **Plan scope:** input and net-broker do not fit one loop, and neither is in the plan's Modify list. They stay hand-rolled:
   - input has raw producers, secret scrubbing and a session pump;
   - net-broker has multi-threaded roles with per-task heartbeats.

   Silo fits directly but is not in the plan; it stays hand-rolled in this phase.

### VFS block path

- **A frozen driver hangs VFS forever.** The masked `sys_recv(tid)` at `blk_router.rs:82` has no timeout and runs under `ROUTER` inside `GLOBAL_VFS`.
- **Init cannot see it:** `available()` only checks that the task is alive.
- **Unbounded reads hide errors:** FAT whole-file reads turn a device error into a short read (`backend_fat.rs:430`). If the receive is bounded without a typed error, today's hang becomes silent truncation. The bound and `FsError` therefore land in one step.

## 2. Proposed Law-1 deltas (`libs/api`, `libs/types`)

No wire byte moves, no syscall number is added or reused, and no allowlist bit is consumed.

| # | Item | Delta | Compatibility |
|---|---|---|---|
| W1 | Proto modules | New `libs/api/src/services/proto/{app_envelope,supervisor,net,tls,nic,vfs,config,input,compositor}.rs`. Each holds opcodes, frame lengths and a `#[repr(u8)]` error/status enum, all pinned to today's bytes by byte tests. `kms`, `ai`, `block`, `silo` and `vmm_session` are re-exported from their existing owners. Collisions are documented, never "fixed". `NetRequest` gets a test pinning its variant count below `0x15`; growing it needs a new TLS routing rule first. | Additive. Old items in `ipc.rs` and `display.rs` move into the modules, with one `pub use` path kept where the census shows out-of-crate users. |
| W2 | Timed attested receive | `Recv` `a3`: bit 0 stays `RECV_ATTEST_CALLER`; **bits 8–31 become `timeout_ticks`** (0 = untimed, today's behaviour). The result is `Timeout` as for `RecvTimeout`. Bits 1–7 stay ignored. 24 bits allow about 46 h, which also fits the 32-bit legacy targets. | Additive. Every in-tree caller passes 0 or 1 (to be checked by grep in step 3). The golden dump, the Recv doc and the headers change; the digest scripts are re-pinned in CP2. |
| W3 | VFS trust wire | `VfsRequest::{TrustLookup, TrustChallenge, TrustCommit}` (0x28–0x2a) become **tombstones** that answer `Err(6)` (Unsupported); removing them would shift `MkdirAt` 0x2b and every variant after it. `VfsResponse::{Trust, TrustChallenged}` are the last variants and are deleted. | An old cellbox gets Unsupported instead of an answer. |
| W4 | kms trust ops | Appended `KmsOpcode`: `BeginTrustQuery` 25, `TrustQueryChunk` 26, `CommitTrustQuery` 27, `CommitTrustChallenge` 28, `AbortTrustQuery` 29. They follow the import pattern, because host + key + session do not fit the 112-byte payload. Appended `KmsErrorCode`: `TrustHandleInvalid` 39, `TrustOrderInvalid` 40. The `ssh_trust` outcome bytes 0–7 stay the reply payload. | Additive (`libs/types`). |
| W5 | Input trust selector | `CellSession` selector 29 (trust prompt) is accepted from the live **kms** provider instead of VFS. Doc change in `cell_session.rs:150`. | The check moves; nothing is renumbered. |

## 3. Non-Law-1 design

### `ostd::service::serve`

`service.rs` becomes `service/mod.rs`; the loop lives in `service/serve.rs`.

```rust
pub struct ServeConfig { heartbeat_ticks: u64, watch: WatchPolicy }
pub enum WatchPolicy { None, OnOpcodes(fn(&[u8]) -> bool) /* fail-open */, Every /* fail-closed */ }
pub enum Wait { Block, Ticks(u64), Poll, Completion { sources: u32, ticks: u64 } }
pub struct Request<'a> { pub caller: CallerIdentity, pub payload: &'a [u8] } // borrowed, no Vec copy
pub trait Handler {
    fn on_request(&mut self, req: Request<'_>, cx: &mut Cx) -> Reply<'_>; // Now(bytes) | Deferred(token) | None
    fn on_owner_death(&mut self, dead: Released, cx: &mut Cx) {}
    fn on_unattributed(&mut self, sender: usize, payload: &[u8], cx: &mut Cx) {}
    fn on_turn(&mut self, cx: &mut Cx) {}
    fn on_idle(&mut self, cx: &mut Cx) -> Wait { Wait::Block }
}
```

**What `serve` owns:**
- the attested receive (W2 when a heartbeat is armed, waiting at most `H/2`) and parsing of the caller identity;
- capturing `Incoming` before dispatch, so nested masked receives in a handler cannot steal the reply token;
- the reply, sent only after `on_request` returns, so VFS can drop `GLOBAL_VFS` first;
- the owner table, stale-generation sweep and `WatchCellOwner` per `WatchPolicy`;
- the heartbeat, plus `cx.keepalive()` for long handlers.

**What each hook covers:**
- `on_unattributed`: the kernel power button (sender 0), the supervisor's `0xE2` relay and messages with no trailer that are not deaths.
- `on_turn`: compositor render and net pump.
- `Wait::Completion`: net's `NET_RX` park.
- `Deferred`: net DNS/ready/TLS and other deferred replies.

`service_entry!` and the unused `CellRuntime` service path are deleted. `AppContext`/`run_app!` stay for apps; the supervisor moves off them.

### Per-service plan

| Service | Watch | Idle | Notes |
|---|---|---|---|
| config | None | Block | Heartbeat only. |
| kms | None | Block | The silo exchange stays a nested bounded call. |
| ai | None | `Ticks(reap)` | Becomes attested; the session owner becomes the attested cell rather than the tid. The allowlist gains `Heartbeat`. |
| supervisor | None | Block | Power button and relay go through `on_unattributed`. Hotswap waits call `keepalive()`. **The power-settle receive no longer drops other messages:** it defers them. |
| compositor | `OnOpcodes(create)` | `Ticks(5 s)` | The resolution check moves from "after a message" to a real tick; render runs in `on_turn`. |
| vfs | `Every` | `Poll` while `idle_work`, else Block | Heartbeat `6_000` ticks. |
| net | `OnOpcodes(first sight)` | `Completion{NET_RX, poll_at}` | `NetState` struct, one no-op probe type for the oracles. Migrates last, with the x86 bench before/after. |

### VFS liveness and block path (step 4)

- **The block bound:** `Router::exchange` uses the exact-operation call (`ostd::ipc` bounded call, already in the VFS allowlist) with `BLOCK_REPLY_TICKS = 3_500`, i.e. the drivers' own 30 s plus 5 s.
  - Exact operations rule out a late reply being taken as the answer to another LBA, which a plain `RecvTimeout` would allow.
  - A timeout maps to `BlockOutcome::Indeterminate` for writes and to `FsError::Io` for reads. **A timed-out write is never retried.**
- **Stall latch:** after one timeout, the device route (tid, generation) is marked stalled. Requests to it fail at once until route discovery names a different provider. One request costs at most one bound, despite the FAT 3× sector retry.
- **`FsError`** (VFS-local `#[repr(u8)]`): `NotFound`, `NotADirectory`, `IsADirectory`, `DirectoryNotEmpty`, `AlreadyExists`, `InvalidPath`, `NoSpace`, `Unsupported`, `Io`, `Indeterminate`, `Unavailable`.
  - On the wire it maps to today's bytes: `Unsupported` → 6, all others → 1. **No new wire byte.**
  - The 22 `FsBackend` methods return `Result<_, FsError>`; `stat`/`observe` return `Result<Option<_>, FsError>`. `health`, `get_file_ptr`, `stage_path` and `publish_if` are unchanged.
- **Heartbeat:** `6_000` ticks, re-armed every turn and before each block exchange. A frozen driver costs one request at most 35 s, which is under 60 s.

### Trust store into kms (step 6)

- **Code:** a new ungated `kms/src/trust/` module takes the parser and lookup from `vfs/src/trust.rs` and challenge/commit from `trust/first.rs`.
- **Storage:** `/srv/cellos/kms/ssh_known_hosts`, written with the kms VFS client's observe/save path. The `/srv/.cellos-trust` access rules are deleted, and `scripts/phase09_trust_disk.py` seeds the new path.
- **Callers:** `ostd::clients::kms` gains the trust calls, cellbox `ssh.rs` switches to them, and `ostd::clients::vfs` trust wrappers are deleted.
- **Allowlist:** kms gains `IpcSubmit/Take/Wait/Cancel`, `GetTime` and `CellSessionV1` for the input prompt.
- **Ordering:** kms starts after input, so the upward call disappears.

### Literal removal (step 2)

Servers, ostd clients, `sys-tools`, the 11 NIC drivers, the kernel (`sci.rs`, `authority.rs`, `gpu-scanout.rs`, ps2/console input bytes), Lua and httpd import the proto modules.

A grep gate (`scripts/check-service-wire-literals.py`, run in CI) fails on an `OP_*`/status literal outside `proto/`. It uses an explicit allowlist for:
- driver-internal opcodes (GPU, SCSI, AML, dwc2 LAN shim);
- network formats that are not IPC;
- the bench's hostile frames.

## 4. Proof obligations (CP2 evidence)

1. **Byte tests:** every opcode, status and error value equals its pre-phase literal.
2. **Frozen block driver fixture (x86):** a test hook stops the driver from replying without killing it. VFS returns `Indeterminate`/`ERR_IO` within the bound, keeps serving in-memory requests and keeps heartbeating. It must fail on `f094e9df5`.
3. **W2:** a timed attested receive returns `Timeout` after the ticks and returns the attested trailer when a message arrives. `a3 = 1` behaves as before.
4. **ai attestation:** an unattested caller is refused, and an attested one is served.
5. **Trust:** the cellbox ssh first-contact fixture passes against kms; VFS trust ops return `Err(6)`.
6. **Net bench:** x86 bench before and after the net migration, with no regression beyond run-to-run noise.
7. **Gates and lanes:** the literal gate is green. x86_64 and riscv64 builds, service host tests, the x86 boot lane, `x86-c2c`, Tier 3 display and lifecycle all pass.
8. **ABI artifacts:** the golden dump changes only in the `Recv` doc row, and the headers are regenerated. The digest scripts are re-pinned and each re-pin is listed.

## 5. Order

One commit per step, split per service inside migration steps:

1. This CP1.
2. W1 proto modules + byte tests, literals switched, gate added.
3. W2 kernel + `serve`; config → kms → ai (ai attested); `service_entry!` deleted.
4. VFS bound + stall latch + `FsError` + heartbeat, with the fixture.
5. supervisor, compositor, vfs on `serve`.
6. W3–W5 trust store into kms.
7. net `NetState` + probe + `serve`, with the bench.
8. Specs 09/12/17 + CP2.

## 6. Risks and rollback

- **W2 widens `Recv`'s meaning.** A caller that passed garbage in bits 8–31 would now time out. The step-3 grep proves that no in-tree caller does; out-of-tree callers cannot be checked.
- **W3 is irreversible** for old cellbox binaries: trust ops return Unsupported.
- **A heartbeat makes hangs fatal:** a service that newly heartbeats is killed and restarted where today it hangs. Intervals sit well above every bounded wait, and `keepalive()` covers long handlers.
- **Net latency:** net migrates last and only behind the bench; if it regresses, net stays hand-rolled and this is recorded.
- **Rollback:** per-commit revert, except W3 once released.

## 7. Questions to the owner

1. Confirm W1–W5 as the Law-1 delta of Phase 12.
2. **W2:** add the timeout to `Recv`'s `a3` (recommended: no new opcode or bit). The alternative needs no ABI change: services disarm the heartbeat around a blocking receive, which loses attested `on_idle` ticks for ai and compositor.
3. **Scope:** input, net-broker and silo stay hand-rolled in Phase 12.
4. **Seeded trust stores:** no runtime migration from `/srv/.cellos-trust`; images are re-seeded at the new path by the provisioner.

## 8. Owner confirmation 1 — 2026-11-13

The owner confirmed W1–W5 as the Law-1 delta of Phase 12.

- **W2:** the timeout goes into `Recv` `a3` bits 8–31; no new opcode or allowlist bit.
- **Scope:** input, net-broker and silo stay hand-rolled in Phase 12.
- **Trust stores:** no runtime migration; the provisioner seeds `/srv/cellos/kms/ssh_known_hosts`.
- **Next:** steps 2–8 open, in the §5 order, under the request §3 file set.
