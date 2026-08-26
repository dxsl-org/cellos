---
title: "RISC-V SoC Profile Slice"
description: "Extract RISC-V SoC profile facts into hal/soc without duplicating shared drivers or changing board descriptors."
status: completed
priority: P2
effort: 1.5d
branch: fix/structure
tags: [refactor, architecture, hal, riscv]
blockedBy: []
blocks: []
created: 2026-08-18
---

# RISC-V SoC Profile Slice

## Verdict

Plan the smallest executable `hal/soc` extraction: a data-only `hal/soc/riscv` profile crate consumed by the RV64 platform path. Defer BCM27xx/MMC, fallback-memory migration, PLIC interrupt policy, RPi3 hardware paths, and feature-collapse.

## Scope Contract

- Deliver: `hal/soc/riscv` immutable profiles for generic QEMU virt, JH7110/VF2, and SG2042/Pioneer; kernel RV64 platform selection consumes those profile facts.
- Preserve: root `boards/` as board descriptor owner, `PlatformInfo` fields, all shared driver crates under `cells/drivers/`, and existing `board-vf2`, `board-pioneer`, `board-rpi3`, `qemu-virt-1g` build semantics.
- Exclude: copied UART/PLIC/CLINT/RTC/VirtIO drivers, `hal/core` feature collapse, `kernel/src/boot.rs` fallback-map migration, and any ARM/RPi3/MMC/SDHCI extraction.
- Invariant: no `libs/api` or `libs/types` ABI change; `docs/code-standards.md:12` marks those interfaces sacred.

## Phases

| Phase | Name | Status | Depends |
|---|---|---:|---:|
| 1 | [Define RISC-V SoC Profiles](./phase-01-define-riscv-soc-profiles.md) | completed | - |
| 2 | [Consume Profiles in RV64 Platform](./phase-02-consume-profiles-in-platform.md) | completed | 1 |
| 3 | [Gate Compatibility and Document Handoff](./phase-03-gate-compatibility-and-handoff.md) | completed | 2 |

## Data Flow

Firmware DTB or fallback board descriptor enters `kernel/src/platform.rs:81`; profile selection maps the active Cargo feature to compatible lists and access quirks; DTB parsing fills `PlatformInfo` at `kernel/src/platform.rs:236`; existing callers read unchanged fields in `kernel/src/main.rs:109`, `kernel/src/memory/paging.rs:184`, `kernel/src/task/drivers/virtio_common.rs:46`, and `kernel/src/task/drivers/uart.rs:143`.

## Dependency Graph

Phase 2 cannot start before Phase 1 exposes the no-alloc profile contract. Phase 3 cannot start before Phase 2 proves the kernel path compiles. Cross-plan dependency: completed board descriptor slice at `.agents/260817-2207-cellos-board-soc-hal-split/plan.md`; it delivered the root `boards/` owner that this plan preserves.

## Backwards Compatibility

Existing features stay source-compatible: `board-vf2` remains a JH7110 selection, `board-pioneer` keeps SBI DBCN console/no RTC/no VirtIO behavior, default RV64 remains QEMU virt, and `board-rpi3` continues propagating to `hal-arm` outside this plan (`kernel/Cargo.toml:84`, `kernel/Cargo.toml:87`, `kernel/Cargo.toml:90`, `kernel/Cargo.toml:99`).

## Test Matrix

- Unit: `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu`.
- Integration compile: RV64 default, `--features board-vf2`, `--features board-pioneer`.
- Regression compile: AArch64 default and `--features board-rpi3`.
- E2E: release RV64 kernel build plus `scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`.

## Risk Register

- High likelihood/High impact: Pioneer console regression if `uart_base=0`/`rtc_base=0`/no VirtIO is not preserved from `kernel/src/platform.rs:91`. Mitigation: profile unit tests and `board-pioneer` compile gate; rollback removes the new crate dependency and restores the three assignments.
- Medium likelihood/High impact: profile crate accidentally becomes a board descriptor clone. Mitigation: scalar policy only; no memory maps, pinmux, driver implementations, or `PlatformInfo` ownership.
- Medium likelihood/Medium impact: `hal/soc` feature sprawl mirrors existing flat board features. Mitigation: one kernel-side selector function, no `hal/core` propagation in this pass.

## Validation Log

- Claims checked: 18 | Verified: 18 | Failed: 0 | Unverified: 0. Tier: Standard.
- Red-team result: accepted top risk is SG2042 console behavior drift; mitigated by explicit unit and compile gates. Deferred finding: PLIC IRQ policy remains in `hal/arch/riscv/src/common/plic.rs:92` and `hal/arch/riscv/src/rv64/trap.rs:103`, intentionally outside this slice.
- Artifact validator: absent in this checkout; manual JSON validation recorded in `reports/harness/verification.json`.

## Unresolved Questions

- None for this slice. The deferred question is whether the next plan should move PLIC IRQ policy or start the AArch64/RPi3 board descriptor first.
