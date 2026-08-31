# Cellos Project Roadmap

**Project**: Cellos (Jarvis Hybrid OS)
**Current version**: 0.2.1-dev (Mycelium Era)
**Current phase**: Phase 1 - Core Stability; active product stage G1 Robot & Embedded
**Last updated**: 2026-08-29
This file is the roadmap entrypoint. The previous all-in-one roadmap is
preserved as a read-only content snapshot at
[project-roadmap-legacy.md](project-roadmap-legacy.md). Use it only when a
historical decision is not represented by the current topic pages.

## How to Read the Roadmap

| Need | File |
|---|---|
| What is active now | [roadmap/current-focus.md](roadmap/current-focus.md) |
| Hardware qualification lanes | [roadmap/hardware-tracks.md](roadmap/hardware-tracks.md) |
| Product-stage overlay G1-G5 | [roadmap/product-stages.md](roadmap/product-stages.md) |
| Runtime and platform overlays | [roadmap/runtime-and-platform-tracks.md](roadmap/runtime-and-platform-tracks.md) |
| Technical milestones and historical status | [roadmap/technical-milestones.md](roadmap/technical-milestones.md) |
| Completed history ledger | [roadmap/completed-history.md](roadmap/completed-history.md) |
| Known open risks and deferred gates | [roadmap/open-risk-register.md](roadmap/open-risk-register.md) |
| Immutable pre-split snapshot | [project-roadmap-legacy.md](project-roadmap-legacy.md) |

## Execution Model

G1–G5 are release and market overlays, not a global execution queue. A lane may
advance only when its own dependency and evidence requirements are met; a later
product-stage label never makes unrelated host or QEMU work wait.

The canonical evidence ladder is `none → contract → host → QEMU → physical →
service → production`. `execution_class` describes whether a lane may run now:
`ready`, `scope-gated`, `contract-gated`, `governance-gated`, or
`external-gated`. `evidence_ceiling` describes the strongest result the next
slice may truthfully claim. QEMU and host evidence never promote a lane to
physical, service, or production qualification.

### Development-first hardware-constrained decision

[ADR-0007](decisions/0007-development-first-hardware-constrained-execution.md)
sets the current execution policy. The available platform inventory is QEMU,
two owner-reported Raspberry Pi 3 Model B+ boards, and incoming sensors. No
additional hardware procurement is planned now. QEMU, RPi3, sensor, and
local-runtime lanes may advance independently to their stated evidence
ceilings. The HDMI external-display lane is complete at exact-device
development evidence on the prior captured Model B device, whose mapping to
the current Model B+ inventory is unresolved. The lane is regression-only;
camera and other sensor integration remains executable but is
deferred in the session order.

QEMU results remain software-only. RPi3 and sensor results may establish
development and hardware-integration behavior on the exact exercised devices,
but the RPi3 is never a production-security qualification target or an
independent external floor. Production admission and production release remain
disabled and fail-closed until every applicable remote-identity, protected-root,
secure/measured-boot, qualified-floor, physical-hostile-evidence,
authenticated-runner, human-approval, and governed-ledger gate is satisfied.
Those gates are milestone-local: they do not block unrelated QEMU, RPi3,
sensor, or local-runtime development.

### Planning classes

- **Current executable work**: useful work supported by present software,
  QEMU, the two Raspberry Pi 3 Model B+ boards, or incoming sensors, bounded
  by its lane ceiling.
- **Current-scope technical debt**: a defect or maintainability gap in the
  current supported scope; it is not a label for every unfinished capability.
- **Future capability**: intentionally later product functionality, not a
  current defect.
- **Completed / regression-only**: a delivered lane whose stated evidence
  ceiling has passed and has no active implementation slice; reopen only for a
  regression or a separately governed higher evidence class.
- **External-gated prerequisite**: work that cannot cross its next evidence
  boundary until a named external asset, product, account, or vendor package
  exists.
- **Production release gate**: a mandatory production-admission or
  production-release invariant. It does not serialize non-production
  development.

## Capability Lanes

