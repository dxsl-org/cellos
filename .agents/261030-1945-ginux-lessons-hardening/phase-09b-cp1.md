# Phase 09B CP1 — software provider crash/restore at a provider-owned state authority (Law-1 additive)

Status: **CP1 APPROVED by the user 2026-11-01.** Option B ("Duyệt B"); K1 syscall 431 `QueryRetirement` / `ViRetirementV1` ("Duyệt K1"); hotswap-demo restore failure becomes RecoveryFailed, no cold-start opt-in ("Đổi thành RecoveryFailed"); VFS test-hooks-only missing-ack knob ("Duyệt"); 09A drift fixed in code: retirement removes the dead owner's Open stash entries ("Sửa code"). **CP2 APPROVED by the user 2026-11-01 ("Duyệt CP2 + commit") on the §11 evidence; independently re-run: kernel host 340/340, provider-recovery 2/2.**
Admission: 09B admitted 2026-11-01 (`admission-approval.md:82`); this CP1 is the separate
design approval that line requires. Depends on 09A (complete, `phase-09a-cp1.md:3`).

## 0. Verdict

Today a crashed provider loses its business state. Nothing commits a request before
the reply, and nothing fences or replays requests. The 09A sealed stash cannot carry
state across a crash: sealing needs a live frozen source. **Recommended: Option B.**
A provider-owned RAM state-authority Cell commits a request binding before each reply,
fences writer epochs and becomes the worker's lifecycle parent. One additive kernel
query (K1) reports the retirement verdict for a dead incarnation, so a restart is
never published when containment is unproven. The 410/411/412/421 semantics stay as
they are. The state is RAM-only and survives a worker crash only (§6).

## 1. Facts (observed in source)

### 1.1 What happens today when a provider crashes
- A user fault on any thread retires the **whole Cell incarnation**. That covers a
  clean root exit, an unhandled user fault on the root or a worker, and an authorized
  stop. The kernel marks the generation `Retiring` and exits every member
  (`kernel/src/task/scheduler.rs:1377-1429`). Entry from the trap is
  `terminate_current_cell_on_user_trap_fault` (`kernel/src/task.rs:1268-1279`).
- Resources are released only after quiescence. Every member must have left every
  hart, each remote hart must have completed its switch epoch, and no DMA publication
  may be in flight (`kernel/src/task/scheduler.rs:1859-1948`). After that the funnel
  reaps each member (`kernel/src/task.rs:1444-1453` → `reap_retired_task_resources`
  `kernel/src/task.rs:1916-1927`). It then drops zombies, clears the fast-IPC VFS
  route, releases the resource registry and SHM, releases the owner slot and finally
  deregisters the quota (`kernel/src/task.rs:1454-1497`).
- Service routes for the dead tid are cleared at `exit_task`
  (`kernel/src/task/scheduler.rs:1619-1620` → `kernel/src/task/drivers/driver_cell.rs:205-217`).
- **Init restart:** init restarts according to the policy (`never` / `on-failure` with
  `reason != 0` / `always`). The budget is 5 restarts per 1000 ticks
  (`cells/tools/init/src/supervisor.rs:8-9,44-48,69-85`). The restart is a fresh
  spawn: `sys_spawn_from_elf` / `sys_spawn_from_path_raw`
  (`cells/tools/init/src/service_table.rs:63,75`), not `SpawnReplacement`. The demo
  provider is configured `restart = "never"` (`config/autoload.toml:22-28`). The
  hotswap supervisor has no death handling for providers. It only relays the
  hypervisor-scanout message `0xE1` (`cells/services/supervisor/src/main.rs:105-115`).
- **State does not survive a crash.**
  - The demo counter lives only in worker memory (`static STATE`,
    `cells/demos/hotswap-demo-v1/src/main.rs:70`).
  - `"inc"` mutates the counter and replies with no request identity
    (`cells/demos/hotswap-demo-v1/src/main.rs:130-134`).
  - The worker writes the stash only on a supervisor `Snapshot` event
    (`cells/demos/hotswap-demo-v1/src/main.rs:88-105`), never per mutation.
- **The stash cannot carry state across a crash after 09A:**
  - Entries are readable only by their `Open` owner or their `Sealed` destination
    (`kernel/src/cell/state_stash.rs:51-57`).
  - Sealing happens only inside `SpawnReplacement`
    (`kernel/src/task/syscall.rs:7949-7955`). That call needs a live task in
    `TaskState::Frozen` with a matching ceiling (`kernel/src/cell/hotswap.rs:143-163`)
    and SupervisorCap (`kernel/src/task/syscall.rs:7898-7900`).
  - A crashed owner's `Open` entries therefore cannot be read by any new incarnation.
    They are dropped lazily, only when the table is full
    (`kernel/src/cell/state_stash.rs:145-146,176-199`).
  - Stash bytes are kernel heap that is not charged to any Cell quota. The bound is
    64 × 1 MiB (`kernel/src/cell/state_stash.rs:21-30,148-156`).
