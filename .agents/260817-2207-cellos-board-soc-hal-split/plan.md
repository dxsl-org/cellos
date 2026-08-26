---
title: "Cellos Board Descriptor Slice"
description: "Introduce declarative board packages and route QEMU RV64 boot/platform data through the first descriptor consumer."
status: completed
priority: P2
effort: 2d
branch: fix/structure
tags: [refactor, architecture, hal]
blockedBy: []
blocks: []
created: 2026-08-17
---

# Cellos Board Descriptor Slice

## Verdict

Completed the first executable migration slice: a typed declarative board schema plus a QEMU RV64 descriptor consumer. `hal/soc/` extraction and all RPi3/SDHCI work stay deferred to separately gated plans.

## Scope Contract

- Delivered: root `boards/qemu/virt-riscv64/` package, typed kernel-side descriptor model, and QEMU RV64 boot/platform reads from descriptor data.
- Preserved: current RV64 fallback memory map, DTB-derived memory path, UART/PLIC/CLINT/VirtIO defaults, and existing Cargo feature compatibility.
- Excluded: RPi3 linker path, SDHCI quirks, AArch64 `hal/arch/arm` cleanup, `libs/api`/`libs/types` ABI changes, copied per-board drivers.

## Phases

| Phase | Name | Status | Depends |
|---|---|---:|---:|
| 1 | [Define Board Package Contract](./phase-01-board-package-contract.md) | completed | - |
| 2 | [Consume QEMU RV64 Descriptor](./phase-02-qemu-rv64-descriptor-consumer.md) | completed | 1 |
| 3 | [Gate Compatibility and Handoff](./phase-03-compatibility-gates-and-handoff.md) | completed | 2 |

## Dependency Graph

`docs/TODO.md:5-25` sets the target layering. `Cargo.toml:33-70` shows HAL and shared Driver Cell workspace ownership. `kernel/src/platform.rs:43-56` is the active platform data shape. `kernel/src/boot.rs:240-265` and `kernel/src/boot.rs:477-515` are the RV64 fallback and DTB paths preserved by the slice.

## Backwards Compatibility

The slice kept `--features board-vf2`, `board-pioneer`, `board-rpi3`, `board-rpi4`, and `qemu-virt-1g` semantics unchanged. QEMU RV64 now has a default descriptor path; legacy feature branches remain until later plans remove them with boot evidence.

## Test Matrix

- Unit: descriptor validation rejects mismatched architecture, empty compatibles, and overlapping fallback ranges.
- Integration: `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`.
- E2E: `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`.
- Regression: AArch64 cargo check stayed green; AArch64 boot is advisory for this slice because no AArch64 source path changed.

## Top Risks

- High: boot regression from fallback memory drift. Mitigation: byte-for-byte descriptor constants mirrored `kernel/src/boot.rs:240-265`, then QEMU boot gate was mandatory.
- Medium: descriptor model becomes a new config framework. Mitigation: QEMU RV64 only; no generator, no `hal/soc` scaffold, no schema beyond facts consumed now.
- Medium: board features remain in generic code after the slice. Mitigation: document as deferred debt, add grep guard only for new QEMU RV64 descriptor paths.

## Handoff

No further work remains in this plan. The next independently reversible slice is `hal/soc` extraction, followed by the deferred AArch64/RPi3 and SDHCI board cleanups.

Decision: the first slice uses one `no_std` Rust descriptor as the canonical board configuration. A TOML resolver remains deferred until a second board proves that generation is needed; this avoids two hand-maintained sources of truth.
