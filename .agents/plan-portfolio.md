# Cellos plan portfolio

**Status:** Canonical scheduling index
**Updated:** 2026-10-08 — Intel x86-64 C2C Anywhere is the sole active program

This file owns scheduling intent. Source/tests own implementation truth; individual plan
files preserve detailed scope and provenance. Untouched checkboxes are not proof that code
is absent. [ADR-0022](../docs/decisions/0022-intel-x86-64-c2c-only-direction.md)
owns direction; this index owns child-plan scheduling. Every task must name a
direct Intel C2C deliverable, required prerequisite, measured bottleneck or
regression/security/build repair protecting that path, plus its acceptance
scenario and evidence ceiling. An old plan's `active`/`ready` status is not
authorization. Plans not admitted below are parked/historical.

## Active program — Intel x86-64 C2C Anywhere only

- `260927-1100-c2c-anywhere-tier-aware` — **primary program**, not a queued
  alternative. [Plan](260927-1100-c2c-anywhere-tier-aware/plan.md).
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
  **Limitations recorded with the admission:** a private-root cell **cannot** register a service
  (`RegisterService` stays `SpawnCap`-gated), so the Tier-2 side is tid-addressed rather than
  registry-named — a genuinely *named* Tier-2 service needs its own authority decision and is not
  claimed here. Of the phase's four pre-delivery refusals, three are witnessed (oversize frame,
  dead/stale peer, unauthorized method) and the fourth — a wrong **user buffer** on the syscall copy
  path — is not re-created: a `#![forbid(unsafe_code)]` cell cannot fabricate a pointer, and the
  address-containment witness already runs as `/bin/tier2-exploit`.
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
  runtime witness yet — it needs a *registered* provider that dies, and slice B's provider is
  tid-addressed (a private-root Cell cannot register) — so the rule is covered by host tests only.
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
- x86 Tier 2 runtime/admission and C/C++ support — required consumer dependency,
  not delivered by the prior RV64 portability closure. Scope through existing
  [kernel evolution](260906-dual-mode-kernel-evolution/plan.md) and portability
  contracts; admission is test-image-only and x86 C++ runtime support is missing.
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

A slice becomes executable only when it names a concrete Intel C2C outcome,
dependency and acceptance scenario, its technical/approval triggers are evidenced,
file ownership does not collide, Law-1 confirmations are satisfied where needed,
and this index records the selected slice in the same change. An unrelated
program requires an explicit new owner direction decision, not just a local plan
trigger. Direction approval does not authorize procurement, paid services,
irreversible provisioning, an ABI change, remote enablement or production.
Do not advertise aggregate COMPLETE/OPEN counts until a generated inventory can
reconcile source evidence with plan metadata.