- **Finding (09A doc drift, not fixed here):** `phase-09a-cp1.md:28` says the retirement
  path removes a retired owner's `Open` entries. The funnel has no stash call
  (`kernel/src/task.rs:1444-1523`); reclamation is the lazy path cited above.
- **Restore failure becomes a silent cold start + Ready.** On a missing or bad stash the
  demo logs "starting cold" and still calls `sys_hotswap_ready()`
  (`cells/demos/hotswap-demo-v1/src/main.rs:118-125`,
  `cells/demos/hotswap-demo-v2/src/main.rs:91-101`). The supervisor tolerates a missing
  snapshot (`cells/services/supervisor/src/hotswap.rs:113-116`). Spec 25 requires
  RecoveryFailed unless the app explicitly opts into cold start
  (`docs/specs/25-cell-kernel-authority-contract.md:118`).

### 1.2 State authority / commit ledger
- No provider-specific state authority, writer epoch or committed watermark exists for
  any hotswap/restart provider. Searching `cells`, `libs` and `docs/specs` for
  `writer.?epoch|watermark|commit.?ledger|idempot` finds no hit on a provider. 09A
  scoped these explicitly to 09B (`phase-09a-cp1.md:35-36`, `kernel/src/cell/state_stash.rs:13-15`).
- Existing request-binding patterns live **inside the worker** and die with it:
  - Input service outcome table: same caller/operation/id with a different request →
    `Malformed`; bounded 64 per operation and 128 per caller
    (`cells/services/input/src/session.rs:307-328`).
  - KMS SSH replay floor (`cells/services/kms/src/ssh/floor.rs:38-52`).
  - Plan rule: a volatile per-worker dedup table is insufficient
    (`phase-09-post-fault-replacement.md:34`, `plan.md:82`).
- Spec 25 requires the authority to commit `(request identity, mutation outcome,
  state revision)` before acknowledging. The authority must fence the old writer and
  issue a fresh writer epoch. Crash replacement must read the latest committed
  revision from the authority, not from a worker checkpoint
  (`docs/specs/25-cell-kernel-authority-contract.md:116-118`). The authority is its own
  failure domain, and no universal state service is mandated (`:120`).

### 1.3 Cross-Cell borrows, rings, grants, pins and how retirement tracks them

| Mechanism | Record / binding | Owner (exporter) death | Borrower death | Ack / quarantine |
|---|---|---|---|---|
| Page/Reg grants | `PageGrant`/`RegGrant`: owner tid + `owner_cell`/`owner_generation`, `shared_to: (tid, perm)` (`kernel/src/task/syscall.rs:190-206,855-864`) | `reclaim_owned_grants` unmaps from the grantee and frees; frames still in use are withheld (`syscall.rs:1256-1270`) | `clear_grantee_refs`: grant becomes unshared; domain receiver drained with invalidation ack (`syscall.rs:1211-1254`) | via pins below |
| Pins (DMA / cache-sync) | owner = **tid** (not incarnation) (`kernel/src/memory/pin.rs:455-492`) | `quarantine_task` marks; frames withheld (`pin.rs:654-672`, `syscall.rs:1192-1209`) | n/a | released only by `acknowledge(tid)` (`pin.rs:799-815`); quarantine bounded to 64 regions (`pin.rs:62-65`); **overflow leaks with only a log line, no counter** (`syscall.rs:1551-1562`) |
| VFS leases | holder tid / grant owner / request generation (`pin.rs:817-857`) | `pending_revoke` + quarantine until the holder releases (`pin.rs:865-915`) | `release_vfs_holder_leases` (`pin.rs:917-933`, `task.rs:1447`) | exact holder release |
| SHM | `SHM_HANDLES` frame → `(cell, generation)` (`syscall.rs:45-54`) | unmap + all-hart flush, then free, at root retirement (`syscall.rs:142-180`, `task.rs:1471`) | **no borrower record**: mapped USER at the identity VA in the shared root (`syscall.rs:5750-5785`) | none; no Cell consumer (only `libs/ostd/src/syscall.rs:1104-1120`) |
| DMA / IOMMU / BDF | per tid | quiesce → revoke/fence → release; on timeout, deferred and retried (`task.rs:1826-1873,1916-1923`) | n/a | IOFENCE/IOTLB completion |
| IRQ waiters | per tid | `release_waiters_for` (`kernel/src/task/drivers/irq_wait.rs:157-162`, `task.rs:1924`) | n/a | n/a |
| Async IPC ops | kernel-owned bounded ops; no caller memory (`kernel/src/task/async_ipc.rs:1-2`) | peer-gone completion (Spec 25:101) | — | — |
| Futex | key `(space, generation, addr)`; SAS shares the root, so a word is cross-Cell (`kernel/src/task/futex.rs:3-12`) | — | waiter entry dropped (`futex.rs:29-30,300`) | **no robust list; never force-unlocked** (`futex.rs:28-30`) |
| Stash | owner/destination incarnation (`state_stash.rs:45-58`) | lazy reclaim (above) | — | — |

