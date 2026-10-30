# OSD CP1: owner-death reclaim beyond VFS and Net (compositor, KMS, kernel send-to-tombstone gap) (final text; decisions in section 12; documents only; nothing applied)

Written 2026-10-30 at HEAD `952d84c8a` (`git status --short`: only `.agents/261110-1200-cellbox-followups/` untracked). Documents only: no source, `Cargo.toml`/`Cargo.lock`, script, lock, plan, image, key, network, build, test or QEMU action was made to produce it. `file:line` citations were read in this session by me, or by a read-only sub-audit whose claim I then spot-checked; the tag **[sub-audit]** marks a fact I did not re-read myself; `[INFERENCE]` marks anything not observed in a file or command output; `OPEN (decider)` marks a decision for the named owner. Form follows `net-owner-death-cp1.md` (its section 11 is the lock-packet form used in section 9 here) and `k4b-cp1.md`.

Inputs bound (SHA-256): `net-owner-death-cp1.md` `c40fdf98ebaee4de89bb13acc64a2bc5a03ec4a2d83bae6cb059729f61224965`; `net-od-cp2-dossier.md` `d47ecfc9d1cab241f917d22bcc7ba365b0354e31968f5362ea5e151600066dc0`; `cp2-dossier-part5.md` `39e8aaae308749ee689d73a8dac7fd15b8bdc94407d868e0ed08b16e573511af`; `k4b-cp1.md` `e44e301751b6fe96f4ea242f2f50e3c73e64fdd0a73ea86d6a5ff38a12b13fba`; `k5-cp1.md` `6ff24401d391681b59a21610f6b8e6cb845c48caa21f182f69c960853e4d3d3e`; `k6-cp1-draft.md` `9c97e579b336a9f316ad60f75786c1794bd0e2a27ef5cdb34990641a53702fdd`; `cleanup/ci.yml.md` `fdbc8e1040a3db52d7f517cd5e2fadf4fb294c86993086e1524ff810037ea14f`; `.agents/cell-contract-0024-locks.md` `1f674e010ab0715bf7b66c554e5d125b2d35b999fb7b1668441abda7227274c0`; `.agents/plan-portfolio.md` `d1e89ceb1c0433627fa68b4122e2127ff6fee6b9e6a734e669503c347a42ddd8`; `config/services.toml` `188d4d1966a91ad0bb4fb0336548a27969f86f349bd2b05041704ed8d5deecdb`.

## 0. Summary

| Item | Finding |
|---|---|
| Question | Which services besides VFS and Net hold per-client state that leaks when the client cell is killed, how bad is it, must the kernel grant `may_watch_owner_deaths` (`scheduler.rs:258-266`) grow, and what is worth doing |
| Handled today | **VFS** (static grant, lease quarantine, `handle_unattributed_owner_death`, `cells/services/vfs/src/main.rs:177-214`) and **Net** (NET-OD, landed `d42e70d6a`, observed O7 + 21/21 in T3-D3). Nothing else holds the watch; the kernel grants it only to those two (`scheduler.rs:258-266`) |
| **Correction to `net-owner-death-cp1.md` section 13** | that table said compositor and KMS state is "keyed by generation ... bounded, no leak of authority" and input "lingers [INFERENCE]". Read in code: the **compositor keys everything by raw thread tid and has no client-death path at all** (only a hypervisor-specific relay); **input** reclaims by polling within one idle turn; **KMS** leaks only *secret retention* (one staged passphrase, one staged key blob) and never wedges on a dead owner |
| Real leaks, ranked | **(1) compositor**: a killed GUI client leaves its windows, views, focus target and registered-grant frames for the life of the compositor; 32 dead owners exhaust the surface table for everyone (P1). **(2) kernel send-to-tombstone gap** (not owner state: a blocking reply `sys_send` that lands on a retiring dead root parks the service forever; hits KMS, compositor, ai, supervisor, block drivers) (P2, 3-line kernel fix). **(3) KMS**: staged passphrase and import blob outlive the killed client until the next Begin (P2/P3, `DEV_POSTURE` builds only). **(4) AI**: 4 sessions + KV memory for <= 30 s (P3, a quota is the better fix). Everything else: nothing to reclaim, or already reclaimed, or not a server (YAGNI list, section 5) |
| Kernel grant | extend `may_watch_owner_deaths` to an **explicit const table of registry-bound service ids** (`NET`, `COMPOSITOR`, later `KMS`); keep the attested-serving-context rule. Rejected: "any registered service" (D-NOD-1 B), a new CapSet right, per-cell static handlers. **No libs/api, libs/types, libs/ostd, manifest, allowlist, policy or ABI change** |
| `RecvTimeout` | **delivers** queued owner deaths at mask 0 (`syscall.rs:4935-4940`, `take_queued_death` `:2794-2841`); the NET-OD text and the brief for this task implied only `Recv`/`TryRecv`. Every receive primitive a service uses (`Recv`, `RecvTimeout`, `TryRecv`, attested or not) delivers; a service parked in `sys_wait_completion` is **not woken** (`scheduler.rs:1716-1722` wakes only `Recv{mask 0}`) |
| K5 blocking `sys_send` risk | splits in two. **Killed client**: a dead *removed* target releases the parked service with an error (`scheduler.rs:1616-1625`), so a kill does not wedge KMS; **except** the quiescence window where the dead root is a `Retiring` tombstone that `ipc_send_kernel` still accepts (section 4.3, new finding, [INFERENCE] until run). **Live non-receiving client** (hostile or timed-out): unbounded, no kernel timeout, heartbeat is opt-in and KMS does not opt in; unchanged by any slice here, options in D-OSD-8 |
| Recommended slices | **OSD-1** kernel table + compositor consumer (P1, do now); **OSD-3** kernel `ipc_send*` rejects `Retiring`/`Terminated` targets (P2, do now, tiny, independent); **OSD-2** KMS consumer + `KMS` table entry (P2, after OSD-1; **adopt only if the owner wants the invariant "no staged custody secret outlives its owner"**, otherwise YAGNI); **OSD-4** bounded reply for live non-receiving clients (design-only, not now); everything else YAGNI. Final CP1 text for OSD-1, OSD-3, OSD-2 in sections 6-8; one lock packet in section 9 |
| Proof ceiling | host tests + bare-metal release builds. **No QEMU proof is available or requested**: the GUI lane states "the Terminal-Cell-kill path (no kill route on this image)" (`scripts/qemu-phase04-gui-test.sh:77`) and no K8 scenario kills a client mid-import (`grep` over the K7/K8/K5 CP1s). A boot proof is a separate lane CP1 (section 10) |
| Law-1 | none triggered in any slice: `libs/api/` and `libs/types/` byte-identical (section 11) |
| Effort | OSD-1 M (kernel ~15 + compositor ~150 production lines, ~350 test lines); OSD-3 S (~10 + ~60); OSD-2 M (~170 + ~350) [INFERENCE from NET-OD sizing, not measured] |

## 1. Scope and non-scope

**In:** the audit of every service and driver cell for client-owned state; the kernel permission-table generalisation; three consumer/kernel slices with exact closure, tests, budgets and one lock packet; the YAGNI list with evidence.

**Out:** any ABI change (syscalls 244-248, `CellOwner`, trailer, opcodes); VFS/Net behaviour (landed); the NET-OD follow-ups of its section 5.7 (graceful-deadline silent removal, legacy `TcpClose`, TLS handshake-in-flight): those belong to `reap-and-rst-cp1.md` (sibling document); a bounded-send primitive (D-OSD-8, Law-1 if kernel); the block-driver/supervisor findings of section 5.3 (reported, not slices); promotion/re-promotion of canonical bytes; any edit by this document.

**Serves:** the general rule of `phase-09-ssh-client.md` item 5 ("close resources on cancellation or consumer death") and NET-OD section 13 ("other services ... would need the same"). Not an Intel C2C outcome (same basis as K2/K3/K5/NET-OD; the owner is asked to confirm, section 9).

## 2. Mechanism facts (kernel side; all re-read at HEAD unless tagged)

