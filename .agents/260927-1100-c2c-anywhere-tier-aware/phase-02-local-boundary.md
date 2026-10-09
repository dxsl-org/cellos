---
phase: 2
title: "Correct local Tier-1/Tier-2 routing"
status: pending
priority: P1
effort: "implementation gate"
dependencies: [1]
tier: thinking
---

# Phase 02: Correct local Tier-1/Tier-2 routing

## Overview

Make local requests obey the tier-aware contract without inserting `net-broker` into the hot path. Deliver an actual named service exchange between a trusted Tier-1 Cell and an admitted private-root Tier-2 Cell, with copied IPC and correct caller identity. Preserve existing Tier-1 behavior; do not advertise ring/grant as the default.

## Requirements and architecture

- Resolve service ID to a *live registered binding* (Cell ID, generation, current TID, isolation domain) through the existing service registry. Bind calls and replies to the same incarnation; a stale late reply must not be accepted after respawn. SDK address construction must not let caller-declared tier grant fastpath privileges.
- Local Tier-1↔Tier-1 and any Tier-2 boundary use sender-masked typed IPC (Spec 17); domain-aware `copy_from_user`/`copy_to_user` remains the only Tier-2 data path. `DomainGrant` to a private root is forbidden until Spec 22's owner/revoke/TLB/DMA state machine lands (`docs/specs/22-native-domain-cell-implementation-gate.md:126-159`). Ring raw address token (`libs/ostd/src/ring_channel.rs:184-208`) remains limited to explicitly trusted fixture/benchmark paths.
- Preserve kernel-attested sender TID/Cell ID/generation and service-side authorization, independent of copied versus later shared transport (`kernel/src/task/ipc_wire.rs:19-36`; `cells/services/vfs/src/main.rs:126-150`). No guest or remote target can be silently resolved as a local Tier-1 service.
- Maintain bounded 4096-byte messages, explicit admission failure and correct sender-mask; never turn a blocked sender into a drop-on-not-ready `sys_try_send` call.

## Related files

- Modify if needed: `libs/ostd/src/cluster_endpoint.rs`, `libs/ostd/src/ipc.rs`, `kernel/src/cell/service_registry.rs`, `kernel/src/task/{ipc_wire,syscall}.rs`, `kernel/src/loader/domain_admission.rs`.
- Integration fixtures: existing native-domain QEMU tests and `scripts/run-c2c-broker-oracle-qemu.sh`; add one real cross-tier named-service exchange in the existing integration suite, not a source-text/wiring assertion.

## Implementation steps

1. Record existing local service lookup/callers and receiver allowlists; create a precise binding lookup with generation validation only where needed. Keep direct local `LocalEndpoint` use, preserve `recv(service_tid)` and attested receiver context.
2. On RV64 with qualified private roots, run Tier-1→Tier-2 and Tier-2→Tier-1 typed requests carrying nontrivial payload and reply. Deny wrong-user-buffer mapping, stale generation, unauthorized service method and oversize frames *before* delivery; show Tier-1↔Tier-1 existing service calls remain unchanged.
3. Test restart during queued/in-flight local work: old reply cannot satisfy new binding; record deterministic `Busy`/target-gone/indeterminate semantics consistent with the Phase-01 matrix, without silently replaying side effects.
4. Keep tier/guest disambiguation local to resolver/kernel binding; avoid adding one `match tier` into every application's hot path. Benchmark existing IPC and the new copied cross-tier path separately with source/build provenance, concurrency and p99 tails.

## Success criteria

- [ ] QEMU RV64 real Tier-1↔Tier-2 service request and reply pass with authenticated owner, invalid-buffer rejection and stale-reply exclusion; an ineligible architecture/profile refuses Tier-2 admission rather than silently launching in SAS.
- [ ] The existing local C2C broker oracle and normal typed VFS request/reply still pass, with measured source-bound before/after p99 and no watchdog misses; no unmeasured improvement claim.
- [ ] No private-root grant, raw shared pointer or remote broker fallback is used by cross-tier IPC.

## Assumptions

- **Claim:** Existing service registry yields enough generation/domain metadata for unambiguous new binding. **Confidence:** medium. **Verify:** inspect registry owner/generation and restart tests before adding an ABI; keep a local trusted lookup if possible.

