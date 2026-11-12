# Phase 12 census: VFS block path, FsBackend, SSH trust store, heartbeat (commit f094e9df5, read-only)

**Short answers:**
1. A frozen block driver hangs VFS forever. There is one masked `sys_recv` with no timeout, and it runs while both `ROUTER` and `GLOBAL_VFS` are held. The fix is to bound the exchange in the router with the exact-operation IPC API, at 3 500 ticks.
2. `FsBackend` has 22 methods across 8 implementations, and its errors are bool/Option/0/empty-Vec. FAT whole-file reads already turn a device error into a short read that looks like success. Bounding the receive without `FsError` would turn today's hang into silently truncated data.
3. The trust store moves cleanly into kms, because kms starts after input. Five new opcodes are needed, since the kms frame payload is only 112 bytes. The three `VfsRequest` trust variants must stay as tombstones, or every later variant's byte changes.
4. Yes, VFS would be killed if it armed a heartbeat. The kernel kills on deadline whatever the task is doing, and that includes the idle blocking receive. The "init watchdog" is actually the kernel scheduler sweep.

## 1. VFS block path

**Findings**

| # | Finding | Evidence |
|---|---|---|
| 1.1 | The one provider receive is a masked `sys_recv(tid, buf)` with no timeout. The plan's `:80` is off by two; the handoff's `:82` is right. | `cells/services/vfs/src/blk_router.rs:80-83` |
| 1.2 | Its single call site is `Router::exchange`. Every read, write, flush and read-run goes through it. | `blk_router/router.rs:158-177` (recv at `:170-171`); used by `blk_router/requests.rs:25,59,115` |
| 1.3 | `discovery.rs` has no send or receive. | grep of `blk_router/discovery.rs` returned nothing |
| 1.4 | The masked receive completes only on a reply or on the driver's death. A frozen driver produces neither. | `router.rs:154-157` |
| 1.5 | The receive runs under the `ROUTER` mutex. | `blk_router.rs:54,154,162,167,177` |
| 1.6 | `ROUTER` is taken inside the `GLOBAL_VFS` lock. VFS is a single loop, so the whole service stops. | `main.rs:219-224` |
| 1.7 | Init's `available()` checks only that the task is alive and registered, so a hung VFS still counts as available. | `cells/tools/init/src/service_table.rs:58-70` |
| 1.8 | VFS never heartbeats today, so the kernel never kills a hung VFS. | §4 |
| 1.9 | `Indeterminate` is produced in four ways. | see below |
| 1.10 | `Indeterminate` is consumed in four ways. | see below |
| 1.11 | Drivers already bound their own device waits at 30 s per command (`IO_BUDGET_MS`). AHCI bounds by poll count instead. `serve_read_run` issues up to 7 one-sector commands per run. | `nvme/src/deadline.rs:20`, `virtio-blk/src/wait.rs:8`, `virtio-scsi/src/wait.rs:8`, `pvscsi/src/irq.rs:61`, `ahci/src/controller.rs:144`, `libs/types/src/block_wire.rs:32,108-114` |
| 1.12 | All five drivers reply with `ostd::ipc::reply_to`, which uses `sys_send` when serving an operation. Plain Send is a valid reply to an exact-operation call. | `virtio-blk/src/main.rs:121`, `ahci/src/main.rs:234`, `nvme/src/main.rs:177`, `virtio-scsi/src/main.rs:173`, `pvscsi/src/dispatch.rs:49`; `libs/ostd/src/ipc/post.rs:109-116`; `libs/ostd/src/ipc.rs:295` |
| 1.13 | **Hazard if the fix is a plain `RecvTimeout`:** a late reply stays queued. The next masked receive from that tid would take it as the reply to a different LBA, and the main loop's wildcard receive could take it as a request. `RecvTimeout` is also not in the VFS allowlist (own bit 29). The exact-operation IPC syscalls already are. | `main.rs:77-119` (IpcSubmit/Take/Wait/Cancel, GetTime at `:107-112`); `libs/api/abi/syscalls.toml:783-786` |
| 1.14 | Exact-operation calls rule out late replies, and they tell apart "never accepted" from "may have run". | `libs/ostd/src/ipc.rs:39-50,365-415,453,780-781` |
| 1.15 | FAT retries each sector read 3 times, and retries a write while it is `NotIssued`. Without a latch, a frozen driver costs 3× the bound per sector read. | `block_stream.rs:19,80,175-206` |
| 1.16 | A CellosFS read failure becomes `FsError::IoError`. That is a storage fault, so it poisons `/srv` until VFS restarts (there is no path back to Ready). | `disk_cellosfs.rs:26-36`; `libs/cellos-fs/src/disk.rs:27-31`; `backend_cellosfs.rs:156-170,65` |

