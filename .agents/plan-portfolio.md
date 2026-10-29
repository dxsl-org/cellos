# Cellos plan portfolio

**Status:** Canonical scheduling index
**Updated:** 2026-10-11 — Cell/Kernel Cell/authority and scoped cluster placement/autoscaling contract cutover owns Intel C2C execution scheduling

This file owns scheduling intent. Source/tests own implementation truth; individual plan
files preserve detailed scope and provenance. Untouched checkboxes are not proof that code
is absent. [ADR-0022](../docs/decisions/0022-intel-x86-64-c2c-only-direction.md)
owns direction; this index owns child-plan scheduling. Every task must name a
direct Intel C2C deliverable, required prerequisite, measured bottleneck or
regression/security/build repair protecting that path, plus its acceptance
scenario and evidence ceiling. An old plan's `active`/`ready` status is not
authorization. Plans not admitted below are parked/historical.

## Sole contract-cutover owner — CELL-CONTRACT-0024

The [Cell contract and authority plan](261011-1200-cell-contract-authority/plan.md)
is the sole owner of unfinished Cell/Kernel Cell/authority contract cutover within
[ADR-0022](../docs/decisions/0022-intel-x86-64-c2c-only-direction.md).
[ADR-0024](../docs/decisions/0024-cell-kernel-authority-contract.md) records the
Cell-centric direction. Current admission includes the user-requested
[scoped G1/G2 prerequisite authorization](261011-1200-cell-contract-authority/scoped-g1-g2-authorization.md):
G1 Intel-private edits followed by G2-R-core/B disposable-image edits only.
WIP remains one implementation slice; actual-diff and pre-capture gates remain.
G2-J, downstream ABI ratification, remote activation and production stay blocked.

**COMPLETED MILESTONE (2026-10-16):** The [Cell contract and authority plan](261011-1200-cell-contract-authority/plan.md) has successfully executed all 10 phases (P01 through P10), verified all native tranches (N02-A through N07), resolved G3 Durable Charge gates (83/83 PASS), and received explicit governance ratification for both Law-1 Checkpoint 1 (CP1 Wire ABI) and Checkpoint 2 (CP2 Implementation Qualification). All 38 execution locks remain actively maintained pending planned slice-by-slice retirement.

- The [lock inventory](cell-contract-0024-locks.md) identifies every considered
  plan, preserves prior progress, scopes each new block and maps replacement phase
  areas. Entry-point `blockedBy`, `executionLock` and `lockScope` fields are
  scheduling relations, not claims that completed implementation stopped working.
- **Precedence applies to direct phase-file navigation too:** a locked plan's
  unfinished subordinate phase, checklist, old cook command, file-owner grant,
  `ready`/`active` label or earlier admission cannot bypass `CELL-CONTRACT-0024`.
  Completed evidence is read-only input at its original ceiling.
- The new plan consumes completed evidence, not execution of locked legacy plans.
  Needed unfinished prerequisites are adopted as named slices inside its phases:
  P02 Cell/Kernel CSpace, P03 threads/SMP/CPU budgets, P04 operations/wait/completion,
  P05 resource/delegation/device authority, P06 lifecycle/state/crash recovery,
  P07 optional POSIX/runtime, P08 remote authority and placement/autoscaling with
  durable application-wide budgets, P09 Intel VMX/guest bridge and P10 exact
  physical/profile qualification and concurrent local/cluster workloads. P01 owns
  exact contract/ABI/manifest/lifecycle/budget inventory and adoption. The existing
  ten-phase DAG remains pending; this avoids a circular prerequisite/unlock dependency.
- **No automatic unlock:** documentation completion, a green old test, or finishing
  the controlling plan does not release a legacy slice. Record explicit per-slice
  rebase against the new contracts, file-owner handoff, retained technical/approval
  gates, acceptance evidence and scheduling selection in this index and the
  controlling plan; update only that slice's lock relation in the same change.
- Law 1 still requires its two approvals for each concrete affected ABI surface.
  Existing frozen surfaces and no-ABI authorizations are not broadened. Protected
  identity/nonrollback state, independent human, physical-device, signing and
  production gates remain separate. Non-Intel and unrelated product work remains
  parked under ADR-0022; the lock does not create a new blanket maintenance ban.

Approved placement is an application-policy controller Cell, not kernel scheduling
across machines. One Cell belongs to one node (one physical/VM kernel instance),
and a cluster has multiple nodes. The broker owns bounded peer sessions/reconnect/
direct-relay choice; service-net owns IP/TCP and protected authority owns trust.
Independent fixed-home Budget Authority enforces the durable application vector;
controller owns placement/scaling policy and membership, not allowance minting.
Issuer precommits genesis before control spawn; destination live atomic claim
precedes child publication. Unknown/draining children remain charged until terminal
key closure. Challenge freshness, old-generation reconciliation and homogeneous
scaling domains are mandatory. Spec 25 v1.3 covers AC-01..AC-50 with approved
earlier semantic amendments. Ten phases, 38 locks and protected/ABI gates remain;
no consensus, DSM, timeout promotion or implicit fallback.
P10 measures actual hundreds of concurrent clients, not Cell counts or guaranteed
CPU/throughput capacity. No historical evidence below is upgraded by this amendment.

[Spec 26](../docs/specs/26-workload-placement-scaling-contract.md) is the detailed
workload architecture: logical versus pinned endpoints, bounded weighted routers,
deterministic constrained placement, nested application shares and a measured-demand
control loop. P08's internal ordered slices preserve the ten-phase DAG; P10 proves
load-triggered creation/routing/drain, not pre-spawned replicas alone. Router and
control-plane capacity is budgeted; no per-request controller hop or durable ledger write.

## Deferred application consumer — Haily

The owner selected [Haily personal assistant](../docs/roadmap/haily-personal-assistant.md)
as the planned first flagship Cellos application, covering coding, scientific
research, writing and other user-directed workflows rather than only chat.
The existing Rust/Windows project remains at `/mnt/d/haily/`; Cellos port and
integration wait for owner-accepted completion of Haily's first version.

