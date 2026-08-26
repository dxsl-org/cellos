---
phase: 1
title: "Define RISC-V SoC Profiles"
status: completed
priority: P2
effort: "0.5d"
dependencies: []
tier: medium
---

# Phase 1: Define RISC-V SoC Profiles

> **Required - deviation-log:** Log every Decision / Deviation / Surprise in this file when it occurs.

## Overview

Create a no_std, data-only `hal-soc-riscv` crate under `hal/soc/riscv` that describes SoC/profile facts used by the RV64 platform path.

## Requirements

- Functional: expose immutable profiles for `generic-virt`, `jh7110`, and `sg2042`.
- Functional: encode compatible strings for UART, PLIC, CLINT, RTC and access policies for UART, RTC, and VirtIO MMIO.
- Non-functional: no allocation, no kernel types, no board wiring, no memory maps, no driver implementations.

## Architecture

Inputs are Cargo feature intent and firmware DTB compatibles. The profile crate transforms them into static lookup policy: compatible arrays and access flags. Output is scalar data consumed by `kernel/src/platform.rs`, whose `PlatformInfo` fields remain unchanged at `kernel/src/platform.rs:25`.

Suggested public contract:

- `RiscvSocProfile { slug, uart_compatibles, plic_compatibles, clint_compatibles, rtc_compatibles, uart_access, rtc_access, virtio_mmio }`
- `MmioAccess::{Mmio, SbiDbcnOnly, Unavailable}`
- `VirtioMmioPolicy::{Discover, Absent}`
- constants/functions: `GENERIC_VIRT`, `JH7110`, `SG2042`

## Assumptions

None - no unverified claims.

## Related Files

- Create: `hal/soc/riscv/Cargo.toml`
- Create: `hal/soc/riscv/src/lib.rs`
- Modify: `Cargo.toml`

## File Ownership

This phase owns only the new crate and root workspace membership. Later phases must not edit the profile contract except to fix compile errors discovered while wiring.

## Implementation Steps

1. Add workspace member `hal/soc/riscv` near existing HAL members in `Cargo.toml:34`.
2. Add `hal/soc/riscv/Cargo.toml` with package name `hal-soc-riscv`, version aligned with HAL crates, no default features, and no dependencies unless a real compile need appears.
3. Define profile structs/enums as `Copy + Debug + Eq + PartialEq`; keep all strings `&'static [&'static str]`.
4. Encode QEMU/JH7110 compatible arrays from current platform lookups: UART includes `ns16550a`, `ns16550`, `snps,dw-apb-uart` from `kernel/src/platform.rs:203`; PLIC includes `sifive,plic-1.0.0`, `riscv,plic0`, `thead,c900-plic` from `kernel/src/platform.rs:214`; CLINT includes `sifive,clint0`, `riscv,clint0`, `thead,c900-clint` from `kernel/src/platform.rs:223`; RTC includes `google,goldfish-rtc` from `kernel/src/platform.rs:231`.
5. Encode SG2042 as UART `SbiDbcnOnly`, RTC `Unavailable`, VirtIO `Absent`, matching `kernel/src/platform.rs:91`.
6. Add host unit tests proving generic/JH7110 discover VirtIO and SG2042 disables MMIO UART/RTC/VirtIO.

## Success Criteria

- [x] `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu` passes.
- [x] `hal-soc-riscv` does not depend on `cellos-kernel`, `cellos-boards`, `libs/api`, or `libs/types`.
- [x] No `cells/drivers/*` file is added or modified.

## Security Considerations

The crate must not widen device access. An `Unavailable` or `Absent` policy must fail closed by causing kernel consumers to keep zero bases or empty VirtIO slots.

## Risk Notes

- Risk: generic profile treats QEMU virt as a real SoC. Mitigation: name it `generic-virt` and document it as a machine/profile baseline only.
- Risk: profile data duplicates board descriptors. Mitigation: do not include `BoardDescriptor` fields from `boards/src/descriptor.rs:59` such as boot contract, wiring, fallback memory, or enabled drivers.
- Rollback: remove `hal/soc/riscv` and the single workspace member line. No runtime behavior changes exist until Phase 2.

## Deviation Log

- Decision: replaced the suggested shared `MmioAccess` enum with `UartAccessPolicy` and `RtcAccessPolicy` so kernel wiring can pattern-match by device class without overloading one cross-device state type; this is reversible because the profile data remains scalar and layout-local.
- Decision: removed the `profiles()` table and `profile_by_slug()` runtime lookup after confirming Phase 2 will select the three public constants directly; this keeps Phase 1 data-only surface at the minimum needed for the planned kernel consumer.
- Decision: expanded the SG2042 unit test after review to assert `thead,c900-plic` and `thead,c900-clint` remain in the compatible lists, so the fail-closed UART/RTC/VirtIO policy does not mask interrupt-controller compatibility regressions.