| Capability | Planning class | Execution class | Evidence ceiling | Owner / next slice | Reopening event |
|---|---|---|---|---|---|
| Roadmap projection | Current executable work | `ready` | `contract` | Hardware-independent roadmap Phase 01/08 | A lane emits bounded evidence/status |
| General QEMU software and integration | Current executable work | `ready` | `qemu` | Owning runtime/platform lane | Exercise the supported software path; do not promote the result to physical, service, or production evidence |
| RPi3 HDMI software and exact-device boundary | Completed / regression-only | `scope-gated` | `physical` development evidence on the prior captured revision `a22082` / Model B / serial `000000003d042795` device; mapping to current inventory unresolved | Phases 04 and 05 completed; no active HDMI slice | Reopen only for a regression: the exact mailbox unsafe island is approved by `lungmat8`, strict F1/F5 passes, and the separately recorded TFTP deployment, later UART boot block, and user visual observation close the reviewed exact-device gate |
| RPi3 peripheral hardware integration | Current executable work | `ready` | `host` now; `physical` development evidence after exact-device exercise | G1 board/peripheral lane using the two available Raspberry Pi 3 Model B+ boards; HDMI external-display work is completed and regression-only | Reconcile each current board's exact serial, revision, and condition before attributing evidence; stop before any production-security qualification claim |
| Camera and other sensor integration | Current executable work | `deferred` | `contract` until resumed; then exact-device `physical` development evidence | Deferred in the current session order; the available camera must be identified before use | Resume the sensor lane in a later session and record the exact sensor/interface before exercise |
| Tier 3 hostile QEMU evidence | Current executable work | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 06; x86 bounds, descriptors, backend faults, reset, restart, and independent vCPU preemption pass in 27 bounded scenarios | Rerun the ARM64 hostile corpus only in an environment that reaches the guest probe past the known synchronous TCG fault |
| ARM64 Tier 3 persistent storage | Future capability | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 09 | Supported Phase 06 hostile scenarios; policy is fixed to `build/tier3-arm64-persistent.img` at 8 MiB with explicit cleanup |
| Desktop, ViUI, and SDK | Future capability | `scope-gated` | `qemu` | Managed-surface child is complete at the QEMU ceiling | Resume the owning desktop lane only under separate approval; the dedicated RV64 oracle proves generated Counter repaint, maximize/restore, accepted close, and pointer-established keyboard activation, while `window-policy` remains a separate passing regression |
| Local Cell-to-Cell runtime | Current executable work | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 04; bounded Candidate B ingress and opaque KMS identity consumer wired; supervisor-only exact-revision recovery fixed; core Phase 04 local protocol contract complete at its disabled ceiling; Phase 05 local-only pool admission in progress; qualified provider execution still gated | Required [CI job](../.github/workflows/ci.yml) `c2c-broker-oracle-single-guest-local-runtime` gates the runner; local protocol host tests pass 94/94, endpoint integration tests pass 5/5, and RV64 broker/`ostd` builds pass with canonical hostile-decode properties, monotonic deadlines, monotonic epoch-before-dedup receive ordering, a 3,712-byte frame-body cap, authenticated replay floors, no occupied session displacement, and a remote call boundary returning typed `NotSupported` without broker contact; actual restart-enabled oracle passes 1/1; KMS absence remains ephemeral/local-only with remote disabled; separately approve protected provider, physical recovery, authenticated relay registration/configuration, cross-broker incarnation binding, two-node relay/direct-LAN, and remote restart/failover scope |
| Kernel signature, pointer, and entropy remediation | Current-scope technical debt | `governance-gated` | `host` | Separately approved security children | Named security/PAL approvals and implementation checkpoints |
| Authenticated software evidence | Completed / regression-only | `ready` | `host` | Hardware-independent roadmap Phase 07; trusted GitHub-hosted `main` run `33251921677:1` at revision `d951d7dbf191133e94061ded7f0a8d17bfcf07c8` completed, manifest digest `2263115d4f3f58b990074d0cb7489ec5f52523f23a2a9777a8685a8c09492abb` was independently verified, the run-id/attempt sequence was consumed once through explicitly provisioned durable operator-owned external state, and exact replay was rejected | Reopen only for a pipeline/schema/workflow-identity regression or a separately governed higher evidence class; authenticated carriage preserves each bundled result's existing ceiling and implies no physical, secure-root, cloud, approval, admissibility, or production claim |
| x86 Tier 3 VirtIO parity | Completed / regression-only | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 10 completed with two-boot block/network and 27-scenario hostile QEMU evidence | Reopen for a software regression or a separately governed physical-x86 qualification run |
| Remote/public Cell-to-Cell operation | Future capability | `contract-gated` | `host` / `service` | ADR-0009 and Cell-to-Cell Anywhere remote/public children; correlated relay server codec verified 40/40 | Authority-owned client framing and bounded broker correlation integration wait for post-entry-GO Phase 4 Build; remote routing still waits for ADR-0008 AC-012 plus the separately governed identity/export gates |
| Protected relay identity | External-gated prerequisite | `external-gated` | `host` | ADR-0008, KMS/Silo protected relay plan, and Cell-to-Cell Anywhere Phase 05 | Real protected persistence, authenticated time, reviewed pending-key binding, and `DEV_REFERENCE` authority Phase 8 GO over AC-001..AC-011 open service-net Build. Build must then implement ADR-0008 and pass AC-012 before the Phase 05 relay route can be enabled; public KMS opcodes stay frozen and this lane-local gate does not block unrelated development |
| STM32 DEV_REFERENCE protected authority | External-gated prerequisite | `external-gated` | `host` `SOFTWARE_HARNESS` complete; later exact-device `physical` development evidence | Authority Phase 4 private v2; typed protocol, authenticated profile bank, full-record journal/recovery model, certificate/profile validator, promotion recovery, production rejection, and deterministic non-executing provisioning plan pass 52 authority-protocol, 38 journal/bank, 17 validator, 22 provisioning, and 8 production-rejection tests plus RV64 no_std checks | Admit exact STM32H573I-DK and private SLB9672 hardware; freeze its TPM handle/NV/template/policy map; obtain operator approval of the irreversible plan hash; prove locked-device isolation, lifecycle/debug protection, endurance, and the full physical failure matrix; and demonstrate a confidential, integrity-protected, purpose-bounded STM32-to-isolated-KMS capability handoff |
| AWS DEV_REFERENCE nonce-bound signed time | External-gated prerequisite | `external-gated` | `host` `SOFTWARE_HARNESS` cores complete; later authorized live-service evidence | Authority Phase 5; strict canonical protocol, request/registration authentication, receipt-first persistence/recovery, allocator and clock policy, pinned KMS boundaries, handler composition, manifest, production rejection, and non-deploying CloudFormation/IAM graph pass 233 signed-time and 8 production-rejection tests; allocator-lineage and package/deploy/rollback/evidence scripts remain open | Select and freeze the authenticated clock provider endpoint, canonical response/signature contract, key/SPKI pin, freshness semantics, and non-restorable epoch/checkpoint authority; implement deterministic restored/fork lineage tests and the artifact/deployment scripts; name the dedicated AWS DEV account/region; obtain deployment authorization; compose the real artifact; then execute outage, restore, rollback, principal-reachability, and allocator-fork scenarios before handing the pinned manifest, vectors, and raw evidence index to Phase 6 |
| G3 accelerator | Future capability | `external-gated` | `contract` | Accelerator evidence envelope | RK3588, accepted RKNN package/license, then X390 evidence |
| VF2 DEV_REFERENCE root stream | External-gated prerequisite | `scope-gated` | `host` `SOFTWARE_HARNESS` complete; later exact-device `physical` development evidence | ADR-0010 and authority Phase 3; deterministic no_std CBOR/COSE core, bundler, independent verifier, bounded host I/O, initialized-DRAM-contained pre-receive window/quarantine validator, logical cleanup-order harness, and XMODEM codec pass 28 tests plus real host smoke | Exact VF2 v1.3B plus STM32 authority hardware must freeze BootROM/SRAM/DRAM/address/quarantine/coherency limits and prove sole-sender/reset/strap/media/cleanup negatives; host output never satisfies that gate |
| Additional physical-board qualification | External-gated prerequisite | `external-gated` | `physical` | Hardware tracks | Exact VF2, Pioneer, RPi4, or x86 board and board-specific logs; existing RPi3 work need not wait |
| Production root selection | External-gated prerequisite | `external-gated` | `production` | ADR-0006 | Vendor package satisfying ADR-0006 and a superseding GO ADR; no stock TPM or generic secure-element counter is selected as the floor |
| Production admission and release | Production release gate | `external-gated` | `production` | Tier 1 admission and governed release owners | Remote C2C identity where applicable, protected relay identity, production KMS/root, secure/measured boot, qualified external floor, physical hostile evidence, authenticated runner, required human approvals, and release-ledger closure |