During that wait, only admitted Cellos substrate work continues under the
controlling plan. The roadmap maps future application needs to P02–P10 and
records release review, single-node, distributed, physical and deployment
stages; it is not an executable child plan or a second WIP lane. Completion of
Haily's first version requires an exact release/behavior/dependency review and
subsequent scoped entry/handoff before implementation. No legacy lock, ABI,
protected authority, physical gate or production admission is released here.
## Active desktop plan — Unified desktop, native sessions and shared Linear theme

**Recorded 2026-10-19:** The [Unified desktop plan](261019-1310-unified-desktop/plan.md)
implements approved [ADR-0027](../docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md)
outcomes on Intel x86-64 across 11 phases. Phase 01 ([phase-01-contract-baseline.md](261019-1310-unified-desktop/phase-01-contract-baseline.md))
is **completed**:
- Created [abi-delta-ledger.md](261019-1310-unified-desktop/abi-delta-ledger.md) specifying D-UI-01 through D-UI-10 candidate deltas, wire layouts, opcodes, error models, candidate specification digest `e47ffa52fc7d11ffe190bda41dc762d60f8d53131809562369b040456b2da6fb`, and Law-1 Checkpoint 1 (Design Approval) pending candidate ratification before code mutations in `libs/api`.
- Created [baseline-manifest.md](261019-1310-unified-desktop/baseline-manifest.md) capturing truthful source baseline (commit `4994602b55a43f3fd4262f36dc3dfaa7b1a2ff44`), complete SHA-256 hash inventory of dirty tracked and untracked artifacts, preserving existing user changes.
- Materialized and verified integration test target `tests/integration/tests/desktop-x86.rs` and `boot_x86_bios_with_accel` helper in `tests/integration/src/lib.rs` for automated x86 QEMU KVM/TCG boot and QMP interaction.
- Reconciled ADR-0027 scope and contracts in `contracts.md`. No ambient scope widening, G2-J crash format promotion, or unadmitted capability changes authorized.


## Cellbox tooling — Phases 01–10 at private evidence; dev-posture x86 promoted (S10-5 CP2 granted 2026-10-29); production/physical gates BLOCKED

**Updated 2026-10-26** (supersedes the 2026-10-20 status text of this section). The maintainer approved [ADR-0028](../docs/decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md) and the [Cellbox implementation plan](261020-1750-cellbox-cli/plan.md), and later approved continuing the candidate design; that consent is preserved. The design is one modular essential-suite executable (`/bin/cellbox`, entry `sh`), typed pipelines, a compact editor, ripgrep-derived search, a Cell-aware top, outbound SSH, and a reusable Terminal SDK/Ratatui backend that is independent of viUI.

Phase states, all on private signed QEMU/TCG images only ([plan.md](261020-1750-cellbox-cli/plan.md) phase table):

- **01 catalog/contract gates:** completed-private-evidence (2026-10-26). Criteria C1–C4 are checked in the [phase file](261020-1750-cellbox-cli/phase-01-baseline-contracts.md); the [closure dossier](261020-1750-cellbox-cli/phase-01-closure-dossier.md) §7 holds the gap list.
  - The catalog is frozen by the user at SHA-256 `f2a82fce61101f9f1dd6d62a75bece735e3eb9129a2295b187c2f49209485339` (2026-10-25).
  - The [reconciled delta index](261020-1750-cellbox-cli/contract-deltas.md) records owners, CP1s and CP2s. The integrating owner was confirmed by the user for P03–P05, P08 and P09.
  - The scoped `cargo deny` for the cellbox graph passes; CBX-01 and CBX-02 are met (Phase 10 still audits).
  - The Q Config per-key write right for `export` ([proposal](261020-1750-cellbox-cli/phase-01-config-right-cp1.md), option Q1) has CP1 (2026-10-25) and CP2 (2026-10-26, deviations 1–2). The `export` denial is lane-proven (P04 466/466, 6 config markers). `/bin/config` ships only with feature `ai`; without it `export` reports `config unavailable`.
- **02 Terminal SDK and real child/session streams:** completed-private-evidence; CP2 2026-10-25.
- **03 typed pipelines:** completed; CP2 2026-10-21; private-image evidence.
- **04 single artifact and caller/image cutover:** completed-private-evidence (2026-10-29, S10-5 CP2 granted by the user). Every Phase 04 delta is CP2-approved: eight slices on 2026-10-22, and S1/S2-lite/S2c-min/S3-edge on 2026-10-25 through the [mini-dossier](261020-1750-cellbox-cli/phase-04-s1-s3-cp2-dossier.md). Criterion 4, canonical one-artifact packaging (including `/bin/cellbox-ssh`), is checked on the promoted dev-posture x86 bytes only (QEMU/TCG; non-Intel recipes not covered).
- **05 Ratatui backend and GUI-terminal emulation:** completed-private-evidence; CP2 2026-10-23.
- **06 Cell top and telemetry:** completed-private-evidence; CP2 2026-10-24. The D60-3 clock-unit defect is open.
- **07 ripgrep search (`libs/vigrep`, ripgrep 15.2.0):** completed-private-evidence; CP2 2026-10-24.
- **08 compact editor with D03 conditional save:** completed-private-evidence; S8-5 CP2 2026-10-25.
- **09 outbound SSH (vendored Sunset 0.6.0):** closed-partial-private-evidence (user, 2026-10-25). The Tier-3 guest is blocked, and S9-5 publickey/KMS custody is deferred (D79-14).
- **10 integrated signed-image qualification and footprint:** closed-partial-private-evidence (2026-10-30, S10-9 CP2 and CBX-27 disposition granted by the user; S10-5 CP2 2026-10-29). Candidate `4fae8b20…` qualified (S10-3 CP2 2026-10-28), S10-4 closed partial, S10-5 promoted the candidate's desktop bytes to the canonical x86 paths (hash-identical; V1–V7 PASS), S10-7..S10-9 measured candidate headless/desktop (class A) and a non-equivalent B2 baseline on test-hooks kernels. Criteria 2, 3, 4, 5 checked (criterion 4 and CBX-27 at their stated ceiling by user disposition, not a feature-equivalent comparison); criterion 1 open: CBX-23/24/25 partial.