| Fact | Evidence |
|---|---|
| Grant today: VFS cell **or** the Active registry-bound `NET` provider (cell + generation match); caller must also have just received from exactly `(cell_id, generation)`; live generation required | `scheduler.rs:258-266` (`may_watch_owner_deaths`), `:677-702` (`watch_live_cell_owner`) |
| `WatchCellOwner`/`CancelCellOwnerWatch`/`Resolve*` need **no** allowlist bit (`SyscallSet::EMPTY.permits` is true) | `libs/api/src/abi/syscall_tests.rs:508-519`; a service that declares `declare_syscalls!` need not add them (documentation only) |
| Watches are not deduplicated or capped per watcher: each successful call inserts a new token and queues one record per watch. A consumer MUST watch once per principal and cancel when done | `scheduler.rs:694-701` (insert), `:704-712` (cancel) [also **sub-audit**] |
| Delivery: `Recv` mask 0, `RecvTimeout` mask 0 and (since NET-OD) `TryRecv` mask 0 deliver the first queued owner death before any queued message: first 8 bytes = exit reason, return value = dead **root** tid, trailer unattributable (all zero). Masked receives never do. A parked `Recv{mask 0}` is woken at root exit; `WaitCompletion` is not | `syscall.rs:4559-4601` (`Recv`), `:2794-2841` + `:4935-4940` (`RecvTimeout`), `peek_owner_death_for_try_recv` (`TryRecv`), `tcb.rs:897-899` (mask rule), `scheduler.rs:1716-1722` (wake) |
| A **plain** `sys_recv` cannot tell a death record from a request (no trailer). Every consumer must switch to `sys_recv_attested` and read "no trailer" as "death or dead sender" | `libs/ostd/src/syscall.rs:1434-1447`; trailer is the **last 32 bytes** of the buffer (`caller_identity.rs:98`) |
| Death is delivered ahead of the dead client's queued messages. **Plain `sys_send` messages already queued in a service mailbox by a sender that then dies are not purged**: they are later received with the dead sender's stamped identity but an all-zero trailer; only async-wire messages (`async_op != 0`) are dropped at exit | `async_ipc.rs:483-495` and `task.rs:2362,2720-2740` **[sub-audit]**; trailer zero: `syscall.rs:2895-2923,3366-3383` |
| Reuse: tids and generations are never reused; **CellId is reused** (lowest free slot after retirement), so state must key by `(cell_id, generation)` or by tid, never by CellId alone | `task_id_selftest.rs:156`, `tcb.rs:585-594`, `cell_quota.rs:207-221` **[sub-audit]** |
| A killed cell root stays in `tasks` as a `Retiring` tombstone until cross-hart quiescence; `exit_task` sweeps `Sending{target==tid}` senders **once, at exit** | `scheduler.rs:1494-1516,1612-1625`, `:1830-1915` |
| `ipc_send`/`ipc_send_kernel` accept any target that is `contains_key` in `tasks`; no `Retiring`/`Terminated` check | `task.rs:2316,2385` (read), `grep Retiring kernel/src/task.rs` hits only a comment at `:1466` |
| Blocking `sys_send` to a **live** target not in `Recv` parks the sender in `Sending` with no kernel timeout; `reply_current` (async-op clients) never blocks; a target itself `Sending` to the caller gets the message queued and the caller returns (cycle rule) | `task.rs:2396-2434`, `syscall.rs:4504`, `async_ipc.rs:268-288`; heartbeat is opt-in (`scheduler.rs:2185-2229`, `heartbeat_deadline`) and `cells/services/kms/src/main.rs` has no heartbeat call (grep) |
| Services are registry-bound by `init` (`Registration::Init` default): `NET 2, INPUT 3, CONFIG 4, COMPOSITOR 5, SILO 6, NET_BROKER 8, SUPERVISOR 11, KMS 13, AI 15`, HYPERVISOR 14; `lookup_bound` is Active-only, non-zero cell/generation, <= 32 entries | `service_registry.rs:143-150`, `config/services.toml:38-104` (`kms` `:89-93`, `compositor` `:74-78`), `cells/tools/init/src/service_table.rs:88-95` **[sub-audit]** |

## 3. Service audit (every service and driver cell)

Severity scale: **S1** unbounded or exclusive-grant wedge for other clients; **S2** bounded resource exhaustion or DoS of other clients until restart; **S3** secret/content retention in the service's own memory; **S4** cosmetic or none. "Worth" = P1 do now, P2 do when the table exists, P3 only with a stronger reason, YAGNI.