Raw SAS pointers passed in messages have no kernel record. For safe-Rust Tier-1
peers this is the failure boundary the plan states (`phase-09-post-fault-replacement.md:14,38`).

### 1.4 Observability and test lanes
- Userspace sees allocator totals through `MemInfo`/`ViMemInfoV1`
  (`libs/api/src/abi/syscall.rs:145-148,1484-1489`, bit 56 at `:1141-1142`).
  `quarantined_pages()` (`pin.rs:947-951`) and `cell_quota::in_use`
  (`kernel/src/memory/cell_quota.rs:354-358`) are kernel-internal. The private P01
  trace (`kernel/build.rs:15-31`) always marks heap class 8 unknown
  (`kernel/src/p01_resources.rs:199-202,313-315`). So a kernel-heap no-leak claim is
  not available today.
- Death notification: `NotifyOnExit`, gated by SpawnCap, subscribes the caller via
  `subscribe_death` (`kernel/src/task/syscall.rs:6098-6131`). `ForceExit` needs SpawnCap
  and refuses system and frozen targets (`syscall.rs:5875-5927`). `RegisterService`
  needs SpawnCap (`syscall.rs:6203-6207`).
- Attested caller `(cell_id, generation)` trailer: `libs/api/src/abi/caller_identity.rs:1-35,112-121`.
  Staged argv is task-owned and inherited only by that child
  (`kernel/src/cell/state_stash.rs:65-107`, `kernel/src/task/launch.rs:169-172`), and
  both regular and replacement spawns consume it (`syscall.rs:1879,1901`).
- `hotswap-smoke` boots with `boot_with_fresh_disk` → `boot_with_netdev`, which passes
  no `-smp`, so it runs on **1 hart** (`tests/integration/src/lib.rs:382-399,1863`).
  Only the disk-less RV64 lane and aarch64 pass `-smp`
  (`tests/integration/src/lib.rs:436-466,632-667`).
- Next free ABI number after `TelemetrySnapshot = 430` (`libs/api/src/abi/syscall.rs:614`):
  431 has no current user. The u64 allowlist is full (`:1052-1056`), so a new
  lifecycle query must be dispatch-gated like `NotifyOnExit` (`:1163-1176`).

## 2. Problem

09B must:
- restore the exact committed business state and outcomes after a worker crash at any
  point;
- refuse commits from the old writer;
- refuse conflicting request-ID bindings;
- never publish Ready for an incomplete destination;
- prove whole-Cell retirement on 2 CPUs, with tracked borrows quarantined and
  untracked exports or abandoned locks failing closed.

Today:
- No authority survives the worker (§1.2).
- The only cross-incarnation transport needs a live frozen source (§1.1).
- No lifecycle owner can see whether a dead incarnation's retirement was contained
  (§1.4).
- The witness lane is uniprocessor (§1.4).

## 3. Options

### A. Kernel stash as commit store, plus a supervisor post-mortem seal
- **Commit:** the worker writes one blob containing revision, ledger and state with
  `StateStash` before every reply. The single-entry overwrite under the stash lock is
  the atomic commit point (`state_stash.rs:157-161`).
- **Crash:** dead-owner `Open` entries are retained as `Orphaned` instead of lazily
  reclaimed. A new SupervisorCap op seals them to a freshly spawned replacement. The
  kernel incarnation check fences the old writer.
- **Law-1:** changes the dead-owner semantics of 410/412, adds a seal syscall and
  changes quota charging.
- **Pros:** no extra Cell. One syscall per commit. Reuses 09A.
- **Cons:**
  - It puts business commit and writer fencing into generic byte transport, which the
    plan forbids (`phase-09-post-fault-replacement.md:26,34`).
  - Every mutation copies the whole state, up to 1 MiB (`syscall.rs:7692-7697`).
  - Kernel heap holds uncharged business state (`state_stash.rs:148-156`).
  - Dead-owner retention needs a new bound.
- **Rejected.**

### B. Provider-owned RAM state-authority Cell, also the worker's lifecycle parent (recommended)
- A per-provider authority Cell (e.g. `recovery-authority`) holds:
  - the committed state and revision;
  - the writer epoch, bound to the worker's attested `(cell, generation)`;
  - a bounded ledger keyed by `(caller principal, request_id)`, storing
    `(method/schema, canonical payload digest, caller rights flags, result, revision)`.