## Security considerations

Caller cannot submit tier or `TrustedShared` directly. Copy validation spans entire private mapping, not only the first byte; do not accept domain pointers into SAS/kernel/peer mappings. Preserve service ACL independent of sender transport.

## Risk assessment and rollback

Cost: a tier dispatch or binding lookup could regress Tier-1 hot calls. Preserve old direct local copied path and disable only the new tier-aware resolver on regression; no on-disk state changes. A delivered request's side effects cannot be rolled back by changing the resolver—use request IDs and explicit unknown-outcome reporting. Do not relax Tier-2 fail-closed gate to force a green oracle.

## Success-criterion evidence (2026-10-09)

- **Criterion 1** (QEMU Tier-1↔Tier-2 request/reply with authenticated owner, invalid-buffer
  rejection and stale-reply exclusion; an ineligible profile refuses Tier-2 admission): **met**.
  On the Intel test-hooks lane: registry-**named** resolution carrying the provider's real
  `(cell_id, generation)`, both directions with a verified payload and checksum, oversize
  refused before delivery, unauthorized method refused, the stale descriptor refused after
  the provider exits, and the production lane still const-asserting Tier-2 denial.
  The invalid-buffer clause is witnessed on the syscall copy path by `tier2-rpc-driver`
  under a reviewed unsafe-allowlist exemption (`[tier2-rpc] INVALID-BUFFER=REFUSED`), and
  for the address-containment class by `/bin/tier2-exploit` (NULL write page-faults and is
  contained) and the kernel's guarded `copy_from_user` ledger probe. Evidence:
  `docs/evidence/c2c-cross-tier-exchange-x86.{txt,log}`,
  `docs/evidence/c2c-named-tier2-service-x86.{txt,log}`.
- **Criterion 2** (existing local oracle and typed VFS request/reply still pass — measured
  source-bound before/after p99, no watchdog misses, no unmeasured improvement claim): met. The
  broker oracle was re-run twice on the tree carrying slices A and B and the Phase-03 opt-in API and
  compared with the pre-slice run: the lane's own calibrated `direct_ipc_ref_ns=147000` is identical
  in all three (`calibration=MEASURED`), all sweeps `success` with `busy/indeterminate/duplicate/stale`
  at zero, soak 10000/10000 `silent_drop=0`, and `watchdog_expired_delta=0` with
  `heartbeat_miss_delta=0` throughout. Normalized p99 (× the reference) is at or below the before run
  at every n≥2 and the two after runs' spread (~7% at n=8) brackets the difference, so the result is
  reported as **no regression**, not as an improvement. Table and logs:
  `docs/evidence/c2c-broker-oracle-qemu-local.txt` (`…-after-1.log`, `…-after-2.log`). Typed guest
  VFS request/reply suites green on the AArch64/RV64/x86_64 lanes.
- **Criterion 3** (no private-root grant, raw shared pointer or remote broker fallback in cross-tier
  IPC): met — the fixture's syscall manifests hold `Log/Exit/Send/Recv/Yield/LookupService` and
  `Log/Exit/Send/Recv/LookupService` only, no capability bits, and both directions go through the
  sender-view/receiver-view copied path; the lane's markers show no grant or ring involvement.

## Deviation log

None.

## Next acceptance scenario (identified 2026-10-09; slice A admitted, slice B pending)

### Target, re-pointed to Intel

[ADR-0022](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md) makes Intel x86-64 the sole
active program, so this phase's success criteria are read on the Intel target rather than RV64 (the
RV64 native-domain lane stays the historical qualified-root reference, not the program's oracle).
The qualified safe-root/admission evidence the plan requires on "the exact architecture/profile" is
the x86_64 **test-hooks** domain lane: it admits `/bin/tier2-smoke` and `/bin/tier2-exploit` to a
Paged Domain with CR3 isolation (`[domain] admitted cell … (CR3 isolation)`, `S22-X86-DOMAIN-LIVE`)
and, since 2026-10-08, boots to its own end (`docs/evidence/atomic-publication-ledger-x86-settling.{txt,log}`).
Shipping Intel admission stays refused by design ([Spec 22](../../docs/specs/22-native-domain-cell-implementation-gate.md)), so the exchange below is a test-image witness and the production
lane must keep asserting the denial — the two lanes must disagree.