The lane-specific child plans under
[`../.agents/260827-1004-hardware-independent-roadmap/`](../.agents/260827-1004-hardware-independent-roadmap/)
own execution details. This page is the authoritative routing index; the legacy
roadmap remains historical only.

## Current Direction

Cellos is being shaped around product stages, not only phase numbers:

- G1 Robot & Embedded: RV64/ARM64 SBC-class robot/embedded system with bounded
  memory, hardware I/O, fast boot, and never-die supervision.
- G2 Server & Specialized PC: x86_64/server qualification, SMP throughput,
  large storage, zero-downtime service upgrades, desktop/tooling depth.
- G3 NPU-native Compute OS: parked until real NPU hardware and vendor API
  experience inform the contract.
- G4 Full Rust std for Tier 1 Cells: planned as a `rust-std` runtime profile
  using pure-Rust PAL/rustc target work, not `std` over mlibc.
- G5 Virtualization Platform: research/design overlay after G4.

## Current Codebase Facts

- Cargo workspace members: 111, verified with `cargo metadata --no-deps`.
- HAL shape: `hal/core`, four `hal/soc/*` crates, fifteen `hal/traits/*`
  crates, and three `hal/arch/*` crates.
- HAL to kernel Rust ABI hook signatures are single-sourced in
  `hal/traits/arch/src/kernel_abi.rs`; `scripts/check-hal-boundaries.sh`
  rejects new local `extern "Rust"` declarations under `hal/arch`.