- The worker sends `Commit{epoch, expected_revision, binding, new_state, result}`. It
  replies to the client only after `Committed`. The authority applies the update in
  its single-threaded handler as one step: check epoch and revision, store the state,
  insert the ledger entry, bump the revision.
- The authority spawns the worker and holds `NotifyOnExit`. On death it does the
  following in order:
  1. Fences the epoch and records the final watermark.
  2. Waits, bounded, for the K1 verdict. `Unproven` → node recovery required, no
     restart.
  3. Spawns the new worker with a one-shot nonce in staged argv.
  4. The child sends `Attach{nonce}`. The authority accepts only when the nonce, the
     kernel sender tid and the attested `(cell, generation)` agree. It then binds a
     fresh epoch and returns `(schema, revision, state)`.
  5. The child validates schema and revision, then sends `Restored`.
  6. The authority publishes Ready with `RegisterService(id, child_tid)`.
- Any failed check → RecoveryFailed: the child exits and nothing is registered.
- **Law-1:**
  - The state protocol is provider-private (cell crate), so it causes no ABI delta.
  - K1 (§4) is the only public addition.
- **Pros:**
  - Matches Spec 25:116-120 and the plan literally: the authority is provider-specific
    and is its own failure domain.
  - The kernel does no business fencing.
  - The dedup table survives the worker.
- **Cons:**
  - One extra IPC round trip per mutation.
  - An authority crash loses RAM state, which correctly yields RecoveryFailed but
    costs availability.
  - Each provider needs its own authority.

### C. VFS file as ledger (RamFS now, CellosFS later)
- The worker atomically replaces a file holding `(revision, ledger, state)` per
  commit. A restarted worker reads the file back.
- **Pros:** an existing independent Cell. It is the natural path to durability once
  the CellosFS owner (261025, Phase 01 item 7: `phase-01-admission.md:17,40-43`) ships.
- **Cons:**
  - VFS has no per-file writer epoch, so a delayed live writer keeps its handle.
    Fencing needs a VFS protocol change (compare-and-swap on revision/epoch, Law-1 if
    the VFS wire changes).
  - It is a generic store, against "no generic state service"
    (`phase-09-post-fault-replacement.md:46`).
  - A filesystem write per mutation.
  - RamFS gives no durability. A durable claim needs CellosFS, which is not owned here.
- **Rejected for 09B.** Revisit when CellosFS owns two-slot commit.

## 4. Recommended design (B + K1): exact observable deltas

### 4.1 Law-1 delta: K1, the only public change
- **New syscall `QueryRetirement = 431`** (`libs/api`).
  - Arguments: `a0 = root_tid`, `a1 = out_ptr`, `a2 = out_len` → returns
    `size_of::<ViRetirementV1>()`, or an error.
  - Not on the allowlist (the u64 is full). Gated at dispatch.
  - Allowed callers: a SpawnCap caller that was a death subscriber of that root at
    exit, or a SupervisorCap caller.
- **Errors:**
  - `PermissionDenied`: the caller is neither.
  - `FileNotFound`: no record, i.e. unknown or evicted. Callers must treat this as
    Unproven.
  - `InvalidInput`: `out_len` ≠ the exact record length.
- **Record `ViRetirementV1`.** `#[repr(C)]`, fixed length, version 1, reserved fields
  must be zero, unknown versions refused (Phase 06 rule,
  `phase-06-abi-record-convention.md:19`). Fields:
  - `cell_id`, `generation`, `root_tid`;
  - `state`: `Pending`, `Contained`, `Quarantined` or `Unproven`;
  - `reason_bits`;
  - `member_threads`;
  - `remote_hart_mask`: harts whose switch completion was awaited;
  - counts: `grants_reclaimed`, `grantee_refs_cleared`, `pins_quarantined`,
    `vfs_leases_pending`, `shm_frames_released`, `dma_deferred`;
  - `quarantined_pages_delta`, `leaked_pages`.
- **Classification**, computed in the retirement funnel (`task.rs:1444-1523`):

  | State | Condition |
  |---|---|
  | `Pending` | Quiescence or deferred DMA not yet complete |
  | `Contained` | No outstanding obligation |
  | `Quarantined` | Only counted, bounded quarantines remain: pins, VFS leases, deferred DMA retried |
  | `Unproven` | `SHM_UNTRACKED` (incarnation owned SHM frames: a USER mapping in the shared root with no borrower record), or `QUARANTINE_OVERFLOW` (pages leaked at `syscall.rs:1557-1561`, now counted) |

  `WRITABLE_BORROW` is reported, not classified. It means the incarnation died holding
  writable grantee references to a live peer's grant, so the owner must treat that
  memory's invariants as unproven and must never unlock or reuse a lock inside it.