| Service (id) | Identity / key | Per-client state | Reclaim today | Leak if client killed | Sev | Receive loop | Worth |
|---|---|---|---|---|---|---|---|
| **VFS** (1) | attested trailer; `(cell, gen)` | leases, handles, cwd preparations | owner watch (static grant) | handled | - | `Recv`/`TryRecv` attested | done |
| **Net** (2) | attested; `SocketOwner{cell,gen}` | sockets, DNS, TLS | NET-OD watch + abort | handled (O7) | - | `TryRecv` | done |
| **compositor** (5) | **raw `sys_recv(0)` tid only** (`main.rs:106`); `SurfaceState.owner`/`View.owner_tid` = creating **thread** tid (`surface_table/state.rs:31`, `frame.rs:37`) | `SurfaceTable` <= 32 surfaces (no per-owner cap), `FrameRegistry` <= 64 frames x 32 views, z-order, pointer focus/capture, up to 3 registered-grant ids per surface (frames **owned by the compositor after the client died**: kernel transfers a live grantee's grant instead of freeing, `syscall.rs:1292-1310` **[sub-audit]**) | cooperative `DESTROY_SURFACE`/`DESTROY_VIEW`; **only** non-cooperative path is `cleanup_owner` triggered by opcode `0xE2` from the supervisor, which init relays **only when `/bin/hypervisor` dies** (`init/src/supervisor.rs:37-38,124-131`, `supervisor/src/main.rs:103-113`, `message_handler.rs:62-71`; all read) | ghost windows; close button cannot remove them (`try_send` to dead tid fails); 32 dead owners exhaust the table for all clients (`CREATE_SURFACE` returns cap 0); grant frames pinned until compositor exit (<= 16 MiB per grant); last frame stays on screen | **S2** (permanent until compositor restart), content retention S3 | `sys_recv(0)` blocking; replies by blocking `sys_send` (`surface.rs:32,172,181`, `grant.rs:28-72`) | **P1** (OSD-1) |
| **KMS** (13) | attested; `Principal{cell,gen}`; transfers bound to `(handle, principal, tid)` | **import**: slot + 64 KiB staging arena holding the staged key blob (plain or wrapped, `ssh/import.rs:22-27,84-105`); **unlock**: slot + staged passphrase `secret` (`ssh.rs:149-150`, `ssh/unlock.rs`); 4-entry replay-floor LRU (`ssh/floor.rs`); persisted identity (not owner state) | **replaced by the next Begin of any principal** ("so a dead client cannot wedge unlock", `ssh/unlock.rs:51-53`, `ssh/import.rs:132-134`); unlock slot consumed by the sign it was staged for (`take_staged_secret`); delete wipes both | staged passphrase and/or import blob remain in KMS RAM until the next Begin; **no wedge, no authority**: only the dead principal can use them (principal+tid checks, `ssh.rs:317-319`) and it never returns (generation never reused) | **S3** only; `DEV_POSTURE` builds only (`ssh-dev-custody`) | `sys_recv_attested(0)` blocking; reply `ipc::reply` if async op else **blocking `sys_send`** (`main.rs:44-52`) | **P2/P3** (OSD-2, owner-gated) |
| input (3) | SessionV1: attested, `Principal`; legacy focus: raw tid | sessions <= 64, frontends, outcomes, fences, trust challenges, raw/paste rings (secret-bearing, wiped on drop) | **polling**: `Sessions::maintain()` at the top of every main-loop turn re-queries the kernel and `close()`s stale principals (`main.rs:141`, `session.rs:1087-1101`, read) | none for SessionV1 (<= one idle turn); legacy `Dispatcher.focused` tid left until the next `SetFocus` (keys dropped, throttled log) **[sub-audit]** | S4 | `sys_try_recv_attested(0)`; **hazard**: a death with reason 0 begins `0x00` and would decode as `SetFocus` (`handle_typed_request`) if a watch were ever added without an intercept | **YAGNI** |
| ai (15) | raw tid, plain `sys_recv(0)` (`main.rs:123-127`) | 4 sessions `{request_id, owner, last_activity}` + KV cache in 16 MiB heap, no per-owner quota | idle reaper 30 s, runs only when a request arrives (`main.rs:91-96,216-238`) | up to 4 slots + prompt bytes for <= 30 s (and until the next request); `Busy` for others | S2 bounded, S3 | blocking `sys_recv(0)`; reply `sys_send` non-blocking for async clients | **P3** (a per-owner session cap is cheaper) |
| net-broker (8) | attested; `RequestKey{id,tid,cell,gen}` | fixed arrays: requests[16], replies[16], inflight[32], stale[64]; no secrets (bench commands) | self-draining: worker completes regardless; reply pump gives up after 32 attempts (`reply_pump.rs`) **[sub-audit]** | <= 4+4 entries for a few turns | S4 | blocking attested; replies never block | **YAGNI** |
| config (4) | attested, gates only Set/Delete | one global store, not owner-scoped | n/a | nothing | - | blocking attested | **YAGNI** |
| httpd, sshd | none: not services to cells; clients are remote TCP peers; httpd is a Net/VFS/AI client | per-connection, private | own deadlines | wrong direction (their own death is Net/VFS/AI's concern) | - | no receive loop | **YAGNI** |
| hypervisor (14) | none | none: it owns its VMs; the kernel reaps VMs per owner tid at exit (`task.rs:1868`) | kernel | nothing | - | masked `RecvTimeout` only | **YAGNI** |
| silo (6, dev aarch64) | attested; `KmsBinding` (only the live KMS) | protocol counters, one pending enrollment scalar in the guest | replaced by the next KMS generation; KMS `reconcile_pending_cleanup` | pending key stays if KMS dies mid-enrollment (1 slot) | S3 | attested | **YAGNI** |
| supervisor (11) | raw tid + process-name check | none | n/a | nothing; but replies with blocking `sys_send` even to unauthorised senders | S1 for the live-client wedge only | `sys_recv(0)` | **YAGNI** (see 5.3) |
| placement-controller, workload-router, budget-authority, platform, power, disk | none | none / stubs / no cell | n/a | nothing | - | no receive loop | **YAGNI** |
| ocel-quickjs, ocel-pdf, ocel-js | raw tid | one exclusive owner slot each | init `NotifyOnExit` retires the engine when no lease remains (`activation.rs:94-130`); quickjs/pdf also a 60 s idle lease; **ocel-js has no lease** (`main.rs:53` `handle`, not `handle_at`) and is not packaged (`scripts/gen-disk-ci.sh:164-169`) | quickjs/pdf: `Busy` <= 60 s; ocel-js: permanent `Busy` | S2 (js only, unpackaged) | `sys_recv(0)` | **YAGNI** |
| virtio-blk, nvme, ahci | raw sender tid, reply routing only | none per client | n/a | nothing | - | `sys_recv(0)`, blocking reply | **YAGNI** (see 5.3) |
| virtio-gpu, gpu, hid, xhci, serial, ... drivers | no client sessions (xhci is an input producer) | none | n/a | none | - | n/a | **YAGNI** |
| audio, TLS outside Net | no audio service exists (`AudioPlay` only logs, `syscall.rs:7289-7293`); TLS state is inside Net (covered) | n/a | n/a | n/a | - | n/a | n/a |

Compositor row: the loop, the surface/view ownership keys, the only reclaim path and the whole `0xE2` relay chain (init -> supervisor -> compositor) I read myself; the **consequence** claims (close button fails because `control.rs:23-27` `try_send`s a dead tid, 32-cap exhaustion returning cap 0, kernel transfer of a live grantee's grant to the compositor at `syscall.rs:1292-1310`, last frame left on screen) are **[sub-audit]** and each is a direct reading of the cited lines, not an observed boot. Everything tagged "read" above I read myself; rows that rest only on the sub-audit (net-broker, config, httpd, sshd, hypervisor, silo, supervisor, stubs, ocel-*, block/GPU/HID drivers, audio/TLS, the kernel cleanup table of section 2) are **[sub-audit]**.

## 4. KMS in detail (import sessions, unlock state, the K5 `sys_send` risk)

### 4.1 What a killed `kms-ssh-admin` or `cellbox ssh` leaves in KMS

| State | Where | Contents | Dead owner can still use it? | Reclaimed by |
|---|---|---|---|---|
| Import slot `Receiving`/`Complete` + arena | `ssh.rs:146-147`, `ssh/import.rs:22-27` | up to 64 KiB of the staged OpenSSH blob (plain keys are plaintext in the arena) | no: `Transfer::owned_by` needs `(handle, principal, tid)` (`ssh.rs:317-319`); a new generation is a new principal | next `import_begin` by another principal (`import.rs:132-134`), commit/abort by the owner (never), `delete_key` |
| Unlock slot `Collecting`/`Staged` + `secret` | `ssh.rs:148-150`, `ssh/unlock.rs` | the staged passphrase | no (same check; `take_staged_secret` requires the same principal and key) | next `unlock_begin` by anyone (`unlock.rs:51-53`), the sign it was staged for, delete |
| Replay floors | `ssh/floor.rs` | 4 `(principal, floor)` entries, LRU | no | LRU eviction. A dead entry costs only a slot; eviction of a *live* principal's floor resets it to 0 (O-4), so dead entries slightly raise that risk |
| `failures` counter | `ssh.rs:153-154` | global per key | n/a | not owner state |
| Persisted identity (T1) | `ssh/persist.rs` | wrapped key | n/a | not owner state, correct to survive |

Timeline of the realistic windows: `cellbox ssh` stages the passphrase immediately before its sign request (two back-to-back calls, [INFERENCE: client flow read at the K6 text level, not traced in `cellbox` this session]), so a Ctrl-C leaves a staged passphrase only if it lands between those two calls (sub-millisecond to a few ms); a kill *during* the sign is harmless (KMS finishes the sign, which consumes and wipes the slot, then replies to a gone tid). A kill of the admin cell mid-import (chunks read from VFS, seconds) is the wider window. **Severity S3, probability low, `DEV_POSTURE` builds only** (the SSH leaf is compiled only with the opt-in `ssh-dev-custody`, excluded from production-like builds by `build.rs`, `lib.rs` and `scripts/check-ssh-dev-persist-exclusion.py`).

No wedge exists on the owner side: nothing in KMS blocks waiting for a client to continue a transfer. This is the K5 statement "a dead client cannot wedge unlock" (`k5-cp1.md` section 9.1), and it holds in code.

### 4.2 What a KMS watch would buy

Prompt (instead of next-Begin) zeroization of the staged passphrase and blob when the owner dies. It would **not** change authority (nothing the dead owner could do), not free any bounded resource (the slots are single and reused), and not protect against a KMS memory disclosure from another path. It is a hygiene invariant: "no staged custody secret outlives its owner". That is why OSD-2 is owner-gated (D-OSD-2).

No zero-kernel alternative exists: KMS has no timer authority (`declare_syscalls![Send, Recv, RecvTimeout, Log, LookupService, VfsMutate]`, `main.rs:13`; `GetTime`/`SetTimer` absent), and `RecvTimeout` has no attested form (`libs/ostd/src/syscall.rs:1499`), so an idle-deadline sweep would need either a syscall-set change or an ABI extension. Replacing the staged slot on any *other* admitted opcode would change the single-slot semantics K5 approved.

### 4.3 The K5 blocking `sys_send` risk, split

KMS replies with `ostd::ipc::reply` when the request arrived as an async operation, else a **blocking** `sys_send(sender, ...)` (`main.rs:44-52`). All current KMS callers (`KmsClient::call` / `call_bounded`, `libs/ostd/src/clients/kms.rs:181-299`) use raw `sys_send` + `sys_recv`, i.e. the blocking path.

| Case | Behaviour | Status |
|---|---|---|
| **Client killed while KMS works** (the owner-death case) | after the dead root is removed from `tasks`, `sys_send` returns `InvalidCommand` (`TargetGone`, `syscall.rs:4525`) and KMS continues. If KMS was already parked in `Sending{target}` when the target exited, `exit_task` wakes it with an error (`scheduler.rs:1616-1625`) | safe |
| **Client killed, reply lands in the `Retiring` window** | `exit_task` already ran its `Sending` sweep; the root tombstone stays in `tasks` until quiescence; `ipc_send_kernel` only tests `contains_key` (`task.rs:2385`), so the message is queued into the dead mailbox and KMS parks in `Sending{dead}`; nothing wakes it (quiescence does not sweep senders) [**sub-audit** read of `take_quiescent_root_retirements`, confirmed by my read of `:1830-1915` that it only moves tombstones to zombies; **[INFERENCE]**: never exercised]. The same window also catches the **error reply to a zero-trailer leftover message** of the dead client (section 2, row 5): KMS answers `CallerUnattested` to a dead tid | **new finding**, fix = OSD-3; window is the quiescence interval (short) but the consequence is a permanent stall of every KMS opcode incl. TLS relay signing and the supervisor-driven enrollment |
| **Live client that is not in `Recv`** (K6 timed-out client; or a hostile cell that sends and never receives) | KMS parks in `Sending{client}` until the client exits or receives; no timeout; heartbeat not armed. K6 mitigates only the honest client (poison, one `try_recv` drain, exit: `kms.rs:288-299`) | **unchanged by OSD-1..3**; any cell that can `Send` to KMS can stall it [INFERENCE: who may `LookupService(KMS)`/`Send` was not audited]. Design options in D-OSD-8 |

## 5. Decomposition

### 5.1 Table

| Slice | What | Prio | Kernel | Why now / why not |
|---|---|---|---|---|
| **OSD-1** | kernel `OWNER_WATCH_SERVICES = [NET, COMPOSITOR]` + compositor consumer (attested receive, one watch per cell on first `CREATE_SURFACE`/`CREATE_VIEW`, death arm calling the existing `cleanup_owner` per tracked tid) | **P1** | `scheduler.rs`, `syscall.rs` (tests), `tcb.rs` (doc) | only service where a normal client kill leaves user-visible and permanent damage; `cleanup_owner` already exists and is already proven for the hypervisor path |
| **OSD-3** | `ipc_send`/`ipc_send_kernel` refuse `Retiring`/`Terminated` targets (`TargetGone`) | **P2** | `task.rs`, `ipc_wire_tests.rs` (tests) | protects every blocking-reply service from a permanent stall; independent of OSD-1/2; 10 lines |
| **OSD-2** | `KMS` table entry + KMS consumer (owner table, wipe of the dead owner's import/unlock slots, reconcile-cancel) | **P2/P3, owner-gated** | `scheduler.rs`, `syscall.rs` (tests) | cheap once OSD-1's table exists; S3 only, low probability; **if the owner prefers minimum scope: YAGNI**, nothing else depends on it |
| OSD-4 | bounded reply to live non-receiving clients | design only | maybe (Law-1) | needs a protocol or ABI decision; D-OSD-8 |
| AI quota | per-owner session cap in `ai` | P3, separate | none | cheaper and stronger than a watch (also stops a *live* client hoarding the 4 slots) |
| Everything else | see section 3 | YAGNI | - | no per-client state, already reclaimed, or not a server |

### 5.2 Order and independence

OSD-1 and OSD-3 are independent (different kernel files) and can land in either order; OSD-2 needs OSD-1's table (one more array element and test row). WIP rule: one implementation slice at a time (as K-series and NET-OD): OSD-1, then OSD-3, then OSD-2 if adopted.

### 5.3 Incidental findings handed to the owner (not slices; all **[sub-audit]**, [INFERENCE] until Main confirms policy)

1. **Block drivers** (`virtio-blk`, `nvme`, `ahci`) accept any sender and serve raw `DrvRequest` sector read/write with no `CallerIdentity` or tid check; I saw no kernel ACL on sending to the `BLOCK_DRIVER` service and `LookupService` is open; launch-profile allowlists and `Send` gating were **not** checked. If true, any cell with `Send` could read/write raw sectors. Needs a separate security review, not an owner-death slice.
2. **Supervisor** replies with blocking `sys_send` even to unauthorised callers (`REJECTED_CALLER`), reachable by any live cell (live-client wedge of section 4.3).
3. **ocel-js** has no idle lease and wedges on owner death; unpackaged.

## 6. Slice OSD-1 final CP1: kernel service table + compositor reclaim of a dead client's windows

### 6.1 Defect

Killing a GUI client (crash, `cellbox` kill route, hypervisor guest cell, any Ctrl-C) leaves its surfaces, views, z-order entries, input-focus target and registered-grant frames in the compositor. `cleanup_owner` (`message_handler.rs:110-141`) is only reachable from the supervisor's `0xE2` relay, which `init` sends only for `/bin/hypervisor`. Evidence: sections 3 and 2 (all compositor facts read; the `0xE2` chain read end to end).

### 6.2 Kernel design

1. `scheduler.rs`: replace the single-service clause by an explicit table.
   ```
   /// Registry-bound services whose provider cell may subscribe to the root death of the principal it is serving (plus the registered VFS cell). Explicit on purpose: being a registered service is not the right.
   pub(crate) const OWNER_WATCH_SERVICES: [u16; 2] = [service::NET, service::COMPOSITOR];
   fn may_watch_owner_deaths(watcher: &Task) -> bool { vfs || OWNER_WATCH_SERVICES.iter().any(|&id| lookup_bound(id).is_some_and(|(_, c, g)| c == watcher.cell_id.0 && g == watcher.cell_generation)) }
   ```
   Everything else of `watch_live_cell_owner` is unchanged (serving-context equality, live-generation resolve, token). Lock order SCHEDULER -> REGISTRY unchanged. A watcher registered as one service cannot use another service's identity (each id is looked up and compared to the watcher's own cell+generation).
2. `tcb.rs`: update the two comments that say "VFS, NET" (`:893` region, `owner_death_matches_receive_mask` doc) to "the registered VFS cell and the `OWNER_WATCH_SERVICES` providers"; doc only (D-NOD-8 precedent).
3. `syscall.rs`: **tests only** (production code unchanged: delivery already covers every primitive, section 2). The existing `net_owner_watch_grant_matrix` (`:10664`) is generalised to iterate the table: granted for each table service with its own identity; refused for every other id (`CONFIG, INPUT, KMS, VFS, SILO, AI, NET_BROKER, SUPERVISOR` while KMS is not yet in the table), for a NET-bound identity presented as COMPOSITOR and vice versa, Paused, re-registered, stale generation, wrong serving context, dead client; plus `owner_watch_services_are_an_explicit_table` asserting the exact array (a tripwire against silent widening). Fixtures `net_watch_scheduler`, `net_watch`, `assert_net_watch` (`:10587-10659`) are renamed/reused with a service parameter.

No `libs/api`, `libs/types`, `libs/ostd`, manifest, allowlist (the compositor has no `declare_syscalls!`, i.e. all bits allowed, and the owner-watch calls need none), policy, launch-edge or `kernel/src/main.rs` change.

### 6.3 Compositor design

**Receive.** `main.rs:97-106`: `buf` becomes `[0u8; api::ipc::IPC_BUF_SIZE]` (4096) and `sys_recv(0, ..)` becomes `sys_recv_attested(0, ..)`. The 32-byte trailer then lands at the end of the 4 KiB buffer, so the 512-byte payload window (`&buf[..512]`, passed to `handle_message`/`handle_input_event` as `&[u8; 512]` via `try_from`) is never clobbered (a 512-byte buffer would overwrite `WRITE_PIXELS` payload bytes 480..511, **sub-audit** trap). `buf.fill(0)` per message stays (4 KiB memset, negligible; required so a death record's unwritten tail reads as "no trailer").

**Classification (pure, host-testable).** `identity = CallerIdentity::from_recv_buf(&buf).filter(|c| c.sender_tid == sender as u64)` (the same rule input uses, `session.rs` **sub-audit**). Then:
- `Some(identity)`: serve as today (input-service events by `sender == input.input_tid`, everything else through `handle_message`), after the watch admission below.
- `None` and `sender` equals the root tid of a watched cell: **owner death** of that cell (also covers a dead client's last zero-trailer message): release it (below).
- `None` otherwise: **dropped** (D-OSD-5 A). Today such a message would be served on a raw tid; after the change a zero-trailer leftover of a killed client cannot recreate a ghost surface after cleanup (the kernel delivers the death first and the leftovers after it, section 2). The repo doc itself says an absent trailer "MUST be treated as unknown caller" (`caller_identity.rs:30-32`).

**Watch admission.** New module `cells/services/compositor/src/owner_watch.rs` (syscall-free; a two-method `WatchSys` trait is the host-test seam; `main.rs` implements it over `ostd::syscall::{sys_watch_cell_owner, sys_cancel_cell_owner_watch}`; same split as NET-OD's `reclaim_owner_state`, which exists because ostd syscalls on the host target are real Linux syscalls):
- state: `Vec<Watched{cell_id, generation, root_tid, token, tids: Vec<usize>}>`, <= `MAX_WATCHED_CELLS = 64` entries, <= `MAX_TIDS_PER_CELL = 96` tids per cell (96 = 32 surfaces + 64 views, the table capacities; [INFERENCE] unreachable);
- `admit(identity, sender, creates_state, sys)`: for `CREATE_SURFACE` and `CREATE_VIEW` only (D-OSD-6 A; other ops never create owned state): if the cell is not watched, call `sys.watch(cell, gen)` **before dispatch** (the serving context is only valid until the next receive, and in this loop it is still the current message); on success sweep a stale generation of the same CellId (as NET: release that entry's tids and cancel its token) and record `(root_tid, token)`; always record `sender` in the cell's tid set (the creating **thread** tid is what `cleanup_owner` keys on). A refused watch is **fail-open** (D-OSD-3 A): serve, log `[compositor] owner death watch unavailable; window reclaim deferred` once, retry on the next create from that cell. Table full: same fail-open.
- `take_death(root_tid) -> Option<(cell, gen, Vec<usize>)>`: exact root-tid match only (a worker tid or 0 never matches), removes the entry (the kernel consumed the token at the death).
- Cancelling: a watched cell is dropped from the table only by its death or a stale-generation sweep (the kernel entry count is bounded by live cells, as NET-OD R-10); no per-surface cancel.

**Reclaim.** `message_handler::cleanup_owner(tid, ...)` becomes `pub(crate)` and returns the number of surfaces removed (today `()`); the death arm calls it for every tid of the dead cell, then renders (the existing `pending_dirty` union repaints the freed rectangles). It already: removes the surfaces' views, `input.remove_surface(cap)` (clears `selected_cap`/`focused_owner` when it was the focused cap, `pointer_router.rs:88-97`), removes z-order entries, `sys_grant_unregister` for every distinct grant id (frees the transferred frames). One log line only when something was removed: `[compositor] owner death: cell C gen G: removed S surfaces`.

**Unchanged on purpose.** The supervisor `0xE2` relay (`message_handler.rs:62-71`, `init/src/supervisor.rs`, `supervisor/src/main.rs`) stays (D-OSD-4: cleanup is idempotent, a second `cleanup_owner` for an already-clean tid is a no-op, and no evidence yet shows the watch path covering a hypervisor-death boot; delete it in a later cleanup once a hypervisor-death run exists). `render_frame`, surface protocol, `input_handler.rs`, `pointer_router*.rs`, `surface_table/**`, `window.rs`/`grant.rs`/`surface.rs`/`view.rs` handlers: byte-identical.

### 6.4 Exact file closure (pre-edit SHA-256 at HEAD `952d84c8a`)

| Path | Action | SHA-256 before |
|---|---|---|
| `kernel/src/task/scheduler.rs` | edit: `OWNER_WATCH_SERVICES`, `may_watch_owner_deaths`, doc comment `:250-257` | `e49d5fea5559275dff32dbdb2469d2d1ff190462cd94aaa30d63923908c711d2` |
| `kernel/src/task/syscall.rs` | edit, **tests only** (`mod tests`: matrix generalised, tripwire) | `c00a71b14fc774e7d961cc302930812c59449ba70932b7e17c1f935148e13b85` |
| `kernel/src/task/tcb.rs` | doc comments only (D-OSD-12) | `3e8880236286f30b33afa8a72291eb7fefa58fe5b8c7a4f0fd38aba7127cdf61` |
| `cells/services/compositor/src/main.rs` | edit: attested receive, classification, death arm, `WatchSys` adapter, `mod owner_watch;` | `22ebdd9a340c053aa4443dd81d0e69022a8084293ddb2c6f460a75c6fa1cee9d` |
| `cells/services/compositor/src/message_handler.rs` | edit: `cleanup_owner` `pub(crate)` + returns count (relay arm unchanged) | `2170f961e3085fb08c1df5fa9f53265873ee6c8951e7bf165f6e8d0e31f9f8fc` |
| `cells/services/compositor/src/owner_watch.rs` | **new**: table, admission, death, stale sweep, `WatchSys`, inline `#[cfg(test)] mod tests` (as `frame.rs:614`) | ABSENT |
| `CHANGELOG.md` | edit at CP2 | `c5ef0411d34cd77667a91ca71f8d937fe8ff819d3c22228e036a6ede3f6aa5db` |

**Pinned, must stay byte-identical (STOP otherwise):**

| Path | SHA-256 |
|---|---|
| `libs/api/src/abi/syscall.rs` | `b202dfa28f461b2c8f9940901294187e65b6ef4ff94638dc1004c01d78325957` |
| `libs/api/src/abi/cell_owner.rs` | `d6d14a5c115858262cb6b2961c96c6afe0a2a037e8ec1f53044ee272f3779e22` |
| `libs/api/src/abi/caller_identity.rs` | `ac5886ae8f03fcef9cb3984bf89fb2c1c57c9154f94f76d17cb63602f5d2cb89` |
| `libs/ostd/src/syscall.rs` | `95e4beb7e60561a061a6aed841bf2fc981a7a99e0c54716fcd3db4df7708c944` |
| `kernel/src/cell/service_registry.rs` | `3209e8328bc3b89bf8f38674e5703b9a8b470333b6d484b681a725455079fd2c` |
| `kernel/src/fast_ipc.rs` | `4676aa66513c9f938544a46a69272d8f59818a648ea75db79a10b21747042d68` |
| `kernel/src/task/async_ipc.rs` | `55a63ee5535a0ab7e170c117adaa14663d0d278661ec7ef86c25165f541f038a` |
| `kernel/src/task.rs` (OSD-3's file) | `a27bc5d110f03cb3db428d6013ea3c9ac3a1bf0426cc099283944a3ba06d512d` |
| `kernel/src/main.rs` | `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64` |
| `config/services.toml` | `188d4d1966a91ad0bb4fb0336548a27969f86f349bd2b05041704ed8d5deecdb` |
| `cells/services/compositor/Cargo.toml` | `4dd6839f533c977d000d395731ae5dbb9c8bd88774c7d78619bf68191598f859` |
| `cells/services/compositor/build.rs` | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` |
| `cells/services/compositor/src/message_handler/grant.rs` | `441199cc1a503db504bcc4890ce4bcd0cc255f1dae4af98ddd93b5f35262f7af` |
| `cells/services/compositor/src/message_handler/surface.rs` | `e4a99b4c4aa671bce961e94390c38e7aa33cc5471967bef4d5b16ba919936224` |
| `cells/services/compositor/src/message_handler/view.rs` | `f4a8922c56e9eaad96b4ff0b58b81d36ff722b40c8a7414552c5260f820fe9aa` |
| `cells/services/compositor/src/message_handler/window.rs` | `723acf9a5d3ffaaabc1c3d80f8e59c3d7912121e776c441610ac786adf167587` |
| `cells/services/compositor/src/surface_table.rs` | `c876073e90ed5ea1063b190092e2ed7c45702ddc272a15e8c09496c36e506e15` |
| `cells/services/compositor/src/surface_table/configure.rs` | `c63ce7eced7e227ba4a69d7c484ea5a84427c2cd6e435b1c229fb20529047152` |
| `cells/services/compositor/src/surface_table/frame.rs` | `2ae91a0fde1e3866cd78a9eeeed95196fb59e68c6770ad1c38979c3f57799ac7` |
| `cells/services/compositor/src/surface_table/lifecycle.rs` | `b1101d6ad433dc2284d30b61a20da735128522e1c208d7214820c52528f487a4` |
| `cells/services/compositor/src/surface_table/pixels.rs` | `c6c18157ac9bd2a2bce51ac5b8998ec6fdb9dd7abafe750671142c8d31b2a2d7` |
| `cells/services/compositor/src/surface_table/state.rs` | `3b968d5974db8ba3ba44e40db7cea520221d2fc37e1c4d4fb247c0d237b923df` |
| `cells/services/compositor/src/surface_table/storage.rs` | `3f4734d8c4b7060c8bc33b0e32fb29a90a76e30d272d7972dc4342376739a787` |
| `cells/services/compositor/src/input_handler.rs` | `76d506096b0b78969bd5ac786b1e6fdbd9c7ac84e659aca3d18b3a50261fda7b` |
| `cells/services/compositor/src/pointer_router.rs` | `a205756f18ac6fb54fe630a5fcef0819cd0206734f0cb746006ed52c1de94310` |
| `cells/tools/init/src/supervisor.rs` (relay retained) | `717223bf8752d0c68446dcec7a3d8d62000a6cc04f9e9286e6132ffd9f8d01c8` |
| `cells/services/supervisor/src/main.rs` (relay retained) | `d7e30e84bafceb3dd3d981ab47f1b543f2e7e80220681cefa16d33658981e0c3` |

Any other path discovered mid-slice stops the slice and returns to the owner as an amendment (Phase 02 A1 pattern). No dependency, feature, manifest or `Cargo.lock` change.

### 6.5 Tests (host only)

Kernel (`cargo test --offline --locked -p cellos-kernel --target x86_64-unknown-linux-gnu`, default and `--features phase04-driver-profile`; baseline after NET-OD was 295/295 per `net-od-cp2-dossier.md` section 1, **re-capture before the first edit**): the generalised grant matrix and the table tripwire of 6.2 item 3; the NET cases keep passing unchanged.

Compositor (`cargo test --offline --locked -p service-compositor --target x86_64-unknown-linux-gnu`; CI runs it at `.github/workflows/ci.yml:1693`; **no test may reach a `sys_*` call**: surfaces in tests use `PixelSource::Owned`, never a registered grant, because `sys_grant_unregister` on the host target is a real Linux syscall with a Cellos number):
1. `admit` watches once per cell for `CREATE_SURFACE`/`CREATE_VIEW`, never for other opcodes; second create from the same cell (and a second tid) does not call `WatchSys::watch`; tids recorded.
2. refused watch: served, not recorded, retried at the next create, log-once flag set; table full: same.
3. `take_death`: exact root tid only (worker tid, 0, another cell's root do not match); entry removed; second call `None`.
4. stale generation sweep: a new generation of the same CellId returns the old tids and cancels the old token, old generation not resurrected.
5. classification: `Some(identity)` with `sender_tid != sender` is dropped; `None` + watched root = death; `None` + unwatched = drop (a zero-trailer leftover after the death is dropped, table not mutated).
6. integration with real `SurfaceTable`/`FrameRegistry`/`ZOrder`/`InputState` fixtures: two cells, each with a surface and a view (owned pixels); death of cell A through the pure death routine removes exactly A's surfaces, views, z-order entries and focus, B intact; focus on A's surface falls to none; a worker-thread-created surface of A is removed too; repeat death is a no-op; the pending-dirty rectangle covers A's old bounds.
7. the `0xE2` relay arm still removes a tid's state (regression of the retained path).

Also: `cargo clippy --offline --locked -p service-compositor -p cellos-kernel --all-targets` (host) warning counts not above baseline; `rustfmt --check` on every closure path.

### 6.6 Builds and budget

Private `CARGO_TARGET_DIR=/tmp/osd1-target`: `cargo check --offline --locked -p cellos-kernel --target x86_64-unknown-none` (default, `--features test-hooks`, `--features phase04-driver-profile`); `service-compositor` bare-metal `--release` for riscv64gc-unknown-none-elf, aarch64-unknown-none-softfloat, x86_64-unknown-none (flag sets exactly as `net-od-cp2-dossier.md` section 4; the dev profile does not link these cells even at baseline); `python3 scripts/cellos-sign --check` rc 0. **QEMU boots: 0.** Not run, not claimed: any boot in which a client is killed (section 10).

### 6.7 Risks (OSD-1)

- **R-1 Behaviour change for identity-less messages** (D-OSD-5 A): legitimate unattributable senders would now be dropped. None identified (every compositor client is a cell with a trailer; the `0xE2` sender is the supervisor cell), but not boot-proven; fallback is the one-line choice B.
- **R-2 Wrong tid set**: a surface created by a tid the compositor never recorded would survive (`admit` records on every create attempt, before dispatch).
- **R-3 Shared kernel files with NET-OD** (same three kernel files): rebase onto the landed NET-OD hashes of section 6.4; no function other than `may_watch_owner_deaths` changes.
- **R-4 Stale kernel watch entries**: one per live watched cell, bounded; dead cells' entries are consumed by the death itself.
- **R-5 Hypervisor relay duplication**: for hypervisor-owned surfaces the new path fires first (the hypervisor cell is a watched cell) and the relay later finds nothing; harmless.
- **R-6 No boot proof** of the wire path (attested receive, kernel grant); only compile and host logic.
- **R-7 Other architectures**: same source for rv64/aarch64; only x86-64 host reasoned; bare-metal builds listed in 6.6.

## 7. Slice OSD-3 final CP1: no blocking send to a retiring or terminated task

### 7.1 Design

`kernel/src/task.rs`: one private helper `fn ipc_target_is_tombstone(task: &Task) -> bool { matches!(task.state, TaskState::Retiring | TaskState::Terminated) }` and its use in `ipc_send` (first lock block, with the `contains_key` test at `:2316`) and in both lock blocks of `ipc_send_kernel` (`:2356`, `:2385`): a tombstone target returns `IpcSendError::TargetGone` exactly as a missing one (the `Syscall::Send` arm maps it to `InvalidCommand`, `syscall.rs:4525`). `ipc_try_send` already requires `Recv` state and is untouched. The VFS call at `syscall.rs:6215` gets the same refusal (it is a send to a dead cell, never useful).

Rationale: `exit_task` sweeps `Sending` senders once (`scheduler.rs:1616-1625`); a send that arrives after the sweep and before quiescence parks forever (section 4.3). A tombstone is "dispatch-visible so stale remote contexts see a terminal state" (`scheduler.rs:1508-1511`), not a receiver.

No ABI, `libs/**`, scheduler or service change.

### 7.2 Exact file closure

| Path | Action | SHA-256 before |
|---|---|---|
| `kernel/src/task.rs` | edit: helper + three checks | `a27bc5d110f03cb3db428d6013ea3c9ac3a1bf0426cc099283944a3ba06d512d` |
| `kernel/src/task/ipc_wire_tests.rs` | edit: host test(s) (the module is already `#[cfg(test)]`-registered at `task.rs:97-98`) | `1faac9bdbcf60636e80e559d4b71db8fcd31b668654f9e4e0d9cef1908f90da8` |
| `CHANGELOG.md` | edit at CP2 | `c5ef0411d34cd77667a91ca71f8d937fe8ff819d3c22228e036a6ede3f6aa5db` |

Pinned: every file of the OSD-1 pinned table that is not in this closure, plus `kernel/src/task/scheduler.rs` (re-hash after OSD-1 if it landed first) and `kernel/src/task/syscall.rs`. Read-only at slice start, STOP if any sends to a `Retiring`/`Terminated` task (their expectations concern live receivers): the boot self-tests `kernel/src/task/ipc_pending_selftest.rs` (`ipc_send` at `:83,126,224,401`), `ipc_guardrail_selftest.rs` (`:37,106`) and `ipc_wire_selftest/{copy_case,race_case}.rs`.

### 7.3 Tests

1. Pure: a `Task` in each of `Retiring`, `Terminated` is a tombstone; `Ready`, `Recv{..}`, `Sending{..}`, `Frozen{..}` are not.
2. Full path under `TEST_STATE_LOCK` if the host harness allows `ipc_send_kernel` (it reads `SCHEDULER`, `sender_context`, `arm_ipc_block_handoff`; **[INFERENCE: host feasibility unchecked]**, fallback: only test 1 plus a boot self-test is *not* added because the self-tests are wired from the protected `main.rs`): caller `Ready`, target `Retiring` => `Err(TargetGone)`, caller state unchanged (not `Sending`), nothing queued in the target mailbox; control: live `Recv` target still `Ok(0)`.
3. `cargo test -p cellos-kernel` (both feature sets) counts unchanged except the new tests.

Budget: host + `cargo check -p cellos-kernel --target x86_64-unknown-none` x3 feature sets; 0 boots. The boot-time proof is the existing ipc self-tests (they must keep passing at the next lane run; not re-run here).

### 7.4 Risks

R-1 a real sender relying on queueing into a retiring peer (none known; the three boot self-tests are listed in 7.2 for a re-read). R-2 `Terminated` is also a filter value elsewhere (`syscall.rs:755,2030`), consistent with "not live". R-3 residual: a service that is *already parked* on a tombstone before this lands stays parked (no sweep added; D-OSD-7 B would add one at quiescence and is not recommended).

## 8. Slice OSD-2 final CP1 (owner-gated): KMS drops a dead owner's staged import and unlock secrets

Adopt only with D-OSD-2 = A. Builds on OSD-1 (the table) and touches nothing outside `ssh-dev-custody`.

### 8.1 Kernel

`OWNER_WATCH_SERVICES = [service::NET, service::COMPOSITOR, service::KMS]` (`KMS = 13`, `libs/api/src/abi/syscall.rs:1408`). Tests: the matrix's negative list drops `KMS`, a KMS-bound identity is granted, the tripwire array is updated. Files: `scheduler.rs`, `syscall.rs` (tests) at their post-OSD-1 hashes (record them at slice start).

### 8.2 KMS design (all under `cfg(feature = "ssh-dev-custody")`; the default and production-like KMS binary stays **byte-identical**, checked as in `k4b-cp1.md` section 8)

- `main.rs`: receive stays `sys_recv_attested(0, ..)` (already attested, mask 0, so `Recv` delivers deaths with no kernel delivery change). Before `handle`: `if caller.is_none() && service_state.ssh_owner_died(sender) { continue }` (exact watched root tid; a zero-trailer message from any other sender goes to `handle` as today and gets `CallerUnattested`). After `handle` returns `Some(response)` and **before the reply** (the serving context is cleared after an async reply, `async_ipc.rs:316-319`): `service_state.ssh_owner_reconcile(caller, &mut KernelWatch)`.
- `ssh/owner.rs` (new, `pub(crate)`): `OwnerWatches { slots: [Option<Watch{principal, root_tid, token}>; 4], refused: bool }` and pure `reconcile(owners: [Option<Principal>; 2], caller: Option<Principal>, sys: &mut impl OwnerSys)`: (1) for each watch whose principal no longer owns the import or unlock slot: `sys.cancel(token)` and drop it (bounds the kernel map: a live `kms-ssh-admin` that finished does not keep an entry), (2) if `caller` owns a slot now and is not watched: `sys.watch(cell, gen)` and record `(root_tid, token)`; refusal is fail-open (D-OSD-9 A), log once `[kms] owner death watch unavailable; staged secrets reclaimed by the next Begin`, retried at the caller's next opcode. `died(root_tid) -> Option<Principal>`: exact root match, removes the entry. Slot owners are read from the existing `pub(crate)` fields: `ImportSlot::{Receiving, Complete}(state).transfer.principal`, `UnlockSlot::{Collecting, Staged}(state).transfer.principal` (`ssh/import.rs:91-96`, `ssh/unlock.rs:22-27`; no edit to those files).
- `SshLeaf::release_owner(principal)` (in `owner.rs`): if the import slot is owned by `principal` call `wipe_import()` (arena zeroized, slot `Idle`), if the unlock slot is owned call `wipe_unlock()` (`secret` zeroized, slot `Empty`). **Untouched**: `identity` (persisted or volatile key), `revision`, `floors` (LRU handles them; the entry of a dead principal costs one of four slots), `failures`, `state`. Never persists anything.
- `dispatch.rs`: `KmsService` gains `owners: OwnerWatches` and two `pub` methods `ssh_owner_died(root_tid) -> bool`, `ssh_owner_reconcile(caller, &mut impl OwnerSys)`; `pub trait OwnerSys { fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)>; fn cancel(&mut self, token: u64); }` is re-exported from `lib.rs` **only** under the feature, with primitive arguments so `Principal` stays `pub(crate)`. `lib.rs` edit is one `pub use` line (so `lib.rs` joins the closure).
- Persist build (`ssh-dev-persist`): identical logic; a dead owner never alters the persisted record (tested).

### 8.3 Exact file closure

| Path | Action | SHA-256 before |
|---|---|---|
| `kernel/src/task/scheduler.rs` | edit: add `service::KMS` to the table | post-OSD-1 value (record at start); pre-OSD-1 `e49d5fea5559275dff32dbdb2469d2d1ff190462cd94aaa30d63923908c711d2` |
| `kernel/src/task/syscall.rs` | tests only | post-OSD-1 value; pre-OSD-1 `c00a71b14fc774e7d961cc302930812c59449ba70932b7e17c1f935148e13b85` |
| `cells/services/kms/src/main.rs` | edit (cfg'd): died check, reconcile, `KernelWatch` adapter | `7fc9a2ba634a020af73efa3f43c214761acb82d4e7fe2e3ccb1eb281d4bdf516` |
| `cells/services/kms/src/lib.rs` | edit: cfg'd `pub use` of `OwnerSys` | `5023526eda64557c6688c4f3bf69ecaef24874d8fc5b31acce30b7b97d065c95` |
| `cells/services/kms/src/dispatch.rs` | edit: field + two methods (cfg'd) | `2cde9db965079ae465ebfa707f2ada9e37d0c59366b60292671c39a0b4cb417d` |
| `cells/services/kms/src/ssh.rs` | edit: `pub(crate) mod owner;` (one cfg'd line) | `178b539842937c8b84f4c0ccdc0d7063099b3e2c48a89ab1a98659d842625ce4` |
| `cells/services/kms/src/ssh/owner.rs` | **new** | ABSENT |
| `cells/services/kms/src/tests/ssh.rs` | edit: `mod owner;` | `a3ac30e4a28c475572f9dbcdb7cbd5cffda8c41c4c89bd0229306bbbc2ed7485` |
| `cells/services/kms/src/tests/ssh/owner.rs` | **new** | ABSENT |
| `CHANGELOG.md` | edit at CP2 | `c5ef0411d34cd77667a91ca71f8d937fe8ff819d3c22228e036a6ede3f6aa5db` |

Pinned: `cells/services/kms/src/auth.rs` `c0e43e3b02a1b17401ae51572781c88325625323c66fdbacd97ff616518cda71`, `build.rs` `792505f3f152be9c322eb27666105777a61a1b0e3604254e882d76711373e900`, `Cargo.toml` `6384bcf565520dd99d43c88da69d2ca3cb5a10b51616f2d313e237cf17e55ca0`, `src/tests.rs` `337295452c62694f318af705ac5aa40eb3e72897a7884bcd0ef03301bd8356f6`, `ssh/import.rs` `0b204cabee028c55fda5970bc3c6fd5e2e4bb0e3fdb5e63e3c4884b50d1c785a`, `ssh/unlock.rs` `9323798ebf0f06dcfdc725fd48797070ad12f5d08fdc387abf5f880a7bc32ac2`, `ssh/floor.rs` `7eb92a005cc4bb5098e7e89777b73a26d87ca5e0e27b1f6b3264d40673fed4de`, `ssh/persist.rs` `b84bf3f128e2c71d9f9359742b40e7d2d3325137d23f7a22d34f0bc208ecf86e`, `ssh/store.rs` `55250449a9817ecc11da370b65423735eeb826fafbb84b80f9e779c700a6e5f2`, `dispatch/ssh.rs` `307717a22244a3e0cd6af1ccf31fddbebebc711756a2055af09534dbdf4b6fac`, `libs/ostd/src/clients/kms.rs` `129e18dde03366610fef64fd8f0d18b9f81c55b3a0dcf6f9461179b7b717ed76`, `scripts/check-ssh-dev-persist-exclusion.py` (the five markers must stay unique to their builds: no new string contains a marker; the log strings above contain none), plus the OSD-1 pinned libs/kernel list.

### 8.4 Tests (`cargo test -p service-kms --features ssh-dev-custody --target x86_64-unknown-linux-gnu`; persist suite `--features ssh-dev-persist --lib` as `ci.yml:1765-1767`)

Scripted `OwnerSys`; fixtures from `tests/ssh/fixtures.rs` (`Actor`):
1. import: A begins and sends chunks, `died(root_A)` + `release_owner` => arena `is_zeroed()`, slot `Idle`; a Complete-but-uncommitted import too; B's `import_begin` then proceeds without the replace path and with the arena reused.
2. unlock: A begins and sends all chunks (`Staged`), death => `secret` all zero, slot `Empty`; a subsequent `sign` by anyone fails `SshUnlockFailed`/order as for an empty slot (no stale use).
3. death of a principal that owns no slot, or of B while A owns the slot: nothing wiped.
4. exact root match: worker tid, 0, other generation of the same cell do not match; a second death is a no-op.
5. reconcile: begin => one watch; commit/abort/sign-consumption/replace-by-other => the watch is cancelled; no second `watch` call while the principal keeps a slot (no kernel duplication); table never exceeds 2 live entries in a begin/commit loop of 100.
6. refused watch is fail-open: op succeeds, slot works, log flag once, retried; staged secret then reclaimed by the old next-Begin rule (unchanged behaviour).
7. identity `None` from an unwatched sender reaches `handle` and yields `CallerUnattested` (unchanged).
8. persisted/volatile identity, `revision`, `floors`, `failures` unchanged by a death (compare before/after); persist suite: a death never writes a record.
9. no new bytes without the feature: bare-metal x86_64 release builds `default` and `ssh-dev-custody`-off are byte-identical to a `git archive HEAD` build (acceptance, as K4b), and the three artifacts pass `check-ssh-dev-persist-exclusion.py` `--expect none|custody|persist`.

Builds: private target dir; x86_64-unknown-none release `default`, `--features ssh-dev-custody`, `--features ssh-dev-persist` with the K4b/K5 flag set (`-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic`); the `hardware-relay-provider`/`CELLOS_PRODUCTION` guards still fire. `python3 scripts/cellos-sign --check`. Boots: 0.

### 8.5 Risks (OSD-2)

R-1 value is hygiene only (4.2): owner may reject the slice. R-2 `main.rs` is the K4b banner file; the died/reconcile lines sit inside cfg blocks away from the banners. R-3 KMS reply path unchanged: the blocking `sys_send` hazard of 4.3 is neither worsened nor fixed (OSD-3 fixes the tombstone case only). R-4 a death raced with an in-flight KDF: KMS finishes the sign (consumes the slot) and then sees the death record: `release_owner` finds nothing to wipe; harmless. R-5 no boot proof (no K8 scenario kills a client mid-import).

## 9. Lock handoff packet (form of `net-owner-death-cp1.md` section 11; DRAFT for the accountable owner, not submitted; one packet, three bounded slices OSD-1, OSD-3, OSD-2)

**9.1 Slice identity.** OSD-1: kernel owner-watch service table + compositor reclaim of a dead client's windows. OSD-3: kernel refusal of blocking sends to retiring/terminated tasks. OSD-2 (owner-gated): KMS reclaim of a dead owner's staged secrets under `DEV_POSTURE`. Serves the general "close resources on consumer death" rule (`phase-09-ssh-client.md` item 5) and the NET-OD follow-up list; **not an Intel C2C outcome** (the owner is asked to confirm the same bounded-slice basis as Phase 02, K2, K3, K5, NET-OD). Controlling plan `.agents/261011-1200-cell-contract-authority/plan.md` (complete; completion unlocks nothing). Evidence reconciled: `net-owner-death-cp1.md` + `net-od-cp2-dossier.md` + `cp2-dossier-part5.md`, `k4b-cp1.md`, `k5-cp1.md`, `k6-cp1-draft.md`, this document.

**9.2 ADR-0024 contract delta.**

| Principle | Effect |
|---|---|
| Cell principal/incarnation distinct from Task | the watch binds `(cell_id, generation)` and the root tid, attested by the kernel; the compositor additionally tracks **thread** tids only as ownership keys it already used, never as principals |
| Shared per-Cell capability space; delegation | no new capability, handle, right or CapSet field; OSD-1/2 widen **who may hold an existing information-only watch** from {VFS, NET} to an explicit table |
| Revocation generations | grant compares the registered provider **generation**; a stale cell cannot hold it; dead generations never match again |
| Kernel substrate vs ordinary policy Cells | the kernel attests a death and a registry-bound right; what is reclaimed (windows, staged secrets) stays service policy; OSD-3 is a kernel liveness rule (tombstone is not a receiver) |
| Durable vs execution state | none durable; OSD-2 explicitly leaves persisted identity untouched |
| Caller/test/doc migration, concurrency, remote | callers: compositor and KMS are the only new watchers; the compositor's receive becomes attested (one behaviour change: identity-less messages dropped, D-OSD-5); tests per sections 6.5, 7.3, 8.4; docs: `CHANGELOG.md`; lock order SCHEDULER -> REGISTRY unchanged; no remote impact |
| No ambient credential / remote shortcut | the right needs the registered provider binding **and** the live serving context: a service learns the death only of a principal that just sent it a message |

**9.3 Locked rows and overlapping plans (read-only; names from `cell-contract-0024-locks.md`, rows at the NET-OD packet's numbering; `git status` clean apart from `.agents/`).**

| Row / plan | Relation | Needed |
|---|---|---|
| `260603-1803-phase-h-vfs-hardening` (L2), `260809-0922-sas-lbi-revocable-vfs-access` (L23), `260927-0739-kernel-architecture-repair` (L35), `260621-0830-cell-perms-p2-p5` (L8) | kernel permission/owner-lifetime files `scheduler.rs`, `syscall.rs`, `tcb.rs` (OSD-1/2) and `task.rs` (OSD-3); same functions NET-OD (adopted, `cell-contract-0024-locks.md:125`) already opened; no VFS behaviour change, no syscall/CapSet right | file-bounded exception per slice; owner confirms no in-flight claim on those files |
| `260801-parallel-midori-closure` (L21), `260927-1100-c2c-anywhere-tier-aware` (L36), `260827-1015-sas-caller-owned-range-ledger` (L33), `260624-0630-kernel-boundary-cleanup` (L9) | adjacent: completion/peer-death and async IPC (OSD-3 touches the same send path read-only in intent); `vfs-lifetime self-test PASS` text unchanged | note; for OSD-3 L21/L36 owners confirm the tombstone refusal is compatible with their peer-death design |
| `260825-1726-kms-silo-production-root`, `260826-1605-phase4-dev-reference-authority` | KMS files (OSD-2 only): same rule as K4b (C-K4b-1: Phase 06 rebases on post-slice hashes); no `storage/**`, `lifecycle/**`, relay or enrollment file is touched | file-bounded exception for the section 8.3 paths; Phase 06 rebase note |
| `261006-0934-httpd-single-cell-concurrency` (L38) | touches Net files; **none** of OSD-1..3 does | none |
| compositor / desktop plans: `261019-1310-unified-desktop` (completed, no lock row; its phase 03 record documents the supervisor-relayed `cleanup_owner`), `260607-1854-compositor-grant-surfaces`, `window-manager-policy`, `desktop-window-policy` (all "unchanged ... no new lock") | the compositor files of 6.4 are in no locked plan's named scope (grep-level over `cell-contract-0024-locks.md`: no row names compositor paths) | owner confirms no overlap or names one |
| `260615-1325-vicell-app-sdk-v1` (ServiceRef/AppContext) | adjacent only: none of its files is edited | note |
| NET-OD (landed) | same three kernel files; rebase onto `e49d5fea…` / `c00a71b1…` / `3e888023…` | none |
| not examined: `260712-1902-dice-attestation-identity` P03, `260825-sdk-delivery` P02, `260819-1409-cell-to-cell-anywhere-core` P02, `migration-ledger.md` | grep-level | owner confirms no overlap or names one |

**9.4 Retained gates.** Law 1: not triggered (section 11); the owner is asked for an explicit "no ABI delta" acknowledgement and for the reading that the compositor's attested receive and the table widening are behaviour of existing syscalls. Protected identity / non-rollback time/state: not applicable (no storage, no time; OSD-2 leaves custody state alone). Independent human decisions: D-OSD-1..15. No hardware, AWS or production gate is touched; `DEV_POSTURE` for anything KMS. A green host test does not replace these gates.

**9.5 Scheduling change (text only).** Record "OSD-1" and "OSD-3" (and "OSD-2" if adopted) as bounded adopted slices with acceptance ceilings: OSD-1 "kernel table + compositor reclaim, host tests + bare-metal builds, no boot proof"; OSD-3 "kernel tombstone refusal, host tests"; OSD-2 "KMS dead-owner secret wipe, host tests, `DEV_POSTURE`", in the controlling plan, `plan-portfolio.md` and `cell-contract-0024-locks.md` next to the NET-OD paragraph (`:125`), each as a file-bounded exception for its own closure with this packet's digest; narrow only the applicable relations of L2/L23/L35/L8 (and L21/L36 for OSD-3); WIP stays one implementation slice, single integrating owner.

**9.6 Execution boundary and rebase order.** Baseline first (kernel default and `phase04-driver-profile`, `service-compositor`, `service-kms` default/custody/persist counts; `git status` clean; digest re-check of the closure tables). Edits only inside the section's closure; pinned files re-verified; `kernel/src/main.rs`, `libs/**`, `Cargo.lock`, manifests, scripts, images untouched (STOP otherwise); no network, key, VM or QEMU during implementation. Order: OSD-1, OSD-3, OSD-2 (each its own commit, kernel and consumer of a slice in the same commit so the pair is never mismatched). Rollback: restore pre-edit hashes. Mismatch safety: new kernel + old consumer: no watch is ever called (safe); old kernel + new consumer: watch refused, fail-open, log once, today's behaviour (safe; OSD-1 also still drops identity-less messages, which is the only mismatch-independent change).

**9.7 Decisions requested.** (1) Adopt OSD-1 and OSD-3 (and decide OSD-2, D-OSD-2) with the section 6.4/7.2/8.3 exceptions. (2) Name the kernel-permission owner and the compositor and KMS file owners; rule the L2/L23/L35/L8, L21/L36 and KMS-plan relations. (3) Acknowledge no `libs/api`/`libs/types` bytes and the two behavioural points of section 11. (4) Approve the one scheduling record. Nothing in this packet authorises a source edit.

## 10. Verification, lanes and run budgets

| # | Proof | Covers | Budget |
|---|---|---|---|
| V1 | host tests of sections 6.5, 7.3, 8.4 | table matrix, classification, admission, cleanup, tombstone refusal, KMS wipe | build + test minutes each; baselines **not measured** here, capture before the first edit |
| V2 | bare-metal release builds (6.6, 8.4) + `cellos-sign --check` | the wire code compiles on rv64/aarch64/x86-64; KMS default byte-identical | minutes |
| V3 | **boot proof: not available.** Compositor: needs a lane that kills a GUI client; the GUI runner states the kill route does not exist on that image (`qemu-phase04-gui-test.sh:77`). KMS: needs a K8 scenario that kills the admin or login cell mid-transfer. Either is a separate lane CP1 (new scenario, driver edit, judge rows), not requested | the real kernel grant + attested receive + death delivery in the compositor and KMS | would be 1 diag boot each [INFERENCE] |

Not claimed after V1-V2: any boot-observed reclaim in the compositor or KMS; ghost-window removal on a real display; kernel behaviour of the `Retiring` window (OSD-3 is reasoned from code and unit-tested at the helper level).

## 11. Law-1 analysis

`code-standards.md:42` scopes Law 1 to `libs/api/` and `libs/types/`. None of the three slices changes a byte of either tree, a constant, discriminant, payload or syscall they define: `OWNER_WATCH_SERVICES` only **reads** `api::syscall::service::{NET, COMPOSITOR, KMS}` (`libs/api/src/abi/syscall.rs:1377,1383,1408`); `CellOwner`, the 32-byte trailer, syscalls 244-248 and the compositor/KMS wire formats are unchanged; `libs/ostd/src/syscall.rs` and `clients/kms.rs` are pinned. Hence **no two-confirmation checkpoint is requested**; `libs/api` doc comments stay (the Watch doc does not say VFS-only, per NET-OD section 2.1).

Behavioural points the owner should see: (1) who may hold a watch widens from {VFS, NET} to an explicit table (kernel permission table, not ABI); (2) the compositor now **requires** an attested sender identity (D-OSD-5); (3) OSD-3: a send to a retiring task now fails instead of parking. If the owner reads any of these as an ABI reinterpretation, the alternative would be a new syscall/flag (Law-1, rejected as heavier for the same effect).

## 12. Decisions (OPEN until the user answers; recommendations in the last column)

| ID | Decision | Options | Recommendation |
|---|---|---|---|
| D-OSD-1 | Kernel grant mechanism for more services | **A** explicit const table of registry-bound ids (this text); **B** any registered service provider (NET-OD D-NOD-1 B); **C** new CapSet right + policy + signer; **D** static per-service handler cell | **A** (one array element per adopted consumer, tripwire test, no new state, unforgeable by name; B hands an information right to every `SpawnCap`-registered cell) |
| D-OSD-2 | KMS slice OSD-2 | **A** adopt after OSD-1 (this text); **B** defer as YAGNI (S3 only, `DEV_POSTURE`, low probability) | **A if the owner wants the invariant "no staged custody secret outlives its owner"; otherwise B.** Honest ranking: not needed for correctness or availability; cheap once OSD-1 exists |
| D-OSD-3 | Compositor when the watch is refused | **A** fail-open, log once (as NET); **B** fail-closed (deny `CREATE_SURFACE`/`CREATE_VIEW`) | **A** (image without a registry-bound compositor must keep a working desktop; B would turn a leak fix into an availability risk) |
| D-OSD-4 | The `0xE1/0xE2` hypervisor relay after OSD-1 | **A** keep, idempotent (this text); **B** delete in OSD-1 (adds `init/src/supervisor.rs` and `supervisor/src/main.rs` to the closure); **C** delete in a later cleanup once a hypervisor-death run shows the watch path covers it | **A now, C later** (no boot evidence yet; the contract's clean-cutover default is applied at C, with the reason recorded) |
| D-OSD-5 | Identity-less messages to the compositor | **A** drop (denies zero-trailer leftovers of a killed client, matches `caller_identity.rs:30-32`); **B** serve as today | **A** |
| D-OSD-6 | When the compositor watches a cell | **A** first `CREATE_SURFACE`/`CREATE_VIEW`; **B** first request of any kind | **A** (only those create owned state) |
| D-OSD-7 | Kernel send-to-tombstone gap | **A** refuse `Retiring`/`Terminated` targets in `ipc_send*` (OSD-3); **B** also wake parked senders at quiescence; **C** none | **A** (B is more invasive in the retirement funnel and not needed once A lands, except for senders already parked) |
| D-OSD-8 | Live non-receiving client wedging blocking-reply services (KMS, compositor, supervisor, block drivers) | **A** accept and document (K6 mitigates the honest client only); **B** services reply to raw callers with `TrySend` and clients move to the async `ipc::submit` path (client + service protocol work per service; honest raw clients lose replies if not yet in `Recv`, so a client migration is a precondition); **C** kernel send timeout or "reply = queue and return" flag (Law-1, ABI) | **A now; B or C only as a separately designed CP1 if a hostile-caller threat to KMS/compositor availability is accepted** |
| D-OSD-9 | KMS when the watch is refused | **A** fail-open (status quo ante: next-Begin replacement); **B** refuse ops 15/22 | **A** |
| D-OSD-10 | KMS watch lifetime | **A** reconcile-cancel when the principal no longer owns a slot; **B** keep until death | **A** (bounds the kernel `cell_owner_watches` map and the KMS table) |
| D-OSD-11 | Proof | **A** host tests + bare-metal builds (this text); **B** also a boot lane scenario per service (separate CP1) | **A** now; B only if the owner wants boot evidence |
| D-OSD-12 | `tcb.rs` doc-only edit | **A** include (stale "VFS, NET" comments in a permission path); **B** skip | **A** |
| D-OSD-13 | Lock treatment | **A** three bounded slices with file-bounded exceptions (section 9); **B** fold into the pending L38/L35 plans | **A** |
| D-OSD-14 | AI, input, net-broker, config, httpd, sshd, hypervisor, silo, supervisor, ocel-*, drivers | **A** YAGNI, with the AI per-owner session cap as its own future item; **B** give each a watch | **A** |
| D-OSD-15 | Incidental findings of 5.3 (block-driver sender checks, supervisor reply policy) | **A** hand to the owner as separate security items; **B** include | **A** |

## 13. Not checked

No compile, test, lane or QEMU run; current host test counts of `cellos-kernel`, `service-compositor`, `service-kms` at HEAD; whether `ipc_send_kernel` is callable from a host test (7.3 item 2); the `Retiring` send window was **never reproduced** (code read only, plus the sub-audit); whether any boot self-test sends to a `Retiring` task (7.2); how long the quiescence window lasts on a multi-hart image; which cells may `LookupService`/`Send` to KMS, the compositor or the block drivers (launch-profile allowlists not read); the `cellbox` client's exact stage-then-sign gap (K6 text level only); `ostd` `display` client behaviour (does a client ever send CREATE_SURFACE from a worker thread: the tid-set design assumes it may); whether any compositor client relies on being served with an identity-less message; aarch64/rv64 behaviour beyond the bare-metal builds; the rows marked **[sub-audit]** (net-broker, config, httpd, sshd, hypervisor, silo, supervisor, ocel-*, block/GPU/HID drivers, audio/TLS, the kernel-side cleanup table); the hypervisor-death scanout path under the new watch; that `scripts/cellos-sign --check` stays green after other plans' pending edits.

## 14. Effort and entry gates

Effort (about): OSD-1 M, 3-4 working days (kernel 15 + compositor 150 production lines, ~350 test lines); OSD-3 S, 0.5 day; OSD-2 M, 3 days (about 170 + 350); docs small [INFERENCE from the NET-OD sizing, not measured]. Entry gates for each: (1) decisions of section 12 bound in an approval record (digest of this file at binding); (2) the lock packet of section 9 approved with owner names and overlap rulings; (3) baseline capture and start-of-slice digest and overlap re-check; (4) owner acknowledgement of section 11. A boot-proof lane CP1 (V3) is a separate approval and not a gate for landing.
