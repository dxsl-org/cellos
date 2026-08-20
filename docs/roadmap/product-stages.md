# Product Stages

**Last updated**: 2026-08-20

## G1 - Robot & Embedded

Goal: ship a bounded, fast-booting, never-die OS for RV64/ARM64 SBC-class
robot and embedded systems.

Required evidence:

- Real board boot evidence for promoted hardware lanes.
- Peripheral I/O through capability-gated driver cells or audited kernel
  integration paths.
- Bounded memory and stack posture per Cell.
- Clear separation between QEMU integration proof and physical hardware proof.

## G2 - Server & Specialized PC

Goal: scale the same SAS/LBI model to x86_64 and server-class deployments with
SMP, larger storage, desktop/tooling depth, and zero-downtime service upgrade.

Current posture:

- x86_64 has implementation and QEMU/Ring-3 smoke evidence, but physical PC
  qualification remains target-specific.
- Untrusted Linux/POSIX application compatibility belongs in Tier 3 VM paths,
  not native Tier 1 cells.

## G3 - NPU-native Compute OS

Parked until hardware exists and the team has vendor API experience. The
contract for accelerators must be hardware-informed; avoid over-specifying
`ViAccelerator` before RKNN/Hailo/K230/P870-class evidence exists.

The first evidence target is RK3588/RKNN; X390 remains the second implementation
after usable silicon and software are available. The maintained readiness and
license gates are in [G3 Accelerator Evidence Envelope](../research/g3-accelerator-evidence.md).

## G4 - Full Rust std for Tier 1 Cells

Direction: a Tier 1 `rust-std` runtime profile using pure-Rust PAL plus a custom
`*-unknown-cellos` rustc target. Do not route native Tier 1 `std` through mlibc,
because that pulls C/POSIX assumptions into the trusted Tier 1 path.

## G5 - Virtualization Platform

Research/design overlay after G4. The intended shape is one VMM core with
profiled Tier 3 guest modes, not two separate codebases. Golden-frame poisoning
remains a named trust-anchor risk before production use.