**Where `Indeterminate` comes from (1.9):**
- A missing reply in `exchange_status` (`blk_router/requests.rs:31-37`).
- A failed kernel-path write or flush (`requests.rs:137,162`).
- The driver's own byte 0x02, or any unknown byte (`block_wire.rs:147-153`).
- The router passes it on; `blk_write_sectors` stops at the first sector that is not completed (`blk_router.rs:184-199`).

**Where it is consumed (1.10):**
- `block_stream.rs:199-203`: never replayed.
- `disk_cellosfs.rs:45-56`: becomes `FsError::IoError`.
- `backend_cellosfs.rs:583,603`: becomes `SaveOutcome::Indeterminate`.
- Reads return only a bool (`requests.rs:42-69,99-123`), so a read can never surface `Indeterminate`.

**Proposal: where the bound goes and its value**

| Item | Proposal |
|---|---|
| Location | Replace `BlockSys::send` + `recv` (`blk_router.rs:43-46`) with one bounded `call`. The live implementation uses `PendingCall::submit` followed by `wait_and_take`. `Router::exchange` (`router.rs:158-177`) is the only consumer; update the test fake at `blk_router/tests.rs:64-90`. |
| Mapping | Submit refused (Busy, PeerGone or InvalidRequest), `PreDispatchTimeout` or `Cancelled` → `Exchange::Refused` → `NotIssued`. Terminal `PeerGone`, terminal `Indeterminate`, or budget expired (cancel, then take) → `Exchange::Missing` → `Indeterminate`, and forget the provider. Reply → `Replied`. The extra route lookup after the reply (`router.rs:171`) is no longer needed, because death can no longer pose as a reply. |
| Value | `BLOCK_REPLY_TICKS = 3_500` (35 s at 10 ms per tick). That is the drivers' 30 s budget plus 5 s, so only a driver that has already overrun its own deadline is cut off. |
| Stall latch (recommended) | After a timeout, mark the device stalled for that (tid, generation). Fail requests to it at once (`NotIssued`, read = false) until a fresh route lookup names a different provider. This caps one request at one bound despite the FAT 3× retry and lets VFS keep serving in-memory requests. |
| Writes | Timeout → `Indeterminate`, logged and never replayed. This matches the existing rules at `requests.rs:148-153` and `block_stream.rs:202-203`. |

## 2. FsBackend (`cells/services/vfs/src/backend.rs:84-191`)

**Methods and today's error convention**

| Method | Signature (abbreviated) | Failure today |
|---|---|---|
| `health` | `&self -> BackendHealth` | enum; only CellosFS overrides it (`backend_cellosfs.rs:232`) |
| `get_file_ptr` | `&self, path -> Option<(usize, usize)>` | `None` = disk backend |
| `list` | `&self, path, out -> usize` | `0` = missing **or** error |
| `list_page` | `-> Option<ListPage>` | `None` = 5 merged causes (`backend.rs:98-103`) |
| `stat` | `-> Option<(u64, bool)>` | `None` = absent **or** I/O error |
| `file_size` | `-> u64` | `0` = absent |
| `read_to_vec` | `-> Vec<u8>` | empty = absent or error; FAT returns a **short read** on an I/O error (`backend_fat.rs:424-431`); CellosFS uses `unwrap_or_default` (`:305-320`) |
| `write`, `append`, `mkdir`, `rmdir`, `unlink`, `rmdir_recursive` | `&mut self, … -> bool` | `false` |
| `read_at` | `-> usize` (default 0) | `0` = unsupported, EOF or error |
| `write_at`, `sync`, `rename_no_replace` | `-> bool` (default false) | `false` = unsupported **or** failed |
| `stage_path` | `-> Option<String>` | `None` = unsupported |
| `observe` | `-> Option<Observation>` | `None` = error **or** unsupported |
| `stage_create`, `stage_write` | `-> bool` | `false` |
| `publish_if` | `-> SaveOutcome` | already typed |

