# Plan — aarch64 core/backend split (ADR-0029 §6)

**Status:** admitted 2026-11-02 ([handoff](lock-handoff-approval.md)); phases 01–04 in progress.
**Authority:** [ADR-0022 amendment 2026-10-31](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md); [ADR-0029](../../docs/decisions/0029-platform-image-families-and-runtime-detection.md) §5–6
**Scope rule:** behaviour-preserving only. No new backend, driver, peripheral or bring-up. It yields file ownership to any active Intel C2C slice.
**Evidence ceiling:** QEMU `virt` (CI) + QEMU `raspi3b` (manual `run-rpi3.ps1`) + RPi4 compile-only. No physical evidence is claimed.
**Recon:** [scout-report.md](scout-report.md)

## Goal

The shared aarch64 core (`hal/arch/arm`, arch-generic `kernel/src`) contains no `cfg(feature = "board-rpi*")`. Board behaviour lives in backends behind traits:
- `qemu-virt`
- `bcm2837`
- `bcm2711`

Group 1 boards keep compile-time selection, but of a backend type, not of scattered branches.

## Phases

| # | Phase | Status | Depends | Checkpoint |
|---|---|---|---|---|
| 01 | [Backend trait and console/diagnostics](phase-01-backend-trait-console.md) | pending | — | no |
| 02 | [IRQ, timer, trap, SMP](phase-02-irq-timer-trap-smp.md) | pending | 01 | **yes** |
| 03 | [Platform map, resources, driver selectors](phase-03-platform-map-resources.md) | pending | 01 | no |
| 04 | [Boot/DTB, board selection, EL2/vCPU monitor](phase-04-boot-el2-vcpu.md) | pending | 02, 03 | **yes** |

## Key design decisions

- **Static dispatch.** `pub type Board = backend::<selected>::Board;` is chosen by exactly one `cfg` in a single `hal/arch/arm/src/aarch64/backend/mod.rs`. Traits are implemented on zero-sized types, so there is no `dyn` and no runtime cost. The existing `hal/traits/interrupt` and `hal/traits/timer` are reused where their signatures fit; aarch64-specific needs (claim/complete, SGI) go in an `ArmIrqController` extension trait.
- **Data stays in `hal/soc/*` profiles** (`ArmVirtProfile`, `Bcm27xxSocProfile`). Backends wrap the profiles, so there is no new address table.
- **RPi3 vCPU layout difference** (`vcpu.rs:120-164`) becomes `Board::VcpuExt` (an associated type). The layout and ABI are unchanged for each build.
- **Design hooks for groups 2 and 3** are limited to trait shape (IOMMU trait with a `None` impl for all current backends). No implementations.

## Assumptions (unverified)

- A1: `run-rpi3.ps1` QEMU `raspi3b` boots to the current shell on the maintainer's machine. It is the only RPi3 runtime evidence available.
- A2: The RPi4 build has no runtime lane. Only compile and size-diff checks are possible.
- A3: The trait boundary adds no measurable boot-time cost (static dispatch). Verified by the release-binary symbol and size diff.

## Behaviour-preservation gate (every phase)

- Build matrix: `board-rpi3`, `board-rpi3-bring-up`, `board-rpi4` and no-feature `virt`.
- `.text` size within ±1% per variant against the pre-plan baseline, recorded in `reports/`.
- QEMU `virt`: `scripts/qemu-aarch64-test.sh` and `scripts/qemu-aarch64-test-hooks.sh` (also `QEMU_SMP=2`).
- RPi3: the `run-rpi3.ps1` boot log matches the baseline banner/prompt sequence.

## Rollback

Each phase is a pure move/refactor commit set and is independently revertable. No ABI, on-disk or wire change.