- **Storage:** a bounded table of recent records (e.g. 64), evicting the oldest final
  record. A `Pending` record is never evicted.
- **Unchanged:**
  - The meaning of 410/411/412/421/401.
  - Exit reasons and `NotifyOnExit` delivery timing (still at `exit_task`, before
    quiescence; the verdict is the post-quiescence result).
  - The kernel performs no forced unlock and does not add robust futexes.
- **SDK:** `ostd::syscall::sys_query_retirement` wrapper plus a typed record decode.

### 4.2 Provider-private semantics (no Law-1; specified here for approval)
- **Authority operations:**
  - `Attach{nonce}`
  - `Commit{epoch, expected_rev, key=(principal, req_id), method, digest, new_state, result}`
  - `Lookup{key, method, digest}`
  - `Fence`, internal on death or hang
- **Outcomes:** `Committed{rev}`, `Present{result, rev}`, `Absent`, `Conflict`,
  `StaleEpoch`, `RevisionMismatch`, `RecoveryFailed`, `QuotaExceeded`.
- **Ledger key:** `(principal (cell, gen), request_id)`.
  - Same key with a different method/schema, payload digest or attested rights flags →
    `Conflict`. The request is refused, not replayed.
  - A different principal reusing the same id is a separate key and never receives
    another caller's result.
  - Per-principal bound, with a client-advanced acknowledged floor for pruning.
    Otherwise `QuotaExceeded`.
- **Worker:**
  - Executes and commits before replying.
  - On a retried request, calls `Lookup` first: `Present` returns the recorded result
    without reapplying, `Conflict` refuses.
  - Registers nothing itself.
- **Authority with no state** (its own restart): it must answer every `Attach` with
  `RecoveryFailed`. Cold start happens only through an explicit config opt-in, never
  by default (Spec 25:118).
- **Hung worker:** the authority fences first, then sends `ForceExit`. Commits from
  the old epoch that are already queued get `StaleEpoch`, and the revision stays
  unchanged.

### 4.3 Explicit non-goals
- The 09A planned-hotswap demo (`hotswap-demo-v1/v2`, supervisor, stash) is left as is.
- Supervisor `SpawnReplacement` integration with an authority is out of scope.
- No durable or reboot claim.
- No device providers (09C).
- No generic or kernel state service.

## 5. File set (to record in `admission-approval.md` before editing)

- **ABI / Law-1:**
  - `libs/api/src/abi/syscall.rs`: 431, dispatch gate note, `ViRetirementV1`.
  - `libs/ostd/src/syscall.rs`: wrapper.
- **Kernel:**
  - new `kernel/src/cell/retirement_verdict.rs` + `kernel/src/cell/mod.rs`;
  - `kernel/src/task.rs`: the funnel fills the verdict;
  - `kernel/src/task/scheduler.rs`: `RootRetirement` carries the remote-hart mask and
    the death-subscriber snapshot;
  - `kernel/src/task/syscall.rs`: arm/decode for 431, plus the leak counter in
    `withhold_or_free`;
  - kernel host tests in those modules.
- **Provider (new):** `cells/demos/recovery-demo/` (bins `recovery-authority`,
  `recovery-worker`; `fault-inject` feature for test images only), with workspace
  `Cargo.toml` member registration.
- **Image and config:**
  - `config/autoload.toml` (authority entry under a `recovery-demo` packaging feature,
    `restart = "never"`);
  - `scripts/generate-boot-config.py`;
  - `scripts/gen-disk-ci.sh` (+ `scripts/gen_disk.ps1`);
  - `scripts/sign-policy.py` (launch rows).
- **VFS** (test-hooks only): a knob in `cells/services/vfs` that withholds one lease
  release, to inject a missing borrower ack.
- **Tests:**
  - `tests/integration/src/lib.rs`: new `boot_with_fresh_disk_smp(kernel, disk, harts)`;
  - new `tests/integration/tests/provider-recovery.rs`;
  - `cells/tests/bench` scenarios: client and fault driver.
- **Docs:**
  - `docs/hotswap-guide.md` (restore failure vs cold-start opt-in, authority pattern);
  - `CHANGELOG.md`;
  - this plan directory (also fixes the 09A doc drift note).

## 6. RAM-only vs durable

| Store | Survives | Lost on |
|---|---|---|
| Authority Cell RAM (state, revision, epoch, ledger) | worker crash, worker hang/kill, repeated worker restarts | authority crash → RecoveryFailed; node reboot; power loss |
| Kernel stash (09A transport) | source address-space teardown within the boot (`state_stash.rs:1-6`) | reboot |
| K1 verdict table | the boot, bounded | reboot or eviction (treated as Unproven) |