### What the exchange would witness

One real named-service exchange in both directions between an admitted private-root Tier-2 Cell and a
Tier-1 Cell, with a nontrivial typed payload and reply, and four refusals **before** delivery: a wrong
user buffer, a stale provider generation, an unauthorized service method, and an oversize frame —
plus unchanged Tier-1↔Tier-1 typed calls and no private-root grant, raw pointer or remote broker
anywhere in the path.

### What exists (re-derived from the tree, 2026-10-09)

- A private-root task already uses the ordinary copied IPC: nothing refuses `TaskAddressSpace::Domain`
  in `Send`/`Recv`/`TrySend`, and the copy views are chosen per direction — sender view out
  (`kernel/src/task.rs:2175-2178`), receiver view in (`:2380-2383`, `:2470-2473`) through
  `kernel/src/task/copy_glue/mod.rs:65-104,211-268`. Admission itself already requires copied IPC
  (`kernel/src/loader/domain_admission.rs:278-284`).
- Refusals that already exist and fire before delivery: oversize (`kernel/src/task/ipc_wire.rs:13-17`;
  `task.rs:2161-2163,2670-2685`), bounded queue/backpressure (`task.rs:2240-2242`), a paused or dead
  provider (`task.rs:2170-2172,2214-2216`; `kernel/src/cell/service_registry.rs:225-233`), a wrong
  user buffer (domain ledger/PTE probe plus the guarded copy, `kernel/src/task/user_copy/mod.rs:4-25`,
  `copy.rs:61-89`), and a stale peer incarnation for wake and async completion
  (`task.rs:2528-2544`; `kernel/src/task/async_ipc.rs:127-143,157-161`). The sender's
  `(cell_id, generation)` rides in the wire header (`ipc_wire.rs:23-30`).
- The bound lookup exists on both sides but is unused by callers: the kernel returns only an
  active, nonzero identity (`service_registry.rs:131-145`; `kernel/src/task/syscall.rs:5658-5686`) and
  `ostd` has the 24-byte wrapper (`libs/ostd/src/syscall.rs:1027-1057`), while `ServiceRef` still
  caches a tid from the legacy `LookupService` (`libs/ostd/src/service.rs:67-136`) and
  `LocalEndpoint::call` goes straight to `service_call_typed(tid, …)`
  (`libs/ostd/src/cluster_endpoint.rs:86-102`).

### What is missing

1. **No caller-side binding consumer.** A stale *service* binding is not detectable today; only the
   wake/async paths check the peer incarnation. Consuming `LookupServiceBound` on the caller side is
   this phase's step 1 and needs no new ABI — the opcode is implemented and frozen.
2. **No receiver service allowlist, byte-0 routing or per-method gate in generic local IPC.** Nothing
   of the sort happens before `queue_wire_msg` (`kernel/src/task.rs:2155-2277,2647-2755`);
   `CallerIdentity` is written *after* the payload copy as a trailer
   (`kernel/src/task/syscall.rs:2732-2752,3181-3196`), and byte-0 framing is an application convention
   (`libs/ostd/src/app.rs:11-15,303-345`). Method authorization is service-specific today, e.g.
   `cells/services/vfs/src/caller.rs:74-82` derives `may_mutate` from the attested flag. Whether the
   exchange needs a new kernel gate or a service-side check is an open design question for it.
3. **No image with both participants.** The x86 domain image admits only `/bin/tier2-smoke`
   (Log/Yield/GetTime/Exit/GrantRegister, `cells/tests/tier2-smoke/src/main.rs:40-136`) and
   `/bin/tier2-exploit` (Log/Exit/StateRestore, `cells/tests/tier2-exploit/src/main.rs:14-57`); neither
   sends, receives, resolves or registers, and the lane asserts admission/fault/teardown markers only
   (`scripts/x86/qemu-domain-test.sh:89-113,142-179,228-245`). The typed-IPC bench lane is the mirror
   image of the problem: it has the SDK surface but is production posture with no Tier-2 Cell, and its
   provider deliberately never replies (`scripts/build-x86_64-c2c-lifecycle-ci.sh:13-22,57-100`;
   `cells/tests/bench/src/scenarios/local_service_lifecycle.rs:68-108,158-220`).