Latest private lanes (Config CP2 evidence, 2026-10-26): P04 terminal 466/466, GUI 290/290, P03 67, P02 48, kernel lane with 7/7 VFS self-tests, Phase 09 103/103. Rows other than those recorded as met in [acceptance.md](261020-1750-cellbox-cli/acceptance.md) remain pending until their phases and Phase 10 qualification.

**Dev-posture x86 rollout is PROMOTED, not released to production (S10-5 CP2 granted by the user 2026-10-29).** The [rollout containment record](261020-1750-cellbox-cli/reports/rollout-containment.md) stays authoritative (new section "Update 2026-10-29: promotion (S10-5)"):

- the canonical `kernel_fs.img`, `init`, boot-config and default kernel hold the qualified desktop bytes (`3848cefb…`, `c0bd1458…`, `dfbe8c55…`); the quarantined bytes are archived outside the repository, not deleted, and are not a safe boot set;
- the desktop packager's exit and the default-kernel ISO refusal were replaced by content rules (S10-2b); the quarantined-hash guard (G2b) is permanent; cloud images and non-Intel recipes stay refused;
- private-fixture signing still needs explicit opt-in, and the dev-key canonical route is admitted only for the pinned assembler into exactly `kernel/src/embedded-x86_64`.

A signature proves neither qualification nor permission to deploy. Physical Intel, VMX, production and remote gates are independent and not claimed. **All 38 CELL-CONTRACT-0024 lock relations remain; S10-5 narrowed none.**

Scope supports Intel Cellos operations only. No IDE/build environment, general Linux command inventory or independent application ecosystem is selected.

## Parked research — VFS, storage and agent search

**Recorded 2026-10-19:** [VFS/storage/search research plan](261019-1534-vfs-storage-search-research/plan.md)
preserves the BFS/RedoxFS/modern-filesystem discussion, native-format source findings,
measurement proposals and unresolved owner decisions. Analysis/research is unfinished
and deliberately paused for owner consideration. No backend, disk format, index engine,
VFS ABI or consistency policy is selected. This is documentation-only registration,
not an implementation admission, new WIP lane or release of any existing execution lock.
Resume requires an explicit owner request; implementation requires separate scoped approval.

## Historical Intel C2C execution ledger — not current admission

The remainder of this ledger preserves completed work and prior admissions.
Its `primary`, `next`, `admitted`, `authorized` and owner-handoff wording is historical
where it would authorize unfinished locked execution. Use the controlling plan,
scoped lock inventory and promotion rule, not those older statements, to schedule.

- `260927-1100-c2c-anywhere-tier-aware` — **former primary execution plan; unfinished
  execution locked**. [Plan](260927-1100-c2c-anywhere-tier-aware/plan.md).
  Next executable scope is Phase 01 contract inventory/review and non-activating
  evidence preparation. Kernel-owner review, contract ratification and Law-1
  approvals still precede affected implementation. This direction decision does
  not ratify Spec 20 or enable any remote route. Local/remote, asynchronous,
  trusted fastpath and guest adapters retain separate gates.
  Relay Phases 04–06 remain blocked on protected authority evidence; source
  incarnation must be protected/nonrollback or separately ratified, not uptime.
  Overlapping kernel/syscall/domain/grant/scheduler work requires an explicit
  ownership handoff from the kernel-repair owner after its applicable gates.
- Required child work is admitted only for this program; it is not permission
  to run all children simultaneously. **WIP: one implementation slice.**
- **Admitted preparatory slice (owner: current C2C implementation session):**
  non-activating broker ingress/quota prototype and local restart/stale-endpoint
  behavioral evidence, including an Intel x86 QEMU runner. No kernel ownership
  handoff, ABI ratification, protected-authority proof or remote activation is
  inferred. Prototype peer facts are harness inputs, not authentication evidence.
  **Landed 2026-10-08:** the ingress/quota decision core (host tests only) and the
  local lifecycle witness on Intel x86_64 QEMU
  (`scripts/build-x86_64-c2c-lifecycle-ci.sh`,
  `tests/integration/tests/local-service-lifecycle-x86.rs`,
  `docs/evidence/local-service-lifecycle-x86-qemu.{txt,log}`). The witness measures
  a synchronous masked reply wait that is never woken versus the bounded
  exact-operation terminal on the same dead peer. Witness only: no kernel, ABI,
  Spec-17 or production call-site change. Provider *restart* and the TID non-reuse
  invariant remain unmeasured.
- **Decision record admitted 2026-10-08:**
  [ADR-0023](../docs/decisions/0023-local-service-generation-binding.md) fixes the
  local binding axis as the existing per-Cell `(cell_id, generation)`, proposes
  exactly one additive opcode (`LookupServiceBound = 429`) with
  `LookupService = 206` untouched and no send opcode, and requires the
  task-id non-reuse invariant to be guarded. Recording the decision itself
  authorized **no** kernel, ABI or remote change: the registry record and guard
  sites need the kernel-repair owner handoff, and the opcode needed Law-1
  approval (see the next bullet).
- **Law-1 design approval recorded 2026-10-08** for the single additive opcode in
  [ADR-0023](../docs/decisions/0023-local-service-generation-binding.md) §2.3: item list,
  compatibility review and digests in
  [law1-lookupservicebound.md](260927-1100-c2c-anywhere-tier-aware/law1-lookupservicebound.md).
  This is checkpoint 1 only — it permits editing `libs/api/` for those items and freezes nothing.
  **Checkpoint 2 was recorded the same day, so the surface is now FROZEN**; the confirmed revision
  and its digests are in that record's §2.2, and
  `scripts/check-lookupservicebound-law1-digests.sh` fails on drift. A future change to a confirmed
  item needs the ABI process again.