No durable claim is made. Reboot, node-loss and power-loss retention require the
CellosFS durable-state owner (`phase-09-post-fault-replacement.md:33`,
`phase-01-admission.md:17,40-43`). Until then that profile stays unqualified.

## 7. Evidence plan

Lane: RV64 QEMU, 2 harts, disk image, `CELLOS_INIT_FEATURES=input,recovery-demo`.
Consumer-observed results come from the client cell's received replies and
`LookupService`, not only markers. Host unit tests cover the authority ledger and K1
classification.

| # | 09B criterion | Evidence |
|---|---|---|
| E1 | committed state restored; fault **before** commit | Worker faults after receiving req N, before `Commit`. Client sees PeerGone, `Lookup` → `Absent`, resubmits the same id → applied once. Final counter = prior + 1. |
| E2 | fault **after commit, before reply** | Worker faults after `Committed`. Client resubmits the same id/payload → `Present{result, rev}` and the counter is **not** incremented again. |
| E3 | same-ID different payload refused | Same principal and id, different payload → `Conflict`; state and revision unchanged. |
| E4 | same-ID different authority refused | Same principal and id, different method or rights flags → `Conflict`. A second principal with the same id → independent result, never the first caller's. |
| E5 | old writer cannot commit | Hung worker fenced and killed. Its queued `Commit` arrives after the fence → `StaleEpoch`; revision unchanged. |
| E6 | RecoveryFailed, no Ready | Corrupt, missing, wrong-schema and wrong-revision state, each injected via the authority test feature. Child gets `RecoveryFailed`, exits, `LookupService(id)` = none, no `RegisterService`. Same result for an authority restarted with no state. |
| E7 | whole-Cell retirement on 2 CPUs | Worker has a helper thread running on hart 1 at fault time (hart marker). Verdict `remote_hart_mask` includes hart 1; `member_threads` ≥ 2; state `Contained`; both threads gone (`ps`). |
| E8 | missing tracked ack quarantines charged pages | VFS test knob withholds the lease release while the worker dies. Verdict `Quarantined`, `vfs_leases_pending = 1`, pages counted; the `MemInfo` free-frame delta equals those pages. Lifting the knob returns them. Restart proceeds only as `Quarantined` (explicit, bounded). |
| E9 | untracked export → no successful restart | Worker test build does `ShmAlloc`/`ShmMap`, then faults. Verdict `Unproven(SHM_UNTRACKED)`; authority logs node-recovery-required, does not spawn or publish. |
| E10 | abandoned shared lock: no forced unlock | Worker dies holding a futex lock word inside an authority-owned writable grant. Verdict reports `WRITABLE_BORROW`; the authority discards that grant without unlocking or reusing it and restores from its committed revision. The peer waiter is never woken as an acquirer. |
| E11 | identity, not address | Authority binds the epoch to attested `(cell, gen)`; old `(cell, gen)` refused after restart. No CR3 or stack-address assertions. Tier-2/Domain worker variant run if the domain lane builds on this image; otherwise reported **unqualified** (baseline block: `admission-approval.md:29`). |
| E12 | 10 cycles reconcile | 10 crash/restore cycles, each recording a before/settled-after vector: `MemInfo` frames, verdict counts, quarantine delta, leaked pages, authority ledger size, stash entry count (test-hooks). Released + explicitly quarantined = the change, with **zero Unproven and zero unknown**. Kernel heap is excluded from the no-leak claim (P01 class 8 is always unknown) and labeled unqualified. |
| R | regressions | `hotswap-smoke` 13/13 (09A lane), `cell::state_stash` host tests, `SMP-RETIREMENT` selftest, x86 kernel `-D warnings` build. |

## 8. Risks
- **Attach race or tid reuse.** Mitigated by requiring the one-shot nonce (staged argv
  is task-owned), the kernel sender tid and the attested `(cell, gen)` to all agree.
  The nonce is consumed on first use.
- **Verdict eviction before query.** `FileNotFound` → treated as Unproven, which fails
  closed and costs availability, not safety.
- **New 2-hart disk lane** may surface SMP defects unrelated to 09B. Those get their
  own report; they are not absorbed.
- **Now-counted quarantine-overflow leaks** may show leaks that already exist.
- **Per-mutation IPC overhead.** Measure under `bench-counters` (Phase 03 tooling); no
  performance claim without it.
- **Idle authority and the heartbeat watchdog** (`phase-09a-cp1.md:78`). The
  `hotswap-smoke.rs:92-98` guard pattern is reused.
- **Phase 06 record convention runs concurrently** (agent `Phase06Abi`).
  `ViRetirementV1` must follow its final rule.
- **`ForceExit` reach.** It allows any SpawnCap holder to kill any non-system,
  non-frozen task (`syscall.rs:5885-5916`). The authority uses it only on its own
  child, but the kernel gate is broader than that. This is not changed here.