- Board descriptors live in root `boards/`; seven descriptors are active
  integration targets, while `q35-x86_32`, `virt-riscv32`, and `virt-aarch32`
  remain placeholder-only documentation entries.
- Active native scripting runtime: Lua. MicroPython is historical roadmap text
  and is not a current Cargo workspace member.
- Application execution uses Tier 1/2/3 terminology. `Tier 1b` and `Tier 3b`
  are legacy guide aliases for Tier 1 runtime profiles and Tier 3 Linux guests;
  SDK packaging uses named modules, not numbered tiers. The ratified [Spec 23
  Native SDK contract](specs/23-native-sdk-contract.md) keeps execution tier,
  runtime profile, stability, and availability as separate axes. Manifest v2 exposes
  canonical `PROTECTION_CLASS_*` aliases while retaining the ABI-stable `tier`
  byte and legacy `TIER_*` names.
- Tier 2 RV64 native-domain substrate and scheduler transitions (Spec 22 Items 2–3)
  are implemented behind `native-domains` and `test-hooks` with verified one-hart
  (`switch`, `sas-fastpath`) and two-hart (`migration`) QEMU evidence. Production
  admission remains disabled by default, SAS remains the default view, and no
  Manifest v3 bytes, installer UI, or qualification claims are exposed.
- Tier 3 x86 has aggregate QEMU evidence: the normal pinned Alpine 3.21.7
  Linux 6.12.81 path reaches `/bin/sh`, and the dedicated `/virtio-e2e` path
  passes two fresh outer boots under QEMU-TCG 10.2.0 with Intel VT-d ACTIVE,
  persistent 16 MiB block write/FLUSH/readback, IRQ5/IRQ6, and shared network
  TX/RX under a distinct nested MAC. The hostile path passes 27 bounded,
  origin-separated scenarios, including an independently observed pause-less
  vCPU preemption, supervisor-driven VFS/Net generation termination, bounded
  unavailable outcomes, persistent block reopen/readback, acknowledged network
  TX, and matching ARP RX after restart. Post-stimulus liveness and host-read
  persistence remain required. This is QEMU-only evidence: ARM64 hostile
  execution is blocked, QEMU-TCG 8.2.2 remains incompatible, and physical x86
  remains hardware-gated.