- **Kernel-repair file-owner handoff granted 2026-10-08 (full slice)** and the work landed the same
  day: `libs/api` opcode `LookupServiceBound = 429`, the registry's `(tid, cell_id, generation)`
  record, provider-identity capture at every `register` site, dispatch, and the two
  `next_task_id` fail-closed guard sites plus a boot no-re-issue guard. Kernel paths touched are
  exactly those the handoff named. Verified on Intel x86_64 QEMU (production image) and on the
  AArch64 `test-hooks` lane. The x86_64 `test-hooks` lane, blocked at the time by a pre-existing
  frame-accounting panic, was unblocked on 2026-10-08 — the ledger needed warming to its fixed point
  before the case measured it (`docs/evidence/atomic-publication-ledger-x86-settling.{txt,log}`) — so
  the Intel domain lane now runs to its own end.
  `LocalEndpoint::call` is unchanged: Phase 03 owns moving the SDK onto the bounded primitive; Phase 02
  step 1 owns making the SDK *consume* the frozen binding.
- **Slice B admitted 2026-10-09 (same owner) — the cross-tier exchange**, still the only implementation
  slice in flight after A landed: a new Tier-2 fixture provider (`/bin/tier2-rpc-provider`,
  `PROTECTION_CLASS_UNTRUSTED`, capability-free) plus a Tier-1 driver (`/bin/tier2-rpc-driver`,
  capability-free), both launched by `init` under a **new** `tier2-rpc-entry` feature so no other
  image changes. The driver addresses the Tier-2 cell by the tid `init` hands it through the reviewed
  argv stash, and the Tier-2 cell calls the *named* Tier-1 VFS service through the SDK. Touches: two
  fixture crates, `cells/tools/init`, `kernel/src/loader/launch_profile/{profiles,targets}.rs`,
  `kernel/src/loader/boot_ceiling.rs`, `scripts/build-x86_64-domain-test-ci.sh` and
  `scripts/x86/qemu-domain-test.sh`. No new ABI, no new authority for any cell, no `RegisterService`
  from a private root, no remote route.
  **Landed 2026-10-09, evidence complete:** a new `tier2-rpc-entry` init feature (that image only)
  launches the Tier-2 provider and then the Tier-1 driver with the provider's tid through the
  reviewed argv stash; the driver's typed 512-byte request is answered with a verified length and
  checksum, the private-root provider calls the **named** VFS service through `ServiceRef` (slice
  A's binding) and reports `is_dir=true` in its reply, and the lane asserts the refusals as well as
  their *absence* forms: `OVERSIZE=REFUSED`, `UNAUTHORIZED-METHOD=REFUSED`, `PROVIDER-REGISTRY=NONE`,
  `STALE-PEER=REFUSED`. AArch64/RV64 `test-hooks` re-run green with the shared loader rows.
  Evidence `docs/evidence/c2c-cross-tier-exchange-x86.{txt,log}`.
  **Limitations recorded with the admission** (one since closed): a private-root cell cannot register
  *itself* (`RegisterService` stays `SpawnCap`-gated), but its **spawner** can — and `init` does, the
  same way it registers the hypervisor — so the Tier-2 provider became **registry-named** on
  2026-10-09 and the driver resolves it through the SDK (`PROVIDER-BINDING tid=9 cell=5 gen=142`),
  which also gave slice A's stale rule its first runtime witness (`STALE-BINDING=REFUSED`,
  `docs/evidence/c2c-named-tier2-service-x86.{txt,log}`). No new authority, ABI or kernel change.
- All four of the phase's pre-delivery refusals are witnessed: oversize frame,
  unauthorized method, stale descriptor, and wrong **user buffer** on the syscall
  copy path (exercised under a reviewed unsafe-allowlist exemption by
  `tier2-rpc-driver`: `[tier2-rpc] INVALID-BUFFER=REFUSED`). Address containment
  additionally runs as `/bin/tier2-exploit`.
- **Phase 02's success criteria are fully evidenced (2026-10-09).** Criterion 1
  on the Intel test-hooks lane (named binding, both directions, all four refusals,
  production denial still asserted); criterion 3 trivially from the fixtures'
  capability-free manifests and the copied-only path; and criterion 2 measured
  rather than assumed: the broker oracle re-run twice on the tree carrying
  slices A and B and the Phase-03 opt-in API, compared against the pre-slice run
  by its own calibrated direct-IPC reference (identical `147000` ns,
  `calibration=MEASURED` in all three), every sweep `success` with
  `busy/indeterminate/duplicate/stale=0`, soak 10000/10000 `silent_drop=0`, and
  `watchdog_expired_delta=0`/`heartbeat_miss_delta=0` throughout. Normalized p99 is
  at or below the before run at every n≥2 within the two after runs' own ~7% spread
  -> recorded as **no regression**, not an improvement.
  `docs/evidence/c2c-broker-oracle-qemu-local.txt` + the two after logs.
- **Owner-approved stranding cutover implemented:** SDK synchronous and bounded RPC
  use one kernel-owned operation lifecycle; immediate replies use the exact-context
  Send bridge, while nested/deferred VFS/KMS/NIC/net/broker providers retain tokens.
  Same-incarnation workers may reply; foreign generations and late/duplicate tokens
  may not. This supersedes the historical opt-in-only and exclusive-IpcReply
  statements below. No new ABI, completion source or remote activation.
  Evidence: `docs/evidence/c2c-stranding-cutover-x86.log`; the lifecycle regression
  now requires synchronous PeerGone, nested VFS/retained Reply, duplicate quarantine,
  oversized-error slot cleanup and post-dispatch cancellation correlation.
  Remaining gates: multi-source waiting, deterministic two-hart proof and full
  fairness/restart/abandoned-grant matrix.