## 9. Rollback
- Revert the 09B commit(s) and cold boot.
- All 09B state is RAM, with no persistent format.
- Removing 431 removes an additive syscall. The only SDK users are
  `recovery-authority` and the wrapper, and they are reverted together.
- External effects already committed and acknowledged to clients are not undone by a
  binary revert (`phase-09-post-fault-replacement.md:71`).

## 10. Decisions requested
1. Approve Option B (provider-owned RAM authority as lifecycle parent).
2. Approve the K1 Law-1 delta: syscall 431 `QueryRetirement`, `ViRetirementV1`, the
   gates, the errors and the classification in §4.1.
3. Approve leaving `hotswap-demo` as a documented cold-start opt-in demo (only the guide
   text changes). The alternative is to make its restore failure RecoveryFailed now;
   that changes supervisor semantics and needs its own note.
4. Approve the VFS test-hooks knob for the E8 missing-ack injection.

## 11. CP2 — implementation and evidence (2026-11-01)

### 11.1 Exact public delta (as implemented)
- `ViSyscall::QueryRetirement = 431` (`libs/api/src/abi/syscall.rs`), no allowlist
  bit (dispatch-gated group next to `NotifyOnExit`). `a0 = root_tid, a1 = out_ptr,
  a2 = out_len` → 96 on success. Typed errors via the IPC sentinel encoder
  (`has_typed_errors`): `usize::MAX-2` no record, `usize::MAX-3` length ≠ 96 or bad
  out pointer, `usize::MAX` not a recorded watcher of that root and no SupervisorCap.
- `api::retirement::ViRetirementV1` (`libs/api/src/abi/retirement.rs`): `repr(C)`, exactly
  96 bytes, version set {1}; decode refuses length ≠ 96, other versions, non-zero
  `reserved0/1`, states outside 1..=4 and unknown reason bits (rules of
  `docs/abi-boundary-records.md`). Producer: kernel; consumer: independently built Cells.
- `ostd::syscall::sys_query_retirement(root_tid) -> Result<ViRetirementV1, SyscallError>`.
- Watchers: up to 8 death subscribers of the root at retirement start (later ones get
  `PermissionDenied`). Table: 64 records, a `Pending` record is never evicted.
- Leaked-page counting: `withhold_or_free`'s `Full` branch now counts pages; a
  concurrent leak elsewhere can only over-report (fails closed).
- Unchanged: 401/410/411/412/421 numbers and arguments, exit reasons, `NotifyOnExit`.
  09A drift: `state_stash::release_retired` in the funnel; the lazy reclaim path and
  `stash`'s liveness argument are deleted.
- Demo semantics: `hotswap-demo-v1/v2` restore failure prints
  `RecoveryFailed: no valid stashed state — no Ready` and exits `0x9B` without
  `sys_hotswap_ready()`; the supervisor's ready wait fails and rolls back.

### 11.2 Deviations from §4–§7 (none changes the approved public delta)
1. VFS knob is the cargo feature `recovery-fixture`, not `test-hooks`: the test-hooks VFS
   flavour is refused on the gen-disk-ci image (flavour assertion, 1.1 KiB quota) and the
   test-hooks image has no shell/autoload for the cells. Same property: test-only,
   built only when `CELLOS_INIT_FEATURES` contains `recovery-demo`; the default image
   asserts its marker absent.
2. Recovery cells are VIFS1-only (the P2 cell table is capped at 64 rows; 66 failed).
3. Service ids are cell-private fixture ids 0x7A0A (authority, registered by init from
   `config/autoload.toml`) and 0x7A0B (published worker), same pattern as init's 0x7A01.
4. E10 has no futex waiter: the authority reads the lock word left in the discarded
   page (1) and frees the page without unlocking or reusing it; the "never force
   unlocked" half rests on the source fact (`kernel/src/task/futex.rs:28-30`).
5. E12's vector is free frames (`MemInfo`), verdict counts, quarantined and leaked pages
   and the authority ledger size; the stash entry count is not part of it (the demo does
   not use the stash). Kernel heap stays outside the no-leak claim (P01 class 8).
6. A fenced live writer is ended by its own `StaleEpoch` exit (`0x9C`), not by a timer at
   the authority (the authority loop has no attested timed receive).