- RV64 QEMU desktop now has a bounded compositor-owned window-policy slice.
  Interactive surfaces carry bounded titles and receive typed lifecycle events
  without losing normal forwarded input. The compositor paints clipped
  frame/title/control decoration outside immutable client coordinates, consumes
  decoration input, and preserves content click-to-raise, capture, keyboard
  focus, and background exclusion.
- A titlebar drag relocates the window. Edge/corner resize, maximize, and
  restore use a staged `WindowConfigure` transaction: the owner applies a
  replacement Grant and acknowledges the matching serial before geometry
  commits. Minimize removes the surface from paint/hit testing until restore;
  close requests require an explicit owner rejection or acceptance.
- The `window-policy` QEMU scenario drives real tablet input and samples real
  scanout for frames, controls, titlebars, client pixels, drag relocation,
  resize commits, minimize/restore, maximize/restore, and close
  reject/accept. Its existing background, capture, and keyboard-focus coverage
  remains; the separate compositor-cursor scenario retains cursor coverage.
  This remains compositor policy, not a desktop shell: no taskbar, snapping,
  persistence, or live resize preview is supplied.
- The compositor's damage-driven scanout path now clips each intersecting
  surface copy to the dirty region before decoration, cursor composition, and
  GPU flush. This preserves the bounded window-policy contract while avoiding
  whole-surface copies for local damage.
- ViUI now has a bounded managed-surface integration: `ManagedSurfaceApp`
  handles configure/resize, minimized/restore, close accept/reject, and
  explicit shutdown for one compositor-owned `ViSurface`. The live generated
  Counter demo uses compositor-forwarded input and precise clipped damage.
  Its dedicated RV64 QEMU oracle passes generated-label repaint, pointer input,
  maximize/restore geometry, accepted close, and post-restore Enter activation;
  the separate `window-policy` QEMU regression also passes. This is QEMU
  software evidence only, not physical-board or production qualification.

## Immediate Open Gates

- Production signing is not fleet-enforced by default: `signing-required` is
  non-default and the non-dev public key path is still a `[0u8; 32]` placeholder.
- Physical hardware evidence remains separate from QEMU/compile evidence. RPi3
  boot/storage/UART and BCM GPIO/I2C/SPI gates pass; VF2, Pioneer, RPi4, and
  physical x86 remain hardware-gated.
- The q35 Phase 05 PCIe/NVMe/e1000/VT-d lane passes in QEMU only. PCIe buses
  above bus 0, real NIC Tx/Rx/DHCP, and the BAR unit-test harness remain open.
- AArch64 test-hooks runtime evidence remains host-gated where the existing
  `qemu_exit::AArch64Semihosting` issue blocks the lane.
- RV32 release compilation is verified, but RV32 runtime cannot run on this
  host without OpenSBI firmware. This is a non-blocking compile-only evidence
  gap, not runtime qualification.
- Tier 2 native domains have implemented RV64 QEMU substrate and cross-hart
  migration evidence, but remain unqualified for production release; physical
  hardware containment, DMA quarantine, and approval gates remain open.
- The Native SDK contract is ratified and the authoritative Phase 02
  [acceptance ledger](app-tier-acceptance-ledger.json) is recorded through
  `LEDGER_RECORDED`. The ratified revision is `798e8b04`; the implemented,
  verified, and attested lifecycle commits are `92340d05`, `635600c8`, and
  `c538df84`. Phase 03 remains `PLANNED`; its production-admission work remains
  blocked. The qualification
  result remains `NOT_COMPLETE`: compile, test/runtime, delivery, hardware,
  admission, and hostile-test witnesses remain mandatory before any applicable
  SDK cell can be promoted to `USABLE`; FFI, `rust-std`, and Tier-2 scopes
  without ratified applicability remain non-qualifying.