4. **Packaging for a new `/bin` path.** A new fixture needs reviewed loader rows
   (`kernel/src/loader/launch_profile/targets.rs`, `profiles.rs`, `kernel/src/loader/boot_ceiling.rs`)
   and init launch ordering (`cells/tools/init/src/boot.rs:227-236`), plus the image builder and its
   marker assertions. The existing markers and the smoke/exploit fixtures stay as they are — admission
   and fault containment are not re-purposed into an RPC witness.

### Two slices, in the order this phase's own steps take them

| Slice | Deliverable | Why first / gate |
|---|---|---|
| **A — step 1: caller-side binding** | Resolve local service calls through the frozen `LookupServiceBound` binding: `ServiceRef`/`LocalEndpoint` bind a live `(cell_id, generation)` and keep `recv(service_tid)` + attested receiver semantics; a stale binding is refused rather than used. | Needs no new ABI, no new `/bin` path and no kernel IPC change: host-testable (`libs/ostd/tests/`), plus the x86 c2c-lifecycle lane (`VFS-BINDING … matches_lookup=true`) and the local broker oracle. It is what makes the stale-generation negative of slice B writable. |
| **B — step 2: the cross-tier exchange** | One IPC-capable admitted Tier-2 fixture (named provider) plus a Tier-1 driver, on the x86 test-hooks domain lane, witnessing the two directions and the four refusals above. | Consumes slice A; touches new cell fixtures, loader rows and the lane's assertions, so it needs its own review and the lane must stay green end to end. |

Both slices are copied-IPC only: no `DomainGrant` to a private root, no ring, no raw pointer, no remote
path. Nothing above is activated by this note — implementation starts only when the portfolio admits
the slice.

## Slice A progress (2026-10-09) — implemented and verified

Admitted 2026-10-09 with the Phase-01 exit sign-off, and landed the same day. The SDK now consumes the
binding the kernel already returns:

| Item | Location | Evidence |
|---|---|---|
| `ServiceRef` caches and resolves the binding `{tid, cell_id, generation}` through `sys_lookup_service_bound`; failures are classified against the registry (`NotFound` when the descriptor is stale, `IO` when the same live endpoint failed) | `libs/ostd/src/service.rs` | `SDK-BINDING matches_raw=true`, `SDK-VFS-CALL=OK` |
| `binding()` and `is_live()` — resolve the whole descriptor; answer whether the held descriptor is still the live one | `libs/ostd/src/service.rs` | `SDK-BINDING-LIVE resolved=true unresolved=false` |
| `LocalEndpoint::bind()` resolves the method's service and refuses with `EndpointError::NoLiveBinding`; `new(tid)` stays identity-free and keeps `IO` | `libs/ostd/src/cluster_endpoint.rs` | `SDK-ABSENT-BINDING=REFUSED` |
| The rule, pure and host-tested: whole-descriptor match, including the tid (a Cell can re-register under a new tid without its generation changing) | `ostd::service::classify_call_failure` | `libs/ostd/tests/cluster-endpoint.rs` |

No new ABI, no new `/bin` path, no kernel IPC change, `LookupService = 206` untouched.
Evidence: `docs/evidence/c2c-sdk-binding-x86.{txt,log}`; the lane is the x86_64
`local-service-lifecycle` witness, whose leg S exercises the SDK itself, and the AArch64/RV64 `test-hooks`
suites plus the local broker oracle were re-run with the change.

**Not in this slice:** the stale-after-death half had no runtime witness at the time — it needed a
*registered* provider that dies, which arrived with the named Tier-2 service below
(`STALE-BINDING=REFUSED`, `docs/evidence/c2c-named-tier2-service-x86.{txt,log}`); `libs/ai-sdk` keeps
its own coarse transport error mapping; and `LocalEndpoint::call` still uses the synchronous path,
which Phase 03 owns.

## Slice B progress (2026-10-09) — implemented and verified