### 11.3 Commands and results
| # | Command | Result |
|---|---|---|
| 1 | `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu --target-dir target/p09b-host` | 340 passed, 0 failed |
| 2 | `cargo test -p api -p recovery-demo --target x86_64-unknown-linux-gnu --target-dir target/p09b-host` | api 162 + 2 passed; recovery-demo 8 passed |
| 3 | `RUSTFLAGS="-D warnings -C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc --target-dir target/k1-x86` | Finished, no warnings |
| 4 | `CELLOS_INIT_FEATURES=input,recovery-demo bash scripts/gen-disk-ci.sh` then `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test provider-recovery -- --test-threads=1` | **2/2 pass** (`provider_recovery_two_harts`, `provider_recovery_one_hart`) |
| 5 | `CELLOS_INIT_FEATURES=input,hotswap-demo bash scripts/gen-disk-ci.sh` then `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test hotswap-smoke -- --test-threads=1` | **13/13 pass** |
| 6 | `bash scripts/gen-disk-ci.sh` (default image restored on the production path) | exit 0; `disk_v3.img` sha256 `27350b70822b0889…`, kernel `3f775518eb7b914d…`, VIFS1 `3ffe0a55832c728b…`; no `recovery-*` artifact shipped; `service-vfs` carries 0 `recovery-fixture` strings |

Kernel host tests added: verdict classification matrix, watcher/supervisor gate,
8-watcher cap, Pending never evicted, oldest final evicted, unknown close no-op,
`release_retired` (open, sealed-to-dead-destination, others kept), syscall 431 decode,
no allowlist bit and typed errors, `vfs_leases_pending_for_owner`,
`quarantined_pages_attributed`, the `recovery-authority` launch edge, and the remote-hart
mask in the existing remote-retirement test. Host: ledger and frame codec (8 tests).

### 11.4 Evidence map (RV64 QEMU, `-smp 2` unless noted; serial lines quoted)
| # | Criterion | Observed |
|---|---|---|
| E1 | fault before commit | `E1 PASS fault-before-commit: old revision kept, resubmit applied once (counter 1)` |
| E2 | fault after commit, before reply | `E2 PASS fault-after-commit: recorded outcome returned, not reapplied (counter 2)` |
| E3 | same id, different payload | `E3 PASS same request id with a different payload refused (Conflict), state unchanged` |
| E4 | same id, different authority | `E4 PASS same request id with a different method/authority refused (Conflict)…`; second principal: `E4 PASS foreign principal reusing request id 1002: independent binding, applied once (counter 3)` |
| E5 | old writer cannot commit | `E5 PASS old writer refused StaleEpoch after fence; revision 2 kept; epoch 3 -> 4` |
| E6 | RecoveryFailed, no Ready | four kinds: `E6 PASS corrupt|missing|wrong-schema|wrong-revision: RecoveryFailed, no Ready (worker service unpublished)`; worker side `[recovery-worker] RecoveryFailed: attach refused RecoveryFailed` / `schema 2 is not 1`; authority `RecoveryFailed: … no Ready publication` |
| E7 | whole-Cell retirement on 2 CPUs | `E7 PASS whole-Cell retirement: threads=2 remote_harts=0x2 state=Contained`; 3 of 11 spin crashes waited for a remote hart (2-hart run), 0 of 11 on 1 hart |
| E8 | missing ack quarantines | `[vfs] recovery-fixture: holding grant lease without acknowledgement`; `verdict … state=Quarantined reasons=0x10 threads=2 remote_harts=0x1 grants=1 … vfs=1 … qpages=1 leaked=0`; `E8 PASS missing borrower ack quarantined 1 page(s); released after VFS completed (free 30556 -> 30557)` |
| E9 | untracked export → no restart | `verdict … state=Unproven reasons=0x1 … shm=1 … (untracked SHM export)`; `node recovery required: containment unproven, no restart`; `E9 PASS … no restart, no Ready, heal refused` |
| E10 | abandoned lock, no forced unlock | `verdict … reasons=0x4 … borrows=1 writable=1`; `lent page discarded with lock word=1 (freed=true): never unlocked or reused`; `E10 PASS …` |
| E11 | identity, not address | `E11 PASS 20 published worker incarnations, all distinct (cell, generation); stale epochs refused (E5)`; Tier-2/Domain worker variant **not run: unqualified** |
| E12 | 10 cycles reconcile | cycles 1–10 `free_before=30557 free_after=30557 state=Contained threads=2 … qpages=0 leaked=0 shm=0 ledger=2`; `E12 PASS 10 cycles: all Contained, 0 quarantined, 0 leaked, settled free frames 30557 constant` |
| R | regressions | hotswap-smoke 13/13; boot self-test `state-stash: round-trip OK, foreign read refused` |

### 11.5 Remaining / not claimed
- RAM-only: no durable, reboot or power-loss claim (needs the CellosFS state owner).
- E11 Domain/private-root worker variant not run; x86 runtime lane not run (RV64 only).
- Remote-hart waits are observed (3/11 on 2 harts), not forced: the witness does not pin
  threads to harts.
- `CHANGELOG.md` entry is with Main (file owned by Phase06Abi).