- **PREQUALIFICATION INFRASTRUCTURE COMPLETE / ADMISSIBLE EVIDENCE
  BLOCKED:** the backend-neutral Tier 1 admission core remains test-only, while
  the canonical 18-row prequalification catalog now maps all 33 stable
  `C3-ADM-*` `test-hooks` cases and the strict parser/validator pins their
  runtime ordering. Verification is Python 13/13, RV64 33/33 plus its aggregate
  test PASS marker, QEMU integration 1/1, production-marker exclusion PASS,
  with the host baseline unchanged at 101 passed, 0 failed, and 4 ignored. The
  rejected local capture/writer and its generated bundle were removed; local
  runs are verification only, non-admissible, and retain no Phase 04 evidence.
  Production admission remains disabled, Phase 03 remains `PLANNED`, and Phase
  04 remains `BLOCKED` pending a signed CI or secure measured runner, a qualified
  authenticated rollback-resistant floor, persistent slot/evidence recovery,
  physical hostile evidence, provisioned owner/publisher anchors, production
  loader/task/audit wiring with no-task-on-denial evidence, both required human
  approvals, and governed ledger/release closure.
- **RUST `STD` FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN
  APPROVAL BLOCKED:** the pinned scope reconciles 27/27 sys modules and 36
  hooks (8 Supported, 10 Unsupported, 18 Deferred), selects a conditional
  content-addressed private source overlay, and verifies a fixture-only,
  non-promotional benchmark validator. PAL-019 technical backing now binds a
  production release tuple without default features and a source-equivalent
  no-default QEMU companion proving zero without synthetic success. PAL-031
  technical backing binds caller-owned GetRandom validation plus isolated RV64
  QEMU hostile/final-write race evidence. The authoritative support map keeps
  both hooks Deferred pending every named approval. This runtime evidence does
  not grant PAL approval, real entropy, or any remaining qualification gate.
  There is no PAL, target, sysroot, runtime,
  live capture, approval, or promotion. All six approval rows remain `NOT
  GRANTED`, the implementation checkpoint remains `BLOCKED`, and umbrella
  Phase 06 remains pending and dependency-blocked on Phase 03. Maintained
  detail is in
  [runtime-and-platform-tracks.md](roadmap/runtime-and-platform-tracks.md).
- [Spec 18c Publisher Provenance Envelope](specs/18c-publisher-provenance-envelope.md)
  is **proposed**, pending security-owner and independent-reviewer approval. It
  introduces no producer, parser, production profile, or admission path; it
  does not alter the ledger's Phase 03 `PLANNED` status or unblock production
  work. Production admission remains disabled pending qualified and approved
  external-floor and owner-record gates.
- Local Cell-to-Cell recovery is explicitly relay-first. The current broker
  fails closed on bounded K1 loading, runs authenticated LAN beacons, and owns
  bounded local ingress/worker/reply roles, but neither direct Noise sessions
  nor relay routing are constructed from its dispatch loop. The external relay
  server requires TLS 1.3 mutual authentication, certificate-bound SPKI
  identities, pre-registration revocation checks, generation-safe duplicate
  handling, and bounded sessions, frames, and I/O from a strict mounted
  manifest. Recovery begins with the pending
  [relay-first contract](../.agents/260819-1409-cell-to-cell-anywhere-core/plan.md);
  public export and distributed leases remain deferred until separately
  approved.
- KMS TLS-signing Phase 1 is verified as a fixture-backed, non-production
  vertical slice. The append-only KMS v1 ABI binds signing authority to the
  live service-net cell generation and TID, keeps C2C X25519 and Relay P-256 as
  independent capability leaves behind one provider boundary, rejects replayed
  request IDs monotonically, and returns only low-S signatures that KMS verifies
  against the exact TLS 1.3 client `CertificateVerify` input. No generic signing
  operation or private-key export was added. See
  [ADR-0005](decisions/0005-mutual-tls-relay-identity.md).
- A 2026-08-29 security review narrowed that Phase 1 evidence: untrusted
  service-net supplies an opaque transcript hash, so the protected authority
  cannot prove the configured relay server. ADR-0008 now assigns the entire
  relay TLS endpoint to the authority and makes service-net a bounded byte
  carrier. The fixture remains unreachable from a relay client; Phase 4 Build
  and all protected prerequisites remain blocked.
