# CP1 — one IPC operation model (Phase 11)

**Date:** 2026-11-11 · **Slice:** S5b (architecture remediation) · **Law 1:** confirmation 1 requested · **Findings:** review H-6, H-14, Medium IPC/kernel shape · **Census:** [services](research/p11-census-services.md), [drivers/apps/tools/tests](research/p11-census-drivers-apps.md), [libs/kernel](research/p11-census-libs-kernel.md) (measured on `0f81a6597`)

## 1. Census

### Blocking sends (`Send`, opcode 0)

| Scope | REPLY | EVENT | REQUEST | UNCLEAR | Total |
|---|---|---|---|---|---|
| `cells/services` (direct) | 48 | 4 | 2 | 0 | 54 |
| `cells/services` via reply wrappers (`send_typed`, `send_error`, …) | 107 | — | — | — | 107 |
| `cells/drivers`, `cells/apps`, `cells/demos`, `cells/tools` | 9 | 6 | 55 | 2 | 72 |
| `cells/tests` | 20 | 26 | 36 | 3 | 85 |
| `libs/` (ostd SDK, posix) | 1 | 6 | 17 | 1 | 25 |

### Hidden behind wrappers

- `AppContext::send`: 11 REPLY sites.
- recovery-demo `common::send`: 16 sites across all three classes.

### Lookups

- **Tid-only `LookupService` (206):** about 150 call sites. Each scope counts direct calls; wrapper callers are listed separately.
  - 130 lines in 85 cell files: 40 in services, 55 non-test elsewhere, 36 in tests (the scope counts sum to 131 because one line was counted in two scopes).
  - 19 in `libs/`.
  - 8 more via the recovery-demo wrapper, 4 via ocel's `Transport::lookup`.
- **Bound `LookupServiceBound` (429):** 8 direct cell call sites (3 in services, 1 recovery-demo, 4 in tests), plus 12 via `sender_is_live_provider`, 3 via `ServiceRef` in tests, and the SDK's `LocalEndpoint::bind`.

### Task state readers

- GetProcs (30), GetProcs2 (239) and TelemetrySnapshot (430) share one encoder (`kernel/src/task/syscall/misc.rs:487`):
  - 0 = Ready, 1 = Running;
  - 2 = every one of the 13 blocked states;
  - 3 = Terminated or Retiring.
- 16 readers outside the kernel, 11 of which decode the value.

### What the census changes in the plan

1. **Replies:** 155 service reply sites (48 direct plus 107 via wrappers) already complete exact operations through the kernel's implicit bridge (`async_ipc::reply_current_in_sched`): a `Send` to the caller currently being served becomes that operation's reply. The non-receiving-client hazard therefore sits only in:
   - (a) REPLY sends to a caller that used a raw `Send` (no token), which park the service in `Sending`;
   - (b) EVENT sends a service makes while serving (supervisor hot-swap snapshot/restore, the 0xE2 relay to the compositor, compositor `window.rs:151`).
2. **Possible live misdelivery:** `compositor/src/message_handler/window.rs:151` sends a state event to the requester *before* the reply at `:153`. For a caller that used `IpcSubmit`, the bridge turns that event into the reply and quarantines the real one. Step 3 fixes it either way.
3. **Kernel `Rename` (255)** is a blocking legacy VFS client:
   - bare-tid lookup;
   - `Sending` with no deadline;
   - a yield-and-poll receive with `deadline: None`;
   - every non-Ok reply mapped to `PermissionDenied`.
4. **`fast_ipc`'s fast path is dead:** it has no production callers. What remains is "who is VFS" (12 production sites, 9 `is_registered_vfs_cell`), and those gate:
   - owner-death watches;
   - caller-context and grant-lease retention;
   - syscalls 244, 247 and 441.
5. **Unused SDK request surface:** `dispatch::run_service`, `LocalEndpoint::call` and `LocalEndpoint::new` have no production callers, and `RemoteEndpoint::call` always returns `NotSupported`. The typed outcome therefore lands on `ServiceRef`, the surface that is used.
6. **Stale-tid authorisation:**
   - block drivers (ahci, nvme, pvscsi, virtio-blk, virtio-scsi) accept a VFS tid cached for life;
   - hypervisor, httpd, sshd and net-tools cache provider tids once;
   - kms, compositor, input, silo and vfs authorise senders by tid-only lookup.

   Bound lookup closes all of these.

## 2. Proposed Law-1 deltas (`libs/api`, `libs/types`)

No syscall number is renumbered or reused, no new opcode is added, and no allowlist bit is consumed (62 of 64 bits are used).