Admitted 2026-10-09 (owner: same C2C session) and landed the same day. The x86_64 test-hooks domain
lane now runs a real cross-tier exchange, which it could not before: its only two Tier-2 fixtures did
no IPC.

| Piece | Location | Evidence |
|---|---|---|
| One postcard record each way, plus the FNV-1a checksum and the fixture's deliberately unregistered service id | `cells/tests/tier2-rpc-proto` | both cells share it, so they cannot drift |
| Tier-2 provider: `PROTECTION_CLASS_UNTRUSTED`, capability-free, `Log/Exit/Send/Recv/LookupService`; serves two requests then exits | `cells/tests/tier2-rpc-provider` | `[domain] admitted cell 'tier2-rpc-provider' to Tier 2 Paged Domain (CR3 isolation)`, `provider-served 2` |
| Tier-1 driver: reads the provider tid from the argv `init` stashed, drives every leg | `cells/tests/tier2-rpc-driver` | `driver-start provider=9` … `DRIVER-DONE` |
| Boot order: provider first, then the driver handed the tid through the reviewed argv stash | `cells/tools/init` (`tier2-rpc-entry`) | `Init: tier2-rpc-provider admitted.` / `Init: tier2-rpc-driver launched.` |
| Empty-cap loader rows for both paths | `kernel/src/loader/launch_profile/{profiles,targets}.rs`, `kernel/src/loader/boot_ceiling.rs` | the lane's markers |
| Image packaging + asserted markers, including the refusals' *absence* forms | `scripts/build-x86_64-domain-test-ci.sh`, `scripts/x86/qemu-domain-test.sh` | `PASS: x86_64 Tier-2 domain-entry lane … every marker present` |

Witnessed: **Tier-1 → Tier-2** (typed 512-byte payload, length *and* checksum verified);
**Tier-2 → Tier-1** (the private-root provider calls the named VFS service through `ServiceRef`, i.e.
through the frozen binding, and reports `is_dir=true` in its reply); **oversize frame** refused by the
kernel before delivery while the provider was still waiting (it never accounted for it);
**unauthorized method** refused by the receiver with no side effect; **stale descriptor** refused once
the provider exited, with no retry onto another incarnation. The Tier-2 side later became
registry-**named** (see limitation 1): its provider is registered by `init`, the driver resolves it
through the SDK, and the stale rule is witnessed on a *bound* descriptor. Full evidence:
`docs/evidence/c2c-cross-tier-exchange-x86.{txt,log}` and
`docs/evidence/c2c-named-tier2-service-x86.{txt,log}`; AArch64/RV64 `test-hooks` re-run with the shared
kernel rows.

**Not claimed here, and why:**

1. ~~**A named Tier-2 service.**~~ **Closed 2026-10-09.** `RegisterService` is `SpawnCap`-gated, so a
   private-root Cell cannot register *itself* — but its **spawner** can, which is the pattern init
   already uses for the hypervisor. `init` now registers the fixture's provider under its service id
   and the driver resolves it through the SDK, so the exchange is registry-named and the descriptor
   carries the provider's real `(cell_id, generation)` (`PROVIDER-BINDING tid=9 cell=5 gen=142`,
   `PROVIDER-NAME-MATCHES-RAW=true`). That needed no new authority, no ABI and no kernel change. It
   also gave slice A's stale rule its first runtime witness: after the provider exits, the cached
   descriptor is refused rather than re-targeted (`PROVIDER-GONE=none`, `STALE-BINDING=REFUSED`,
   `STALE-BINDING-CLEARED=true`). Evidence: `docs/evidence/c2c-named-tier2-service-x86.{txt,log}`.
2. **A wrong user buffer on the syscall copy path.** Of the four pre-delivery refusals, this is the one
   not re-created: a `#![forbid(unsafe_code)]` Cell cannot fabricate a pointer, and the
   address-containment witness for that class already runs as `/bin/tier2-exploit` on the same lane.
   A raw-pointer fixture would need its own unsafe-allowlist entry and review.
3. **The unauthorized-method gate is receiver-side.** Generic local IPC has no per-method or byte-0
   routing to enforce it in the kernel; that remains the open design question for the remote profile.