- **Phase 03 bounded lifecycle proof slice exercised:** single-hart x86 QEMU
  dispatched 64 calls, refused overflow with no delivery, retained all terminal
  reservations until take and reused capacity. Real 3000-tick expiry produced
  queued PreDispatchTimeout/dispatched Indeterminate; four queued calls from a
  dead caller were removed before provider release and a fresh caller succeeded.
  Evidence: `docs/evidence/c2c-saturation-deadline-caller-death-x86.{txt,log}`;
  kernel 9 and lifecycle integration 2 tests PASS, independent serial smoke PASS.
  Production kernel/SDK and ABI unchanged. This closes those single-caller
  witnesses, not the full fairness/restart or abandoned-grant matrix.
- **Phase 03 provider replacement/raw-event witness and SDK fix:** old PeerGone
  retained across replacement, old token refused, fresh Reply and dead-submit
  refusal; third-Cell raw event received while RPC stayed pending, with no raw
  reply leak. The witness failed before fixing `PendingCall::wait_and_take`:
  unrelated retained terminals exhausted rounds immediately. It now measures
  scheduler-tick budgets and yields without consuming another result.
  Evidence: `docs/evidence/c2c-restart-event-coexistence-x86.{txt,log}`;
  ostd 67 PASS, lifecycle integration 2 PASS and independent serial smoke PASS.
  No ABI/kernel change; not registry rebind, hotswap or hardware input.
- **Phase 03 controlled multi-caller progress:** A retains64 pending operation
  reservations while B receives a correlated reply from the same provider,
  before A's drain gate opens. Two A overflow refusals deliver nothing, all64
  A replies drain once, and a second B reply succeeds. Evidence:
  `docs/evidence/c2c-multi-caller-progress-x86.{txt,log}`; lifecycle integration2,
  independent serial smoke and review passed. No production/ABI changes.
  Quota isolation only; full fairness under sustained peer-queue pressure remains open.
- **Phase 03 peer-mailbox pressure and finite competing producers:** provider
  mailbox (depth 64) saturated by A causes B to observe Busy before provider
  wakes; concurrent rolling calls (A 128, B 64, total 192) complete with exact
  correlation, retrying Busy only, with zero duplicates or leaked messages.
  Evidence: `docs/evidence/c2c-peer-pressure-x86.{txt,log}`; lifecycle
  integration 2 PASS, independent serial smoke PASS. Fixture-only; unbounded
  starvation freedom and SMP two-hart wake proof remain open.
- **Phase 03 concurrent input streaming under pending RPC:** 8 outstanding
  asynchronous calls held pending while an independent input source streams
  32 synthetic input frames to the caller; verified zero premature settlements,
  exact ordered input delivery, exact 8 RPC reply correlations, and zero
  leaked messages in the raw mailbox. Evidence:
  `docs/evidence/c2c-concurrent-input-x86.{txt,log}`; lifecycle integration 2 PASS,
  independent serial smoke PASS. Abandoned caller grant lifetime and quarantine
  are asserted on the x86 domain lane (`scripts/x86/qemu-domain-test.sh`).
  Deterministic two-hart publication and wake races are verified in kernel boundary
  regressions. Multi-source waiting remains open.
- **x86_64 C++ freestanding runtime qualified (2026-10-09):** POSIX shim C++ ABI
  enabled on x86_64, `raw_syscall` implemented for x86_64 Syscall ABI, and kernel
  initializes FPU/SSE (CR0.EM=0, CR0.MP=1, CR4.OSFXSR=1, CR4.OSXMMEXCPT=1, LDMXCSR 0x1F80).
  `/bin/cpp-smoke` runs end-to-end in Tier 2 Paged Domain under CR3 isolation,
  verifying static constructors, virtual dispatch, virtual delete, templates,
  operator new/delete, typed VFS IPC, and C-ABI file I/O.
  Evidence: `docs/evidence/c2c-x86-cpp-freestanding-runtime.txt`.
- **Phase 03 step-3 opt-in API landed 2026-10-09** (same session): `ostd::ipc::PendingCall` +
  `Completion` give Cells the nonblocking multi-outstanding call path the step-2 specification
  describes, with `is_definite`/`is_uncertain` encoding the retry rule; the serving side uses the
  existing `ostd::ipc::{current, reply}`. Strictly opt-in — the blocking API, every service's reply
  discipline and every kernel path are unchanged. Witnessed by `bench async-lifecycle` driving the
  API through the same measurements (8 outstanding, one correlated completion each, one `wait` round,
  `unterminal=0 lost=0` on mid-flight death), asserted by the x86 integration test:
  `docs/evidence/c2c-async-lifecycle-x86.{txt,log}`. **Still not admitted, each with its own cost
  stated in the phase file:** the blocking-API migration (the stranding fix), multi-source waiting,
  cancelling a dispatched operation, the two-hart wake proof and retained-reply lifetime.
- **Phase 03 step 2 landed 2026-10-09** (same session, no new admission needed): the lifecycle
  specification is now in the contract — [Spec 20 §2.6](../docs/specs/20-unified-ipc-contract.md),
  draft v3.2 — covering the operation lifecycle, the mapping to §2.4's outcomes, submission-as-copy,
  identity binding, reply retention, bounded capacity and timer-bounded waiting, each with what
  remains unproven. **Its main finding sizes the rest of Phase 03:** a bounded caller's terminal comes
  from `IpcReply` (only that handler reaches `async_ipc::terminal`), so an ordinary masked
  `Send` reply never completes an operation — measured, not inferred — which means the opt-in async
  API needs both sides to opt in, and step 3's stated goal (moving `LocalEndpoint::call` /
  `ServiceRef::call` onto the primitive so a dying provider stops stranding the caller) costs either a
  service-wide reply migration (Spec 17 §9 + two Law-1 confirmations) or a kernel change letting a
  plain reply terminalise an operation. Recommendation recorded: keep the blocking API as it is, ship
  the opt-in path first, and take the stranding fix as its own decision with that cost stated. No ABI,
  kernel or scheduler change was made.