| # | Item | Delta | Compatibility |
|---|---|---|---|
| D1 | `RegisterService` (205) | `a2` becomes `flags`: `RETAINS_OUTER_CALLER`, `OWNER_WATCH`. Only the namespace owner (SpawnCap holder) may set them. Unknown bits are rejected. | Old callers pass `a2 = 0` and get today's behaviour, except for the VFS and `OWNER_WATCH_SERVICES` rows, which init now registers with flags. |
| D2 | `WaitIrq` (234) | No ABI change. The kernel implements it as a wait on completion source `IRQ`, and the doc of `source::IRQ` widens to cover legacy lines held via `PcieDriverCap`/`PlatformCap`, not only MSI lines. | Same wake semantics; lost-wakeup rule kept. |
| D3 | `WaitForEvent` (217), `IpcWait` (258) | No ABI change. Kernel-side compatibility wrappers over `WaitCompletion` sources `NET_RX` / `IPC`. | Same results. |
| D4 | Retired opcodes | Marked `reserved = true` in `syscalls.toml`: `Call` 2, `Reply` 3, `Yield` 104 (legacy aliases), `Exec` 6, `AudioPlay` 218, `NetTx` 310, `NetRx` 311 (fixed-result arms). `ViSyscall` keeps no variant for them. The kernel decodes them to `UnknownSyscall`, the error every other unassigned number already gets. The ostd base allowlist drops `Reply`. | Nothing in `libs/` or `cells/` calls them. `NetTx` stops silently dropping frames. |
| D5 | Tid-only lookup | Retire `LookupService` (206, bit 37) **after** every caller is migrated (step 7). It is marked reserved, and bit 37 stays reserved, not reused. | About 150 call sites move to `LookupServiceBound` (429). Out-of-tree consumers cannot be checked; this is stated, and accepted only with the owner's confirmation. |
| D6 | `ProcessInfo` state | No encoding change: 0/1/2/3 stay. The doc fixes the meaning of 3 to "Terminated or Retiring". | `sshd ps` is fixed to the documented encoding. |
| D7 | Spec 17 | §1 names the exact operation as the request/reply model and `Send` as event-only. §6 describes the bounded SDK default. §9 records the amendment. | Docs. |

`MkDir` raw 110, `Create` raw 111 and `FileOp` 107 op=1 have fixed results but are still reached by raw numbers. They stay as they are and are listed for a later CP.

## 3. SDK and kernel changes (not Law-1)

### SDK `send`

`ostd::syscall::sys_send` stays, as the raw primitive. Over it:
- `ostd::ipc::post(tid, bytes)` (new name) uses `TrySend` and returns `Busy` instead of parking.
- `ostd::ipc::post_blocking` is the one allowlisted blocking path. It is used only where the census shows the receiver never waits in receive: input registration and HID events (dwc2 `usb_hid.rs:975`, xhci `input.rs:38`).
- `clippy.toml` `disallowed-methods` flags direct `sys_send` outside `libs/ostd` and `cells/tests`.

### Replies

Every service REPLY site goes through `ostd::ipc::reply_current(bytes)`. It takes the captured token (`ipc::current()` at receive time) and calls `IpcReply`. It returns `NoOperation` when the request came by raw `Send`, and the service then *posts* the reply, never blocks.
- The existing wrappers (`send_typed`, `send_error`, `send_ready`, …) change inside, so their 107 callers are untouched.
- The two places that already choose a reply path at runtime (dwc2 `reply_nic`, edu-holder `answer`) become the template.

### Requests

- Typed outcome `ostd::service::CallOutcome<T> = Reply(T) | Refused(RefusedReason) | Uncertain(UncertainReason)`:
  - `Refused`: no live binding, stale binding, busy at admission, encode error;
  - `Uncertain`: peer gone after acceptance, timeout after dispatch, cancelled.
- `ServiceRef::call` and `LocalEndpoint::call` return it, and `ViError::IO` collapsing ends.
- `LocalEndpoint::new` (tid-only, tests only) is removed, and `dispatch::run_service` (no callers) is deleted.
- REQUEST sites migrate to `ServiceRef::call` per directory: `libs/ostd` clients (`tls`, `KmsClient`, `display/ipc`, posix) first, then services (kms silo, vfs blk router), drivers, apps and tools (net-tools 37 sites), then tests.
- Tests that exist to exercise raw `Send` keep it, by an explicit allow:
  - `smp.rs:106`;
  - `async_lifecycle.rs:1174`;
  - `local_service_lifecycle.rs:209`;
  - `tier2-rpc-driver` 129/140.

