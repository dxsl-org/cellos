# Cellos Project Roadmap

**Project**: Cellos (Jarvis Hybrid OS)
**Current version**: 0.2.1-dev (Mycelium Era)
**Current phase**: Phase 1 - Core Stability; active product stage G1 Robot & Embedded
**Last updated**: 2026-08-21

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
  byte and legacy `TIER_*` names; Tier 2 native domains remain unimplemented.
  Any implementation must first satisfy the mandatory [Spec 22 native-domain
  gate](specs/22-native-domain-cell-implementation-gate.md), including
  recoverable user-pointer handling, revoke/teardown, DMA, negative-test, and
  rollback evidence.

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
- Tier 2 native domains have an accepted design gate but no runtime mechanism;
  current native cells remain in the shared SAS and are not treated as
  contained merely because the manifest taxonomy names a future protection
  class.
- The Native SDK contract is ratified and the authoritative Phase 02
  [acceptance ledger](app-tier-acceptance-ledger.json) is implemented. Its
  validator and CI gate have been reviewed, but Phase 02 remains in progress.
  Its next state transition is pending a ratified Git revision plus one
  adjacent append-only lifecycle event. The current result is `NOT_COMPLETE`:
  compile, test/runtime, delivery, hardware, admission, and hostile-test
  witnesses remain mandatory before any applicable SDK cell can be promoted to
  `USABLE`; FFI, `rust-std`, and Tier-2 scopes without ratified applicability
  remain non-qualifying.
- Net-broker has implemented pieces for Noise/identity/routing, but `main.rs`
  still marks K1 loading, beacon sockets, relay dispatch, leases, and enrollment
  as TODO wiring.

## Update Rule

Keep this file short. Put maintained detail in the matching
`docs/roadmap/*.md` topic file. Do not edit the legacy snapshot; historical
delivery evidence belongs in `project-changelog.md`.
