# Cellos Project Roadmap

**Project**: Cellos (Jarvis Hybrid OS)
**Current version**: 0.2.1-dev (Mycelium Era)
**Current phase**: Phase 1 - Core Stability; active product stage G1 Robot & Embedded
**Last updated**: 2026-08-27
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

## Capability Lanes

| Capability | Execution class | Evidence ceiling | Owner / next slice | Reopening event |
|---|---|---|---|---|
| Roadmap projection | `ready` | `contract` | Hardware-independent roadmap Phase 01/08 | A lane emits bounded evidence/status |
| RPi3 HDMI software boundary | `governance-gated` | `host` | RPi3 HDMI Phase 04 | Named reviewer approval for `cells/drivers/bcm-display/src/mailbox.rs` unsafe DMA-page copies, or an equivalent safe redesign |
| Tier 3 hostile QEMU evidence | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 06 | Add VMM/VirtIO transport for bounds, descriptors, and backend errors, plus independent preemption and supervisor-restart outcomes |
| ARM64 Tier 3 persistent storage | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 09 | Supported Phase 06 hostile scenarios; policy is fixed to `build/tier3-arm64-persistent.img` at 8 MiB with explicit cleanup |
| Desktop, ViUI, and SDK | `governance-gated` | `qemu` | Managed-surface child is implementation-complete | Restore signed-image F1 policy: `hypha-llm-gateway` must forbid unsafe code; BCM unsafe use requires a reviewed allowlist entry |
| Local Cell-to-Cell runtime | `scope-gated` | `host` | Cell-to-Cell Anywhere Recovery Plan Phase 01 | Implement the approved ephemeral K1 injection for the RV64 `app-bench` oracle, then record local IPC, queue/cache, and saturation baselines |
| Kernel signature, pointer, and entropy remediation | `governance-gated` | `host` | Separately approved security children | Named security/PAL approvals and implementation checkpoints |
| Authenticated software evidence | `scope-gated` | `host` | Hardware-independent roadmap Phase 07 | Run `.github/workflows/ci.yml` on `main`, then verify its immutable attested bundle; only software/QEMU classes are eligible |
| x86 Tier 3 VirtIO parity | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 10 | Supported Phase 06 scenarios, shared persistence backend, then one pinned transport contract |
| Protected relay identity | `external-gated` | `host` | KMS/Silo protected relay plan | VF2, STM32H573, OPTIGA TPM, and named AWS DEV account/region |
| G3 accelerator | `external-gated` | `contract` | Accelerator evidence envelope | RK3588, accepted RKNN package/license, then X390 evidence |
| Physical boards and production root | `external-gated` | `physical` / `production` | Hardware tracks and ADR-0006 | Exact board logs; vendor package and superseding GO ADR |

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
- Tier 3 x86 QEMU qualification reaches the pinned Alpine 3.21.7
  Linux 6.12.81 `/bin/sh` BusyBox prompt under QEMU-TCG 10.2.0 at both 1 GiB
  and 2 GiB. Ubuntu 24.04's QEMU-TCG 8.2.2 remains an explicit compatibility
  risk; physical x86 qualification remains hardware-gated.
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
  Focused tests and the RISC-V build pass; QEMU runtime qualification remains
  unrun because `disk_v3.img` is absent.

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
  non-promotional benchmark validator. `PAL-019` remains Deferred while the
  default development tuple reports predictable `dev-weak-rng` bytes as
  successful entropy over a zero-byte RNG source. `PAL-031` technical backing
  now has a bounded caller-owned GetRandom output implementation and isolated
  RV64 QEMU hostile evidence, including final-write races against root
  retirement, grant revoke, and exact backing-frame reuse. The governed
  security manifest now binds this evidence; the authoritative support map
  still classifies `PAL-031` as Deferred pending every named approval.
  This runtime evidence does not grant PAL approval, production entropy, or any
  remaining qualification gate. There is no PAL, target, sysroot, runtime,
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