- KMS/Silo Phase 2 is complete as a clean-cutover `DEV_REFERENCE`,
  AArch64-virtualized-QEMU-only provider. The public/general Silo handle,
  initialization, signing, ECDH, and raw-command surfaces are removed. Only the
  live KMS instance may reach the private, TLS 1.3 client
  `CertificateVerify`-purpose protocol; direct and unbound callers fail closed.
- Silo publishes readiness only after admission, VM load, one-time development
  initialization, guest readiness, and public-metadata validation. Its exact
  `/bin/silo` self-registration authority is test-hooks-only,
  non-manifestable, non-delegable, and limited to `service::SILO`. The locked
  guest is digest-admitted before launch (33,888/61,440 bytes; SHA-256
  `fea5cd2b9c36bb158e1e74b9e2c60209c133e0057292f0b9b4bc5f3e830838e4`);
  faults or resets permanently fail the instance closed with no retry or
  fallback.
- The exact signed 12-cell AArch64 QEMU lane passed registered readiness, KMS
  self-verification, direct/unbound denials, VFS PAGE+REG grant lifecycle, and
  `vfs-test` 96/0. This proves software custody/containment only. Because the
  same Cellos EL2 host constructs and loads the guest and supplies the
  disposable development seed, Stage-2 is not an independent hardware root or
  production qualification.
- KMS/Silo Phase 3 is complete as a constrained, non-production certificate
  provisioning slice. The supervisor-only lifecycle admits one pending
  generation, publishes a canonical bounded CSR through an ordered one-shot
  handle, requires live service-net profile staging, and activates only the
  exact staged generation/policy/profile tuple.
- The fresh nonce-bound pending P-256 key remains inside Silo. Abort, invalid
  CSR access, proof failure, and commit failure require confirmed cleanup or
  leave a fail-closed tombstone; persistence or activation disagreement seals
  serving rather than exposing mixed state. Service-net validates only
  allowlisted mounted profiles and bounded client-only certificate chains.
- Phase 3 deliberately does not claim an end-to-end default-runtime enrollment:
  runtime KMS remains sealed until protected persistence and authenticated time
  exist, and frozen opcode 14 exposes only the active key, not the pending key
  needed for authenticated precommit certificate binding.
- **PRODUCTION RELAY SIGNING BLOCKED:** fixture and development Silo providers
  are non-production. Cargo rejects unsafe provider/downgrade combinations,
  production artifacts exclude the development Silo, and
  `hardware-relay-provider` remains compile-blocked because
  [ADR-0006](decisions/0006-block-production-root-pending-exact-product-evidence.md)
  selected no product. Production is `BLOCKED_BY_ADR_0006`; no Phase 1–3
  result is hardware-backed signing or a production relay artifact.
- Phase 6 closed NO-GO. No exact product, procurement path, OTP/provisioning
  plan, or board/AP integration is approved. Reopening requires one
  vendor-signed package binding all eight ADR-0006 criteria to the same proposed
  deployment; receipt permits review, not approval. Every item must pass without
  inference and a superseding GO ADR must select the exact product before
  production implementation resumes.
- The KMS/Silo protected relay identity plan remains blocked on hardware assets
  and a named AWS DEV account/region. Its authorized software track has
  completed the Phase 2 `SOFTWARE_HARNESS`: a closed `no_std`, no-allocation
  authority protocol and protected-state model, literal private/public wire
  fixtures, and production-marker rejection. These host results satisfy no
  physical or live-cloud acceptance criterion and do not unblock Phases 3–5
  while Phase 1 admission remains blocked.
- Phase 1 admission validates the locked hardware inventory and AWS
  identity/account/region evidence but stays fail-closed: its currently
  authorized AWS commands cannot prove read-only permissions. Revising that
  evidence contract requires operator approval. Phases 3–5 physical work,
  Phases 6–8 integration/evidence, and production use remain blocked; only a
  superseding GO ADR may authorize the exact production product and trust
  chain.

## Update Rule

Keep this file short. Put maintained detail in the matching
`docs/roadmap/*.md` topic file. Do not edit the legacy snapshot; historical
delivery evidence belongs in `project-changelog.md`.