**Implementations (8):**
- Production: `backend_bin_overlay.rs:38`, `backend_bootfs.rs:153`, `backend_cellosfs.rs:231`, `backend_fat.rs:322`, `backend_ramfs.rs:174`, `access/stub.rs:7`.
- Test fakes: `dispatch_grants.rs:241`, `manager/read_path_tests.rs:778`.

CellosFS already works internally with `cellos_fs::FsError`, which has 12 variants (`libs/cellos-fs/src/disk.rs:7-21`). It collapses them to bool/Option at `backend_cellosfs.rs:148-207`.

**How callers interpret errors today**

| Caller layer | Evidence |
|---|---|
| `VfsManager` wrappers turn "no backend" into `false`/`0`/default | `manager.rs:230-345` |
| `healthy()` checked again after an operation (26 sites in dispatch/grant/paths) | `manager.rs:98-104`; e.g. `dispatch.rs:133-135`, `grant_write.rs:71-73` |
| Short-read detection by comparing length with `stat` size | `trust.rs:95-97`, `trust/first.rs:113-115`, `grant_write.rs:74-76` |
| `bool` → `ERR_IO` (1); wire codes are 1/2/3/4/6 (5 unused) | `paths.rs:12-20`; client side `libs/ostd/src/clients/vfs/read_file/wire.rs:4-18` |
| Clients cannot tell "not found" from "I/O error" and work around it | `cells/services/kms/src/ssh/persist/blob_io.rs:118-123`, `kms/src/storage/runtime.rs:36-41` |

Call-site sizing: about 65 manager-level calls outside the backends. Largest files: `save.rs` (11), `dispatch_grants.rs` (10), `grant_read.rs` (5), `dispatch_file_handles.rs` (5), `trust/first.rs` (4), `paths.rs` (4), `dispatch_dirs.rs` (4), `dispatch.rs` (4), `dir_ops.rs` (4).

**Proposal: `FsError`** (a VFS-local `#[repr(u8)]` enum)

