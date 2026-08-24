# Cellos Project Roadmap

**Project**: Cellos (Jarvis Hybrid OS)
**Current version**: 0.2.1-dev (Mycelium Era)
**Current phase**: Phase 1 - Core Stability; active product stage G1 Robot & Embedded
**Last updated**: 2026-08-24
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
  successful entropy over a zero-byte RNG source; `PAL-031` remains Deferred
  while `GetRandom` lacks bounded caller-owned writable output validation.
  There is no PAL, target, sysroot, runtime, live capture, approval, or
  promotion. All six approval rows remain `NOT GRANTED`, the implementation
  checkpoint is `BLOCKED`, and umbrella Phase 06 remains pending and
  dependency-blocked on Phase 03. Maintained detail is in
  [runtime-and-platform-tracks.md](roadmap/runtime-and-platform-tracks.md).
- [Spec 18c Publisher Provenance Envelope](specs/18c-publisher-provenance-envelope.md)
  is **proposed**, pending security-owner and independent-reviewer approval. It
  introduces no producer, parser, production profile, or admission path; it
  does not alter the ledger's Phase 03 `PLANNED` status or unblock production
  work. Production admission remains disabled pending qualified and approved
  external-floor and owner-record gates.
- Net-broker has implemented pieces for Noise/identity/routing, but `main.rs`
  still marks K1 loading, beacon sockets, relay dispatch, leases, and enrollment
  as TODO wiring.

## Update Rule

Keep this file short. Put maintained detail in the matching
`docs/roadmap/*.md` topic file. Do not edit the legacy snapshot; historical
delivery evidence belongs in `project-changelog.md`.
