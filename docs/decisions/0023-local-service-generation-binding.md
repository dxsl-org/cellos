# ADR-0023 — Bind local service identity to the Cell generation, and carry local service calls on the bounded exact-operation primitive

> **Status**: **Accepted and FROZEN 2026-10-08.** Both Law-1 checkpoints are recorded — design
> (checkpoint 1) and the implemented ABI delta with its evidence (checkpoint 2) — in
> `.agents/260927-1100-c2c-anywhere-tier-aware/law1-lookupservicebound.md`, which is therefore the
> gate for the §2.3 surface: removal, rename, layout/discriminant change or addition now needs the
> ABI process again, including two fresh confirmations, and
> `scripts/check-lookupservicebound-law1-digests.sh` fails on drift. The kernel-repair file-owner
> handoff was granted for the full slice the same day; §2.2's registry record and §2.5's guards are
> landed. Nothing in §2.4 (moving the SDK's local calls onto the bounded primitive) is implemented:
> that is C2C Phase 03 work under its own gates.
> **Extends**: [ADR-0015](0015-dual-mode-hybrid-architecture.md) (tier model).
> **Relates to**: [Spec 17](../specs/17-ipc-wire-contract.md) (ratified wire),
> [Spec 20 draft v3](../specs/20-unified-ipc-contract.md) §2.2/§4,
> [ADR-0008](0008-protected-relay-tls-endpoint-ownership.md)/[ADR-0009](0009-correlate-relay-packet-failures.md).

## 1. Context

Phase 01 of the C2C program recorded a blocker: *"Phase-01 ratification still needs
kernel-repair owner review of live local generation binding"*
(`.agents/260927-1100-c2c-anywhere-tier-aware/phase-01-contract.md:52`), and
Spec 20 draft v3 states *"proposed generation pinning and stale-response rejection still
need a design/ABI review"* (`docs/specs/20-unified-ipc-contract.md:35`). This record is that
review. The blockers below are attached to a **specific** mechanism, not to a vague principle.

### 1.1 What the current local call path actually is

| Fact | Evidence |
|---|---|
| The registry maps `service_id → Active(tid) \| Paused(tid)`. No generation, no cell id. | `kernel/src/cell/service_registry.rs:25-31` |
| `LookupService` returns a bare TID; `0` means "no provider". | `kernel/src/task/syscall.rs:5579-5584`; stable-ABI note `libs/api/src/abi/syscall.rs:1302-1305` |
| `LocalEndpoint<M>` stores only `tid: usize`; `new` rejects zero and nothing else. | `libs/ostd/src/cluster_endpoint.rs:52-67` |
| A local call is `sys_send(tid)` then a **masked** `Recv` on that tid, with no deadline and no generation check. | `libs/ostd/src/ipc.rs:161-181` |
| `ServiceRef` self-heals by clearing its cache on `Send`/`Recv`/`WrongSender`. | `libs/ostd/src/service.rs:110-136` |
| A **bounded** variant exists (`service_call_typed_bounded`, deadline shared by admission and the reply wait) but neither variant checks the responder's generation, and its own doc requires the caller to "poison that service generation" after a receive error because a late reply may still arrive. | `libs/ostd/src/ipc.rs:216-263` |
| IPC admission checks receiver existence and the hot-swap quiesce barrier, keyed on **tid**. | `kernel/src/task.rs:2166`, `:340-359`, `kernel/src/cell/service_registry.rs:120-127` |

### 1.2 The premise this record corrects

The Phase-01 handoff implied a cached endpoint can *"send to a recycled/restarted TID"*. Source
review shows the second half is not reachable today:

| Fact | Evidence |
|---|---|
| `Scheduler.next_task_id` is a single monotonic counter, never a free list; no free-slot search exists. | `kernel/src/task/scheduler.rs:290`, `:333`, `:275` |
| It only ever increases; task death removes the record and the service route but never frees the number. | `kernel/src/task/launch.rs:118,246,251`; `kernel/src/task/scheduler.rs:711`, `:941`, `:1261-1264`; `kernel/src/cell/service_registry.rs:141-152` |
| Sending to a dead tid fails hard and immediately: `TargetGone` → `SyscallError::InvalidCommand`. | `kernel/src/task.rs:2166`; `kernel/src/task/syscall.rs:4154-4155` |

So within one boot a stale cached tid produces a **hard, safe failure**, not misdelivery. The
real defects are different and smaller, and they are what this decision addresses:

1. **No recorded incarnation.** Nothing in the kernel or the endpoint says *which* Cell
   generation a lookup named. The registry has no such field, so a client cannot hold, compare,
   or report a provider identity — it can only observe "the tid is gone".
2. **The synchronous reply path has no bounded terminal and no responder check.** A client that
   has completed its send and is blocked in a masked `Recv` is *not* woken when the provider
   dies. The kernel says so itself: *"A plain reply waiter that has already left `Sending` is
   still not woken here (known pre-existing gap, 2026-07-31 Recv buffer-pinning audit); fixing
   it needs a state-machine audit."* (`kernel/src/task/scheduler.rs:1279-1284`). The only exits
   are the caller's own liveness heartbeat or an unrelated `WrongSender`. Adding a deadline
   (the bounded variant does that) bounds the wait but still cannot tell a **late reply from the
   previous incarnation** apart from the current one, which is why its doc puts the burden on
   the caller to poison the generation by convention (`libs/ostd/src/ipc.rs:229-230`).
3. **A load-bearing invariant has no guard.** Local IPC safety currently rests on "tids are
   never reused", which is true but unasserted, and two of the three allocation sites use a
   bare `+= 1` that would wrap to `0` in release builds — the reserved "no provider" sentinel
   and a live-task collision (`kernel/src/task/scheduler.rs:711`, `:941`, versus the guarded
   `checked_add` at `kernel/src/task/launch.rs:251`).
4. **The reuse that *is* real is on the other axis.** `CellId` slots are explicitly reusable,
   which is exactly why a per-Cell `cell_generation` epoch exists
   (`kernel/src/task/tcb.rs:547-556`, `:479-493`; `kernel/src/task/scheduler.rs:277-279,330,349-362`).

### 1.3 The kernel already ships the correct primitive

The bounded **exact-operation** IPC path already binds, bounds, and reports:

| Property | Evidence |
|---|---|
| Submit binds the peer as `(cell_id, cell_generation)` read live from the scheduler. | `kernel/src/task/async_ipc.rs:106-138` |
| Reply acceptance re-checks `peer_tid`, `peer_cell`, `peer_generation`, and the operation phase. | `kernel/src/task/async_ipc.rs:165-172` |
| Peer death transitions every matching operation to terminal `PEER_GONE`, matched on `(tid, cell, generation)`. | `kernel/src/task/async_ipc.rs:57-64`, `:298-307`; wired at `kernel/src/task/scheduler.rs:1324-1325` |
| Bounded terminal outcomes exist as a type, not a convention. | `libs/ostd/src/ipc.rs:53-66` (`Reply`, `PeerGone`, `PreDispatchTimeout`, `Indeterminate`, `Cancelled`) |

The same axis is already duplicated across the kernel: `CallerIdentity{cell_id, generation,
sender_tid}` (`libs/api/src/abi/caller_identity.rs:62-70`), `CellOwner{cell_id, generation,
root_tid}` (`libs/api/src/abi/cell_owner.rs:54-58`), the wire header's `sender_cell_id` /
`sender_generation` (`kernel/src/task/ipc_wire.rs:24-31`), dir attestation, hot-swap ceilings,
and retirement matching.

The correct answer to "what is a local service's generation?" therefore already exists in the
tree under one name. It is not the TID, and it does not need a new token type.

### 1.4 Observed on Intel x86_64 QEMU (2026-10-08)

Sections 1.1–1.2 are source reading. This section is measured behaviour, produced by one run that
puts the two paths side by side against the same dead peer on the same kernel — and that also
exercises the §2.3 opcode end to end.

- **Scenario:** `bench local-service-lifecycle` (`cells/tests/bench/src/scenarios/local_service_lifecycle.rs`).
  A provider probe consumes exactly one request and then calls `sys_exit(0)` **without replying**.
  `SYNC-RESULT=RETURNED` is printed only if the kernel ever wakes the synchronous caller; its
  absence is the finding.
- **Image:** production feature set (no `test-hooks`), built by
  `scripts/build-x86_64-c2c-lifecycle-ci.sh`.
- **Observed, leg 0 (the §2.3 opcode):** `VFS-BINDING tid=4 cell=1 gen=109 matches_lookup=true`
  — the bound lookup names the same provider `LookupService` does, with a live Cell identity, and
  `ABSENT-BINDING lookup=None bound_is_none=true` for an id this image does not provide.
- **Observed, leg A:** the synchronous caller never returned.
- **Observed, leg B:** the same run's bounded exact-operation call reported
  `ASYNC-TERMINAL=PEER-GONE`.
- **Evidence:** `docs/evidence/local-service-lifecycle-x86-qemu.txt` (summary) and
  `docs/evidence/local-service-lifecycle-x86-qemu.log:192-204` (raw), reproducible with
  `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu
  --test local-service-lifecycle-x86`.
- **The §2.5 guard**, separately: `[selftest] TASK-ID-REUSE: PASS (monotonic across spawn/exit)`
  on an AArch64 `test-hooks` boot which then runs its guest suite to `96 PASS, 0 FAIL`
  (`docs/evidence/task-id-reuse-guard-aarch64-qemu.{txt,log}`) — see §2.5 for why that lane and
  not x86_64.
- **Guard against a vacuous pass:** the same test against `build/vicell-x86.iso` (an image
  without `/bin/bench`) fails, so the assertions are only satisfiable by the scenario
  actually running.
- **Not covered by this evidence:** a provider *restart* (the provider is not respawned), any
  remote/relay path, and any change to the synchronous call path the SDK uses. The existing RV64
  broker oracle covers restart-and-re-lookup, but its client is never a plain reply waiter blocked
  in a masked `Recv`, which is precisely the shape measured here.

Defect 1.2(2) is therefore not only documented but reproduced on the Intel target.

## 2. Decision

### 2.1 The binding axis is the pair `(cell_id, generation)` — never the TID

A local service binding is `(tid, cell_id, generation)` where `(cell_id, generation)` is the
existing kernel-minted per-Cell epoch (`kernel/src/task/tcb.rs:547-556`, `:655`). `tid` is a
transport detail inside that binding: it is never reused, so it is safe to *carry*, and unsafe
to *trust as identity* — for the same reason `CellId` alone is unsafe there.

No new token type, minting rule, or per-task generation is introduced. The Async/IPC path,
`CallerIdentity`, `CellOwner`, dir attestation and retirement matching already consume this
axis; a second, parallel identity concept is the failure mode this decision exists to prevent.

### 2.2 The kernel service registry records the provider's `(cell_id, generation)`

`ServiceEntry::Active(tid)` becomes `ServiceEntry::Active { tid, cell_id, generation }` (and
`Paused` likewise). The value is captured at `RegisterService` from the provider's live task
under the `SCHEDULER` lock the handler already takes for its `tid_is_live` check
(`kernel/src/task/syscall.rs:5563-5574`), so registration and identity capture are one atomic
act. `clear_tid` and the pause/commit paths keep their current semantics.

This is **kernel-internal**: `LookupService`'s observable behaviour does not change, so no ABI
is touched. Lock order stays `SCHEDULER → service registry`, the order
`paused_target_rejects` already documents (`kernel/src/task.rs:340-345`).

### 2.3 Exactly one append-only syscall exposes the binding; `LookupService = 206` is untouched

`LookupService` returning a bare tid with `0 = absent` is documented stable ABI
(`libs/api/src/abi/syscall.rs:1302-1305`). Reinterpreting it is a frozen change, and the
repository's own precedent for this situation is to add a versioned sibling rather than
redefine the old one (`GetProcs2 = 239` was added while `GetProcs = 30` kept serving;
`docs/project-changelog.md:3750-3766`).

**Proposed: opcode 429 `LookupServiceBound`** — the next free number after `SerialConfigure = 428`
(`libs/api/src/abi/syscall.rs:567`; `400` must stay unmapped, `500-503` are reserved).

Confirmed ABI (Law-1 checkpoint 1, 2026-10-08 — see
`.agents/260927-1100-c2c-anywhere-tier-aware/law1-lookupservicebound.md`):

- **Signature**: `LookupServiceBound { service_id: u16, out_ptr: usize, out_len: usize }`,
  encoded as `a0 = service_id`, `a1 = out_ptr`, `a2 = out_len`.
- **Success return**: `SERVICE_BINDING_LEN` (24), the bytes written — the same
  bytes-written convention `QueryDirHandles = 241` already uses for a fixed kernel-written
  record (`libs/api/src/abi/syscall.rs:609-620`).
- **No live provider**: `0`, the same absence sentinel `LookupService` already returns
  (`libs/api/src/abi/syscall.rs:1302-1305`). A `Paused` provider answers `0` too, preserving the
  hot-swap quiesce barrier that `is_paused_tid` gives the tid-keyed path today
  (`kernel/src/task.rs:340-359`). So does a provider whose recorded identity is not live
  (`cell_id == 0` or `generation == 0`): the kernel must not state a binding it cannot stand
  behind, and `LookupService`'s tid answer is unaffected.
- **Short buffer**: `SyscallError::BufferTooSmall` — never a partial write. This is the
  error the two existing fixed-record writers already use (`QueryDirHandles`,
  `ResolveCellOwner`), so the implementation follows them rather than the `InvalidInput`
  first drafted here; the deviation is recorded in the Law-1 record for checkpoint 2.
- **Record**: new module `libs/api/src/abi/service_binding.rs` —
  `pub const SERVICE_BINDING_LEN: usize = 24;` and
  `#[repr(C)] pub struct ServiceBinding { pub tid: u64, pub cell_id: u64, pub generation: u64 }`,
  little-endian `to_bytes`/`from_bytes` and an `is_live()` that requires all three fields
  nonzero. Layout discipline follows `CellOwner` (`libs/api/src/abi/cell_owner.rs:54-71`).
  No reserved field: a future need is a new opcode, which is this repository's append-only
  precedent, and an unvalidated reserved field would be an unused surface today.
- **Semantics**: resolved and written under one `SCHEDULER → registry` section, so the three
  fields are one instant. No caller-selectable tier, epoch, or tid is accepted.
- **Allowlist**: shares bit 37, the bit `LookupService` already owns as an *open* syscall
  (`libs/api/src/abi/syscall.rs:1000-1001`), so any client that may resolve a service endpoint
  today may resolve its binding tomorrow. This matters because the `u64` allowlist is **full** —
  bits 0–62 are syscalls and bit 63 is the VFS-mutate declaration
  (`libs/api/src/abi/syscall.rs:938-940`, `:1068`) — so a fresh bit is not available, and sharing
  an existing one is the only append-only option.

The alternative name `LookupServiceEx` is rejected only because `Bound` states the property
being added; the number, not the name, is the ABI.

### 2.4 Local service calls ride the existing bounded exact-operation primitive

No new send opcode. A bound local call is:
`LookupServiceBound(id)` → hold `(tid, cell_id, generation)` → `sys_ipc_submit(tid, request)`
→ `sys_ipc_take` / `sys_ipc_wait` → terminal.

This closes defect 1.2(2) **without changing `Send`**, because the async path's `peer_died`
already delivers terminal `PEER_GONE` to a blocked client keyed on `(tid, cell, generation)`
(`kernel/src/task/async_ipc.rs:298-307`, `kernel/src/task/scheduler.rs:1324-1325`). The
documented sync-path gap is closed by *not using that path for service calls*; `Send`/`Recv`
keep their ratified Spec-17 semantics for cells that use them directly, including the broker
benchmark oracle, which must not change.

Consequences that follow directly and are therefore not open questions:

- **No expected-generation argument on `submit`.** Between lookup and submit, a tid cannot
  change owner (1.2), so `submit(tid)` already resolves the live binding itself and answers a
  definite `PeerGone` otherwise. Adding an expected binding today would be a check against a
  threat that does not exist. **Trigger to revisit:** if TID reuse is ever introduced, `submit`
  and any bound-send must take the expected `(cell_id, generation)` in the same commit. Decision
  2.5 exists to force that conversation.
- **The terminal does not have to return the provider identity.** Generation verification is an
  admission-time property in the kernel; the caller's stored binding is a cached descriptor, and
  `PeerGone` already distinguishes "incarnation is gone" from `Indeterminate`.

### 2.5 The TID non-reuse invariant is load-bearing, and is now guarded

Recorded explicitly: **within one boot, a task id is never re-issued.** Everything in 1.2 and
2.4 depends on it. Both guards landed 2026-10-08 under the granted kernel-repair handoff:

1. The two bare `self.next_task_id += 1` sites now go through
   `Scheduler::advance_task_id`, which uses the `checked_add(1).expect(...)` form the cell/ELF
   path already used (`kernel/src/task/launch.rs:251`). Exhaustion fails closed instead of
   wrapping to `0` — which is simultaneously the reserved "no provider" sentinel and a potential
   live-task collision. | `kernel/src/task/scheduler.rs` |
2. A boot-time `test-hooks` guard, `kernel::task::task_id_selftest`, spawns real threads through
   the ordinary syscall path, retires each through the real death funnel, and asserts no
   allocation re-issues a retired number and that the sequence strictly increases. It is
   transparent to boot (it snapshots and restores `next_task_id`, removes its synthetic parent and
   reaps every thread) and runs in the same single-hart window as the other task self-tests.
   | `kernel/src/task/task_id_selftest.rs`, registered at `kernel/src/main.rs` |

Observed: `[selftest] TASK-ID-REUSE: PASS (monotonic across spawn/exit)` on an AArch64
`test-hooks` boot which then runs its guest suite to `96 PASS, 0 FAIL`
(`docs/evidence/task-id-reuse-guard-aarch64-qemu.{txt,log}`). The x86_64 `test-hooks` lane cannot
reach it: it panics earlier in an **unrelated, pre-existing** frame-accounting case
(`docs/evidence/atomic-publication-x86-pre-existing-failure.{txt,log}`, reproduced with this whole
slice stashed).

### 2.6 Remote `ServerEpoch` is the same node-local axis, not a second concept

The C2C remote descriptor's `ServerEpoch` (`libs/types/src/c2c.rs:28-50`) denotes one live
exported-server incarnation, scoped today to one boot-local broker
(`docs/specs/20-unified-ipc-contract.md:35`). This decision fixes the local half of that axis
(`cell_id, generation`) and leaves the remote half exactly as Spec 20 describes: a broker
incarnation must be **protected and non-rollback** before it may order replay, and the current
uptime-derived beacon epoch is not such a source
(`cells/services/net-broker/src/local_runtime.rs:74-86`). Local generation does **not** authorize
remote epoch reuse; conflating them is explicitly refused.

## 3. Consequences

- One kernel-internal registry change plus **one** append-only syscall, instead of a new send
  opcode, a new reply-matching opcode, and a parallel identity type.
- `LocalEndpoint`/`ServiceRef` gain a real, comparable provider identity and a bounded terminal;
  `ViError::IO` stops being the only vocabulary for "the provider went away".
- The `Send`/`Recv` allowlist, Spec 17's ratified clauses, and every existing caller — including
  the C2C broker benchmark oracle — are untouched.
- Recorded costs, not hidden ones: a second lookup opcode must be added through the full
  registration path — `ViSyscall` variant, `From<usize>` arm (`libs/api/src/abi/syscall.rs:1154`),
  `allowlist_bit` arm sharing bit 37 (`:938`, `:1001`), the `CASES` table in
  `libs/api/src/abi/syscall_tests.rs:21`, and the kernel dispatch arm — and the registry entry
  grows by 16 bytes per service, bounded by `MAX_SERVICES = 32`
  (`kernel/src/cell/service_registry.rs:19-23`).
- **Until the guards in 2.5 land, the invariant is documentation, not enforcement.** This is
  stated rather than implied.

## 4. Alternatives rejected

| Alternative | Why rejected |
|---|---|
| Change `LookupService = 206` to return a binding | Frozen, documented stable ABI (`libs/api/src/abi/syscall.rs:1302-1305`); the repo's own precedent is an additive sibling (`GetProcs2 = 239`) |
| Add a `SendBound` opcode now | Defends against recycled tids, which source shows do not occur (1.2); `submit` already resolves the live binding and answers `PeerGone` |
| Mint a new per-task (per-TID) generation | The real reuse is on `CellId` slots, which is why `cell_generation` exists; a second identity axis would need its own mint, guard, and every consumer migrated |
| Put the generation in the `LocalEndpoint` constructor argument | Caller-supplied identity is exactly what the plan forbids; the value must come from kernel state at registration |
| "Fix" the sync reply path in place (add a deadline + generation filter to `Recv`) | Requires a state-machine audit of a path the kernel already documents as unfinished (`kernel/src/task/scheduler.rs:1279-1284`); the async primitive already has the required semantics |
| Treat local `(cell_id, generation)` as the remote replay epoch | Local epochs are boot-local and not protected; Spec 20 requires a protected non-rollback source for replay ordering |

## 5. What this decision does not authorize

- The ABI is **FROZEN** as of Law-1 checkpoint 2 (2026-10-08): the §2.3 surface and the §2.2 delta
  are the confirmed revision (`law1-lookupservicebound.md` §2.2 digests), and
  `scripts/check-lookupservicebound-law1-digests.sh` fails on drift. A future change requires the
  ABI process again (`CONTRIBUTING.md:70-71`, `docs/code-standards.md:40-45`, ADR-0013 decision
  #8).
- No change to §2.4. `LocalEndpoint::call` and `ServiceRef::call` still use the synchronous
  masked send/reply path; moving them onto the bounded primitive is C2C Phase 03 work, with its
  own dependencies and evidence ceiling. Freezing the opcode says nothing about what is
  *reachable*: the witness is the only caller.
- Landed and in scope: the non-activating witness
  (`cells/tests/bench/src/scenarios/local_service_lifecycle.rs`,
  `scripts/build-x86_64-c2c-lifecycle-ci.sh`,
  `tests/integration/tests/local-service-lifecycle-x86.rs`), the §2.2 registry record, the §2.3
  opcode, the §2.5 guards, and the evidence files referenced above. The kernel paths in scope are
  exactly those the granted handoff named: `kernel/src/cell/service_registry.rs`,
  `kernel/src/task/syscall.rs`, `kernel/src/cell/hotswap.rs`, `kernel/src/task/scheduler.rs`. No
  other kernel path is touched.
- No remote enablement, no Spec 20 ratification, no change to Spec 17, and no claim that
  `ServerEpoch` is now sound. A local `(cell_id, generation)` is not a remote replay epoch.
- No tier change: Phase 02 of the C2C plan still requires its own host + single-guest QEMU
  evidence before any Tier-1/Tier-2 routing claim.

## 6. References

| Item | Location |
|---|---|
| Phase-01 blocker this record answers | `.agents/260927-1100-c2c-anywhere-tier-aware/phase-01-contract.md:52`, `:76-90` |
| Local binding review handoff | `.agents/260927-1100-c2c-anywhere-tier-aware/phase-01-contract.md` § Local binding handoff |
| Contract draft | `docs/specs/20-unified-ipc-contract.md:14`, `:30`, `:35`, `:94`, `:117` |
| Wire law (unchanged by this decision) | `docs/specs/17-ipc-wire-contract.md` |
| Law-1 procedure | `CONTRIBUTING.md:70-71`, `docs/code-standards.md:40-48`, `.agents/260712-1900-manifest-v2/phase-00-law1-confirm-gate.md:37,73` |
| Law-1 record for opcode 429 (checkpoint 1 recorded) | `.agents/260927-1100-c2c-anywhere-tier-aware/law1-lookupservicebound.md` |
| Law-1 record format precedent | `.agents/260913-2002-g2-level-a-ai-inference/law1-confirmation.md` |
| Additive-syscall precedent | `docs/project-changelog.md:3750-3766` |
| Generation axis consumers | `libs/api/src/abi/caller_identity.rs`, `libs/api/src/abi/cell_owner.rs`, `kernel/src/task/async_ipc.rs`, `kernel/src/task/ipc_wire.rs` |
| Observed evidence (x86_64 QEMU) | `docs/evidence/local-service-lifecycle-x86-qemu.{txt,log}`; scenario `cells/tests/bench/src/scenarios/local_service_lifecycle.rs`; image `scripts/build-x86_64-c2c-lifecycle-ci.sh`; test `tests/integration/tests/local-service-lifecycle-x86.rs` |
| Guard evidence (AArch64 test-hooks QEMU) | `docs/evidence/task-id-reuse-guard-aarch64-qemu.{txt,log}` |
| Pre-existing x86 test-hooks failure (not this slice) | `docs/evidence/atomic-publication-x86-pre-existing-failure.{txt,log}`; open item in `.agents/260927-0739-kernel-architecture-repair/plan.md` |