| Variant | Source | Wire byte (pinned to today's) |
|---|---|---|
| `NotFound` | absent path | 1 |
| `NotADirectory`, `IsADirectory`, `DirectoryNotEmpty`, `AlreadyExists`, `InvalidPath` | cellos-fs refusals, FAT structure rules | 1 |
| `NoSpace` | `cellos_fs::NoSpace` | 1 |
| `Unsupported` | today's default-false trait methods | 6 |
| `Io` | the device reported a failure, or a read timed out | 1 |
| `Indeterminate` | `BlockOutcome::Indeterminate` on a mutation | 1 |
| `Unavailable` | backend poisoned, unmounted or no backend | 1 |

Scope:
- Change every bool, Option, 0 or empty-Vec method to `Result<_, FsError>`. `stat` and `observe` become `Result<Option<…>, FsError>`.
- Keep `health`, `get_file_ptr`, `stage_path` and `publish_if` as they are.
- Splitting `NotFound`, or adding an `Indeterminate` byte on the wire, is an append-only CP1 decision.
- **Do it in the same step as the bound:** the FAT `Err(_) => break` (`backend_fat.rs:430`) must become `Err(FsError::Io)`.

## 3. SSH trust store

**What it is today**

| Item | Today | Evidence |
|---|---|---|
| Stored data | Text file `cellos-ssh-trust v1`: one line per host/port with algorithm, key and provenance. Limits: 1 024 entries, 4 KiB per key, 1 MiB total. | `trust.rs:9-21,31-37` |
| Path | `/srv/.cellos-trust/ssh_known_hosts`, on CellosFS `/srv`. No caller may read it; only the VFS uses it. | `trust.rs:31`; `access/rules.rs:44-50,118-121` |
| Seeded by | Offline provisioner script | `scripts/phase09_trust_disk.py:8,67-68` |
| Entry point | `trust::handle` from VFS dispatch | `dispatch.rs:80`; `trust.rs:49-104` |
| State kept in VFS | `trust_challenges` (at most 4 in total, 1 per caller); lost when VFS restarts | `manager.rs:57,91`; `first.rs:47-48,68-73` |
| Writes | Conditional publish through the VFS-internal save path | `first.rs:306`; `save.rs:267-279` |
| Upward call into input | `register_trust_challenge` and `collect_trust_decision` | `first.rs:33-40,194,231`; `libs/ostd/src/session.rs:547-608` |
| Input's admission check | Selector 29 is accepted only from the live registered VFS | `input/src/session_trust.rs:173-187`; `libs/api/src/abi/cell_session.rs:150` |
| Wire | `VfsRequest::TrustLookup`, `TrustChallenge`, `TrustCommit`, with replies `VfsResponse::Trust` and `TrustChallenged` | `libs/api/src/services/ipc.rs:248-277,581-589` |
| Callers | Only cellbox's ssh client (lookup, challenge, commit polling) | `cells/tools/cellbox/src/ssh.rs:667,953,977`; client wrappers `libs/ostd/src/clients/vfs.rs:296-365` |
| Not callers | sshd, kms and kms-ssh-admin do not use it | grep of `cells/services/sshd/src` and `cells/tools/kms-ssh-admin/src` returned nothing |
| Other upward calls | None. `prepared_cwd` calls the kernel only. The plan's claim that this is the only inversion holds. | `prepared_cwd.rs:135,155,247,268` |
| Boot order | config → ai → input → net → … → silo → kms, so a kms → input call is not an inversion | `config/services.toml:56-111` |

**What kms has today**

| Item | Evidence |
|---|---|
| Fixed 128-byte frame with 112-byte payload | `libs/types/src/kms.rs:32-34` |
| Opcodes 1-24 (SSH custody uses 15-24); errors end at 38 | `libs/types/src/kms/model.rs:27-76,170-225` |
| SSH custody only under the dev feature `ssh-dev-custody` | `kms/Cargo.toml:20-25`; `kms/src/dispatch.rs:5-6,30-31` |
| Persists through the VFS client under `/srv/cellos/kms` (only live KMS admitted) | `kms/src/storage/runtime.rs:8-72`; `storage/record.rs:10-12`; `ssh/persist/blob_io.rs:115-116`; `vfs/src/access/kms.rs:3-12,47-55` |
| Allowlist: Send, SendGather, Recv, RecvTimeout, Log, LookupServiceBound, VfsMutate | `kms/src/main.rs:13-21` |
| Loop: attested receive, no heartbeat | `kms/src/main.rs:41-64` |

**Proposal: move plan into kms**

| Step | Change |
|---|---|
| Code | New **ungated** `kms/src/trust/` module. Move `parse`/`lookup`/base64/`canonical_host` from `trust.rs:107-236` and challenge/commit/persist from `first.rs:51-313`. Trust lookup is production behaviour, so it must not sit behind the dev feature. |
| Storage | `/srv/cellos/kms/ssh_known_hosts`, inside kms's existing protected namespace. Publish with the existing client calls `observe` / `save_stage` / `save_commit` (`libs/ostd/src/clients/vfs.rs:225-290`) in place of the VFS-internal publish. Delete the `/srv/.cellos-trust` rules (`rules.rs:44-50,118-121`). Update the provisioner (`phase09_trust_disk.py:67-68`). Migrating an existing seeded store is a CP1 decision. |
| New opcodes (append) | Host (≤ 253 B) + key + session do not fit 112 B, so stage the query in chunks, following the import pattern at `model.rs:56-64`: `BeginTrustQuery = 25` {kind lookup/challenge, port, lengths, SHA-256} → handle; `TrustQueryChunk = 26`; `CommitTrustQuery = 27` → `ssh_trust` code, or confirmation + challenge refs (32 B); `CommitTrustChallenge = 28` {challenge ref} → code; `AbortTrustQuery = 29`. |
| New errors (append) | `TrustHandleInvalid = 39`, `TrustOrderInvalid = 40`. Reuse `CallerUnattested`, `PermissionDenied`, `Busy`, `PersistFailed`, `InvalidRequest`. The `ssh_trust` byte values (`ipc.rs:351-367`) stay as the reply payload. |
| kms allowlist | Add IpcSubmit, IpcTake, IpcWait, IpcCancel, GetTime, CellSessionV1 (the set VFS lists at `main.rs:107-113`) for the session calls. |
| Input | Admission at `session_trust.rs:183`: `service::VFS` → `service::KMS`. Update the comments in `cell_session.rs:150` and `session.rs:542-545`. |
| Clients | `ostd::clients::kms` gains trust_lookup/challenge/commit. Delete `clients/vfs.rs:294-365`. Switch cellbox `ssh.rs:667,953,977` and `ssh/tests.rs`. |
| VFS wire | **Keep `TrustLookup`, `TrustChallenge` and `TrustCommit` as tombstones** that answer `Err(6)`. They are variants 0x28-0x2a; `MkdirAt` is 0x2b (`ipc.rs:1263-1270`), so removing them would shift every later byte. `VfsResponse::Trust` and `TrustChallenged` are the last response variants (`ipc.rs:581-589`) and can be deleted. |
| Delete | `vfs/src/trust.rs`, `trust/` (`first.rs`, `tests.rs`), `manager.rs:57,91`, `dispatch.rs:80`, VFS allowlist `main.rs:102-113` (only if nothing else in VFS needs it; `prepared_cwd` uses `ostd::session::kernel_request`, so check this before removing CellSessionV1). |

## 4. VFS heartbeat

**Findings**

| # | Finding | Evidence |
|---|---|---|
| 4.1 | VFS lists `Heartbeat` in its allowlist but never calls it; there is no `sys_heartbeat` anywhere in vfs src. | `main.rs:83` |
| 4.2 | The plan's "init watchdog" is the kernel scheduler sweep. It kills any task whose deadline has passed, whatever its state, and logs the park point. Only an explicit Heartbeat re-arms it. Init only allowlists Heartbeat. | `kernel/src/sched/scheduler.rs:2574-2625`; `kernel/src/task/syscall/ipc.rs:1176-1191`; `libs/ostd/src/app.rs:359-366`; `cells/tools/init/src/main.rs:38` |
| 4.3 | **An idle VFS would be killed.** The idle loop parks in an attested receive with no timeout. | `main.rs:172-176` |
| 4.4 | There is no timed attested receive: `RecvTimeout` passes the timeout where the attestation flag would go, and `AppContext` drops attestation on timed waits. | `ostd/src/syscall.rs:1608-1617` vs `:1543-1555`; `kernel/src/task/syscall/decode.rs:890-895`; `app.rs:229` |
| 4.5 | **A frozen driver would get VFS killed** after one interval, because the receive is unbounded. | `blk_router.rs:82` |
| 4.6 | **Long whole-file reads under `GLOBAL_VFS`:** `ReadAsync` reads the whole file synchronously. FAT reads in 4 KiB loops, 3 attempts per sector. bootfs reads 64 KiB chunks. The 16 MiB heap caps the file size. VFS is killed if one request runs longer than the interval. | `dispatch.rs:107-138`; `dispatch_dirs.rs:211`; `dispatch_file_handles.rs:105`; `grant_read.rs:73,105`; `backend_fat.rs:409-432`; `block_stream.rs:19`; `backend_bootfs.rs:226-240`; `main.rs:127` |
| 4.7 | Startup mount and route discovery (up to 500 ticks) run before the loop. With no deadline at spawn, that is safe today. | `main.rs:143`; `blk_router.rs:59,132-147`; `kernel/src/task/tcb.rs:648` |
| 4.8 | A kill means a restart (VFS restart policy is Always). Handles, pending reads and trust challenges are lost, and a CellosFS poison clears. | `service_table.rs:42-55` |

**Proposal**

| Item | Proposal |
|---|---|
| Interval | `VFS_HEARTBEAT_TICKS = 6_000` (60 s). That is longer than one block bound (3 500) plus margin. It is the same order as input's 5 000 (`input/src/main.rs:97`). |
| Re-arm points | (1) Every main-loop turn before dispatch (`main.rs:162`). (2) In `Router::exchange` before each bounded call. Every wait is then at most 35 s, so a real deadlock (e.g. a `GLOBAL_VFS` self-lock) still trips at 60 s. |
| Idle | Without a kernel ABI change: disarm with `sys_heartbeat(0)` just before the blocking attested receive at `main.rs:175`, and re-arm after it. The `try_recv` path at `:173` re-arms every turn. A parked idle receive is not a hang. Alternative (Phase 08 syscall table, Law-1 checkpoint): add an attested `RecvTimeout`, so `serve` can wait at most `H/2` like `app.rs:219-224`. This is the CP1 choice for the shared `serve` design. |
| Fixture criterion | With the router bound plus the stall latch, a frozen driver costs one request at most 35 s, which is under 60 s. VFS returns `Indeterminate`/`ERR_IO` and keeps heartbeating. |

## Not found
- No driver-side change is needed for exact-operation replies: all five drivers already use `reply_to` (row 1.12).
- No init-side heartbeat consumer exists (searched `cells/tools/init/src`).
- No sshd or kms-ssh-admin use of the trust store.
- No VFS error byte 5 is in use.