- **Phase 03 step 1 admitted and landed 2026-10-09** (owner: same C2C session): the measured
  prototype that step 2's decision needs, and nothing else — no ABI, no kernel, no scheduler, no
  production callsite. `bench async-lifecycle` on the Phase-02 image, pinned by the x86 integration
  test: 8 outstanding bounded calls to one peer with exactly one correlated completion each (the
  peer answers in reverse, so identity is the operation, not arrival order), p50/p99 ≈ 1.56/1.59 ms
  on TCG and **one** `wait` round for the drain; `sys_try_send` measured as "delivered if the
  receiver is in `Recv`, else refused **in the return value**" with nothing queued on refusal; and a
  peer dying mid-flight terminalising every outstanding operation. Evidence
  `docs/evidence/c2c-async-lifecycle-x86.{txt,log}`. **Consequence:** a new public submission
  syscall is not justified by measurement, and step 2's two-Law-1-confirmation gate is not
  triggered; steps 2–4 (multi-source completion waiting, cancellation, two-hart wake proof,
  retained-reply lifetime, queue/bytes/fairness reservation) remain unadmitted and still need the
  file-owner handoff if they touch kernel syscall/completion/scheduler paths.
- **Phase 01 closed 2026-10-09 by contract-owner sign-off**, and with it **slice A of Phase 02
  admitted — the only implementation slice in flight** (WIP: one):
  **caller-side binding consumer** in `libs/ostd`: `ServiceRef` resolves and caches the frozen
  `LookupServiceBound` binding (`{tid, cell_id, generation}`) instead of a bare tid, and a call made
  under a binding whose provider incarnation is no longer live is refused with a typed error rather
  than silently re-targeted; `LocalEndpoint` gains the bound resolution. **No new ABI** (the opcode is
  frozen), **no new `/bin` path**, **no kernel IPC change**, and no `LookupService = 206` behaviour
  change: the resolution helper keeps its meaning and only the caching handle becomes
  generation-aware. Owner: current C2C implementation session; files:
  `libs/ostd/src/service.rs`, `libs/ostd/src/cluster_endpoint.rs` (+ tests).
  **Landed 2026-10-09, evidence complete:** `ServiceRef` caches and resolves the binding, classifies a
  failed exchange against the registry (`NotFound` for a descriptor the kernel no longer reports,
  `IO` for a failure against the same live endpoint) and gains `binding()`/`is_live()`;
  `LocalEndpoint::bind()` refuses with `EndpointError::NoLiveBinding` while `new(tid)` stays
  identity-free; the rule is pure and host-tested (`libs/ostd/tests/cluster-endpoint.rs`). Witnessed on
  the x86_64 lifecycle lane — `SDK-BINDING matches_raw=true`, `SDK-VFS-CALL=OK`,
  `SDK-BINDING-LIVE resolved=true unresolved=false`, `SDK-ABSENT-BINDING=REFUSED` — with the
  AArch64/RV64 `test-hooks` suites and the local broker oracle re-run green:
  `docs/evidence/c2c-sdk-binding-x86.{txt,log}`. **Not proven there:** the stale-after-death half has no
  runtime witness far longer than it should have: it needs a *registered* provider that dies, and the
  fixture was tid-addressed at first because a private-root Cell cannot register itself. The spawner
  can (`init` does), so the named fixture supplied it on 2026-10-09 —
  `STALE-BINDING=REFUSED`, `docs/evidence/c2c-named-tier2-service-x86.{txt,log}`.
- **Phase-01 exit evidence recorded 2026-10-09:** all four success criteria carry evidence (contract
  table; the §2.4 state-transition matrix; Spec-20/Spec-17/ADR-0015/Spec-22/ADR-0008/0009 consistency,
  including the Spec 17 §9 record of the changed syscall surface; and the local oracle re-run —
  `docs/evidence/c2c-broker-oracle-qemu-local.{txt,log}`, soak 10000/10000,
  `overflow status=PASS busy=1 queue_peak=16`, `restart status=PASS stale_send=INDETERMINATE`). See
  [phase-01-contract.md](260927-1100-c2c-anywhere-tier-aware/phase-01-contract.md) § *Exit-gate evidence
  review*.
- **Next slice identified 2026-10-09:**
  [phase-02-local-boundary.md](260927-1100-c2c-anywhere-tier-aware/phase-02-local-boundary.md) § *Next
  acceptance scenario* names two, in this phase's own step order. **A (step 1) — caller-side binding
  consumer** (admitted, see above). **B (step 2) — the cross-tier exchange:** one IPC-capable admitted
  Tier-2 fixture plus a Tier-1 driver on the x86 test-hooks domain lane, witnessing both directions and
  wrong-buffer / stale-generation / unauthorized-method / oversize refusals *before* delivery, with
  Tier-1↔Tier-1 unchanged and the production lane still asserting Tier-2 denial. Slice B touches new cell
  fixtures, loader launch-profile/ceiling rows and lane assertions, so it needs its own review. The
  phase's RV64 wording is superseded by ADR-0022; the Intel test-hooks lane is the qualified target.