### Kernel

- **`Rename`:** submits an exact operation to the bound VFS with a deadline (owner decision: it stays in the kernel). VFS death or expiry returns `Uncertain` → `Io`, and refusal → `NotFound`/`PermissionDenied` as VFS answered.
- **Registration flags:** `service_registry` rows carry them, and `is_registered_vfs_cell` and `OWNER_WATCH_SERVICES` become flag reads. `kernel/src/fast_ipc.rs`, `libs/ostd/src/fast_ipc.rs` and `cells/services/vfs/src/fast_handler.rs` are deleted.
- **Parked states:**
  - `Sleeping`, `Sending`, `Recv`, `PipeRead`/`PipeWrite`, `FutexWait`, `Waiting`, `WaitEvent`, `WaitCompletion`, `WaitIpc` and `WaitIrq` become `Blocked { wait: WaitKind, deadline: Option<u64> }`.
  - `Polling` and the dummy waker are deleted.
  - Phase 10's deadline queue and event-waiter list then key on `deadline` and `WaitKind`.
- **Dead `Task` fields:** `leases`, `next_lease_id`, `grant_table`, `next_grant_id`, `pending_future`, `cluster_mode` and `cluster_id` are deleted, and so are the unreachable internal variants `Lend`, `BorrowRead`, `BorrowWrite`, `Grant` and `Map`. `allowed_drivers` and `pku_key` are only copied to child threads and stay; they are listed for a later CP.

## 4. Proof obligations (CP2 evidence)

1. **Non-receiving client fixture (x86):** a client submits by raw `Send` to the supervisor, config and compositor and never receives. Each service then serves a second client. It must fail on `0f81a6597` and pass after step 3.
2. **`Rename` fixture:** VFS is killed mid-`Rename`, and the caller gets an error within the deadline. It must fail before step 3.
3. **Bridge ordering:** a fixture shows the compositor CONFIGURE_ACK reply arrives as the reply, not as the state event.
4. **Unchanged lanes:** the C2C evidence lanes (saturation/deadline/caller-death, restart, peer pressure) pass unchanged. So do the full local lane set on 3 arches, `x86-c2c` and the two-node lane.
5. **Golden dump:** `syscalls.golden.json` changes only in the D4/D5 rows, which become reserved. The headers are regenerated. The three Law-1 digest scripts are re-pinned, and each re-pin is listed.
6. **Negative controls:**
   - a retired opcode returns `UnknownSyscall`;
   - `RegisterService` with an unknown flag bit, or with flags from a non-owner, is refused;
   - `post` to a non-receiving tid returns `Busy` and does not park.

## 5. Order

Each step is one or more commits and leaves the tree shippable:

1. This CP1.
2. `CallOutcome` and the `ServiceRef` surface; `ostd` clients migrate.
3. `reply_current` in the reply wrappers, the services' raw `Send` removed, the compositor ordering fixed, `Rename` fixed. This adds the 3 fixtures.
4. D1 flags, `fast_ipc` deleted.
5. D2/D3 waits, parked states collapsed.
6. D4 opcodes retired, dead fields and variants removed.
7. Bound lookup everywhere, then D5.
8. Spec 17 (D7) and CP2.

## 6. Risks and rollback

- **D5 is the irreversible part.** Once 206 is reserved, an out-of-tree binary that calls it fails. Rollback for everything before D5 is per-commit revert.
- **The `post` default changes failure timing:** a caller that relied on parking now sees `Busy`. Every non-test send site is classified in the census, and the allowlisted blocking path covers the receivers that never wait.
- **The parked-state collapse touches every wake path.** It lands after Phase 10's rank asserts, which catch lock-order changes, and is checked on the 2-CPU lanes on all three arches.
- **Volume:** about 400 send sites and 190 lookup sites. Migration commits are mechanical per directory and are reviewed separately from logic commits.

## 7. Questions to the owner

1. Confirm D1–D7 as the Law-1 delta of Phase 11.
2. **D5:** retire `LookupService` (206) after migration (irreversible), or keep it as a deprecated alias for one more release?
3. **Blocking path:** confirm that `post_blocking` is limited to input registration and HID events (the census lists 2 sites).

## 8. Owner confirmation 1 — 2026-11-11

The owner confirmed D1–D7 as the Law-1 delta of Phase 11.

- **D5:** `LookupService` (206) is retired after every in-tree caller is migrated. This is irreversible; out-of-tree callers cannot be checked.
- **Blocking path:** `post_blocking` is limited to input registration and HID events.
- **Next:** steps 2–8 open, in the §5 order, under the request §3 file set.