- `261004-1957-x86-pc-lane` — **Intel-only supporting hardware lane**,
  one exact headless configuration ([plan](261004-1957-x86-pc-lane/plan.md)).
  **Authorized scope: phases 01–06** — 01 `x86_64-pc` descriptor + HCL model
  (**completed 2026-10-04**), 02a/02b AHCI/SATA (**completed 2026-10-05**,
  two-boot persistence), 03 xHCI + HID (**completed 2026-10-05**, enumeration +
  in-cell decode) and 03b USB HID producer role (**completed 2026-10-05**;
  append-only syscall 423 + service id 17 sharing allowlist bit 50, approved by
  the owner), 04a igb NIC part A (**completed 2026-10-05**; append-only opcode 424
  `FindPcieDeviceByVendor` + 48-byte `PcieDeviceInfo`, approved by the owner; SKU
  claim narrowed to `10C9`+`1533`), 04b igb DHCP data plane + VT-d variant
  (**completed 2026-10-05**; no product change — the net service already owned
  DHCP), 05 ACPI DMAR → real IOMMU (**completed 2026-10-05**; DRHD base parsed
  from DMAR, `DmaIsolation` profile contract makes absent-DMAR fail-closed), 06
  multi-port COM/RS232-485 (**completed 2026-10-05**; ABI 425–428 + `serial_port`
  capability + policy blob v4, owner-approved; RS485 declared, not claimed).
  **Phase 07 remains hardware/procurement-gated and is not activated here.**
  Qualify one exact Intel machine after the existing QEMU gates and separately
  approved acquisition; a separately qualified second Intel node follows the
  first-node gates. Matching model is preferred, not mandatory.
  The former Intel-plus-AMD machine objective is superseded. No physical HCL row
  exists. WIP remains one controller family at a time, only when needed by C2C.
  Intel VMX is a required Tier 3 dependency in `260711-1917-tier3b-x86-vtx` P09;
  SVM/TCG is not Intel evidence. x86 AVX2/other optimization is parked unless a
  measured C2C workload needs it. Secure Boot retains its security-track gates.
  No new ABI, hardware qualification or evidence promotion is authorized here.
- x86 Tier 2 runtime/admission and C/C++ support — **completed 2026-10-09**:
  x86 freestanding C++ runtime profile qualified in CR3-isolated paged domain
  (`docs/evidence/c2c-x86-cpp-freestanding-runtime.txt`); production switch-ordering
  gate closed and verified across QEMU TCG, QEMU KVM, and VMware Workstation Pro
  (`docs/evidence/vmware-domain-test.txt`).
- `260711-1917-tier3b-x86-vtx` P09 — required Intel VMX/EPT dependency. Plan and
  qualify Intel guest lifecycle/storage/network before claiming the Tier 3 target.
  Existing SVM evidence is retained; AMD physical work is parked.
- Protected relay identity/time/persistence — required, phase-local C2C
  prerequisites. Existing authority approvals and evidence gates remain; reuse
  earlier records without opening another general hardware/cloud program.

Side work is limited to defects, security, build/CI and evidence repairs needed
to protect this program's shared baseline. No unrelated feature work is admitted.

## Parked candidates — not an executable queue

The following earlier queues are preserved for context. Before resuming any
slice, record its concrete Intel C2C dependency and meet the promotion rule.
Satisfying an old trigger alone does not reopen an independent program.

- `260712-0800-supervisory-cell-migration` — P-TRUST dependency satisfied; WIP-limited.
- `260712-1000-cell-package-distribution` — blocked on a capability-scoped installer
  redesign; ambient or name-authorized `/bin` writes are forbidden.
- **Trust & Identity program** (one portfolio group, separate child plans):
  - `260712-1900-manifest-v2` — P00-P02 complete; P03 deferred.
  - `260712-1902-dice-attestation-identity` — P00 complete; P01-P05 queued.
- `260605-1406-phase28-wasm-cells-epmp` — partial/suspect: WASM crates are present but
  retain-vs-remove and runtime qualification are unresolved; ePMP is M-mode-blocked.
- Per-request server scale (D5) — accepted goal, WIP-limited behind Midori. Promotion requires
  N=64/128/256/512 memory/spawn/isolation baselines **measured with M heavy cells resident**
  (mixed occupancy, not a homogeneous light sweep) before image sharing, demand stacks, profile
  quotas, or dynamic cell tables are implemented. Firmware memory discovery landed
  (`kernel/src/boot/dtb_memory.rs`), so the hardcoded 190 MiB map that capped the 2026-07-31
  measurement (n=8–9) no longer binds; those baselines have not been re-run. A variable VA budget
  (fixed 32 MiB stride today) is an additional named prerequisite — Spec 19 §3 amendment,
  2026-09-22.

## Explicitly deferred

- ARM/RPi3/RISC-V/MCU and AMD physical bring-up, NIC/peripheral expansion and new boards.
- Robotics and LAB-01 / BASE-01 / ASSEMBLY-01 physical workflows.
- Desktop, ViUI, Ocel/browser, graphics/typography and GPU acceleration.
- AI/Hypha/NPU/GPU product expansion and general office/server replacement.
- Standalone runtime breadth, BEAM parity expansion and general VM-platform work.
- Manifest v2 `cap_args`, DICE adapters and other breadth without an identified
  C2C consumer remain parked; a necessary security slice still needs its own gates.
- KMS protected persistence and SSH key custody (Cellbox D79-14, 2026-10-25). KMS runtime
  protected-state load/persist is deliberately fail-closed (`cells/services/kms/src/storage.rs`):
  no authenticated root, sealed state or rollback protection. A plan must establish those, then
  add an SSH ed25519 key class, chunked import over the 128-byte frame (OpenSSH v1, unencrypted
  or bcrypt + AES-256-CTR; `bcrypt-pbkdf` must enter the lock), session-bound userauth signing
  through Sunset's `SignKey::AgentEd25519`/`CliEvent::AgentSign`, and caller admission for
  `/bin/cellbox-ssh`. It depends on the CellosFS crash-safety plan
  (`261025-cellosfs-crash-safety`). It unblocks Phase 09 S9-5 (`--auth publickey`, pipeline
  exec, exact non-UTF-8 stdin). **Opened 2026-10-30 as plan `261030-1000-kms-ssh-key-custody` (charter KSSH-CP1-A approved, documents only; tiers T0/T1 `DEV_POSTURE`/T2; slices K0-K9; every code slice needs its own CP1/CP2 and the lock-adoption packet).**
- Completed assets remain in-tree; existing cross-architecture regressions may
  protect shared changes but do not make those architectures active targets.

## Completed / closed records
- `260916-1200-tier1-rust-std-pal-implementation` — [completed 2026-09-16] Tier 1 Rust `std` PAL in-tree implementation (custom target specs, sysroot overlay, PAL primitives, workload parity PASS).
- `260913-2002-g2-level-a-ai-inference` — Spec 24 CPU inference path (CP-1..CP-3); phases
  01-04 complete at the host/QEMU ceilings, NPU/GPU/Tier 2 checkpoints remain gated.
- `260727-2101-midori-lessons-cellos` — complete convergence program (D39).
- `260925-2214-beam-parity-b0-actor-supervisor` — B0 of the BEAM/OTP backend roadmap
  (`docs/roadmap/beam-parity-backend-roadmap.md` §5): a userspace actor + supervisor library
  (`ostd::actor`, `ostd::actor::supervisor`) so an application declares its own supervision tree
  instead of editing `/bin/init`. No `libs/api` change; design in
  [ADR-0021](../docs/decisions/0021-actor-supervisor-library-in-userspace.md). Witnessed at the
  `qemu` ceiling (`scripts/qemu-actor-supervisor.sh`, evidence under `docs/evidence/`). B1 (in-cell
  concurrency and cancellation) and B2 (per-request cell cost/scale, which keeps D5 WIP-limited)
  stay queued behind their own triggers; this promotion opens no ABI and touches no
  capability-scheduling boundary.

- `260712-1901-cap-revocation` (P00-P05) — closed 2026-09-25 at the `qemu` ceiling.
  `sys_cap_revoke` no longer label-changes: MMIO windows lose user accessibility
  (`unmap_mmio_user_x86` / `clear_mmio_user`), owned grants are reclaimed with in-flight
  pins quarantined rather than freed, `iommu::unmap_dma` is real (leaf cleared + IOTLB /
  IOFENCE acknowledged) and the whole-domain teardown is shared with cell death, and
  `pcie_driver`/`platform`/`supervisor` became revocable via three additive `cap_mask`
  bits (Law-1 confirmed twice) with their DMA/BDF/BAR/ECAM teardown. The victim is told
  with `AppEvent::CapRevoked` on the newly registered `0xF2` envelope. Witnessed by one
  RV64 QEMU run (`qemu-native-domain-test --harts 1`, kernel `e1ec27a0…`): IOMMU-TEARDOWN,
  GRANT-RECLAIM and MMIO-REVOKE markers plus the `thread-cap` revoke aggregate. Raw log:
  `docs/evidence/cap-revoke-qemu.{log,txt}`. Residual recorded, not hidden: no Cell issues
  `CapRevoke` yet (the end-to-end path is witnessed in-kernel), the DMA-fault oracle needs
  real IOMMU hardware, and the x86/aarch64 MMIO legs are compile-verified only.
- `260922-1549-cell-native-portability-program` (7 phases, ADR-0018/ADR-0019) — closed
  2026-09-25 at the `qemu` ceiling. Tier 2 admission on the path, `cpp-freestanding`, per-task TLS,
  futex ABI + wait queues, the kernel pipe object, the generated porting kit, and the three
  reference-port classes are all witnessed by RV64 QEMU runners
  (`qemu-native-domain-test`, `qemu-cpp-smoke`, `qemu-tls-test`, `qemu-futex-test`,
  `qemu-pipe-test`, `qemu-c-pthread`, `qemu-c-spawn`); the follow-on C thread/process
  proposal closed with it. Raw logs: `docs/evidence/`; closure record:
  `.agents/260922-1549-cell-native-portability-program/phase-07-reference-ports-and-cost.md`.
  Residual recorded, not hidden: a third-party port needing a `fork`/`exec` process tree stays
  class D, and class C is witnessed by an in-tree workload rather than vendored third-party code.
- `260616-0755-viui-completion` — canonical ViUI v2 implementation record.
- `260712-1100-loader-trust-repair` — P-TRUST landed in `721e1f6f`.
- `260712-1900-manifest-v2` implementation P00-P02 — landed in `c25f3185`.
- `260712-1903-thread-cellid-quota-fix` — kernel-side closure recorded.
- `260801-d12-hardware-supplement-ruling`, `260801-d13-tier1-signature-admission-ruling`,
  `260801-d15-d17-rulings`, `260801-d18-d25-rulings`, and
  `260801-d26-d33-rulings` — decision/documentation records complete.

## Superseded / retired

- `260624-cell-to-cell-anywhere` — superseded 2026-09-27; P00–P03 historical partial foundation only, integration not delivered.
- `260819-1409-cell-to-cell-anywhere-core` — superseded 2026-09-27; completed local-only evidence retained, remaining phases replaced by `260927-1100-c2c-anywhere-tier-aware`.

- `260608-1451-viui-next-phases`, `260609-0601-viui-g2`, and
  `260608-1227-viui-embedded-robot-readiness` — superseded by the closed ViUI record and
  current Spec 14.

## Promotion rule

A slice becomes executable only after the controlling Cell contract plan records
its explicit adoption/rebase and file-owner handoff, this index selects it as the
single implementation slice, and its concrete Intel C2C outcome, dependency,
acceptance scenario and evidence ceiling are named. `CELL-CONTRACT-0024` is not
released by completion of documentation or an older prerequisite. Update the
affected lock scope and [inventory](cell-contract-0024-locks.md) in the same change;
all unselected scope remains blocked or parked.

Technical and approval triggers must be evidenced and applicable Law-1 checkpoints
must be satisfied. No-ABI authorizations do not silently become ABI authorization.
An unrelated program requires an explicit new owner direction decision, not a
local plan trigger. Direction approval does not authorize procurement, paid
services, irreversible provisioning, remote enablement or production. Preserve
independent human and physical qualification gates. Historical aggregate counts
are not current implementation truth.
