# Scout Report: Cellos Board Descriptor Slice

## Relevant Files and Roles

- `docs/TODO.md:5-25` states the desired `hal/arch`, `hal/soc`, root `boards` layout and forbids per-board UART, SDHCI, DesignWare I2C/SPI, GIC/PLIC, and PCIe driver copies.
- `docs/code-standards.md:12-20` makes `libs/api` and `libs/types` stable ABI; this slice must not touch them.
- `docs/specs/04-hardware.md:17-19` distinguishes platform HAL from Driver Cells; `docs/specs/04-hardware.md:29-32` makes DTB the MMIO registry source.
- `docs/specs/04-hardware.md:63` requires board-agnostic HAL traits with no kernel changes for new real-board implementations.
- `Cargo.toml:33-70` keeps HAL crates under `hal/*` and reusable Driver Cells under `cells/drivers/*`; drivers stay shared.
- `kernel/Cargo.toml:83-100` currently stores board/memory/firmware distinctions as flat Cargo features.
- `hal/core/Cargo.toml:40-41` propagates `board-rpi3` into `hal-arm`, a known leak but out of this first slice.
- `kernel/build.rs:10-25` selects the RPi3 linker script by board feature; unchanged in this slice.
- `kernel/src/platform.rs:43-56` defines `PlatformInfo`; `kernel/src/platform.rs:119-148` initializes RV64 from DTB/defaults.
- `kernel/src/platform.rs:151-209` hardcodes RPi3 and QEMU AArch64 defaults; unchanged in this slice.
- `kernel/src/boot.rs:240-265` defines QEMU RV64 fallback memory; `kernel/src/boot.rs:477-515` builds a DTB memory map first and falls back closed.
- `kernel/src/task/drivers/uart.rs:139-153` already reads UART base via `platform::with`, proving descriptor-fed platform data is compatible with existing UART init.
- `kernel/src/memory/paging.rs:184-198` maps MMIO from `platform::with`, so platform descriptor drift can break paging before drivers run.
- `scripts/qemu-boot-test.sh:8-13` defines the RV64 boot gate and warns build-only checks are insufficient.

## Patterns to Preserve

- Keep early boot allocator-free; descriptor data used before heap/MMU should be static or generated Rust constants.
- Keep existing `PlatformInfo` consumers stable for the first slice; add fields only if every consumer is enumerated.
- Keep board executable code out of this slice; QEMU RV64 is data-only.
- Do not create board-local driver crates. `cells/drivers/*` remains shared workspace ownership.

## Prior Research Findings

- PRIOR: Zephyr/Linux/U-Boot converge on board data, SoC integration, and shared controller drivers rather than per-board driver forks; see `.agents/reports/research-260817-cellos-soc-board-layering.md`.
- PRIOR: The accepted architecture decision is root `boards/`, not `hal/boards`; see `.agents/reports/research-260817-board-soc-driver-split.md`.

## Precedents

- Precedent mining was skipped for final artifact writing because WSL failed with `Wsl/Service/E_UNEXPECTED` during `git grep`; current task explicitly stopped further exploration.
- Baseline status from parent agent: RV64/AArch64 cargo checks are green before this plan.

## Prior Failures / Incidents

- Host-gated: WSL can intermittently fail with `Wsl/Service/E_UNEXPECTED`; do not interpret that as a Cellos build failure.
- Active plan sync attempted `node .claude/scripts/set-active-plan.cjs ...`, but `.claude/scripts/set-active-plan.cjs` is absent in this checkout. Plan artifacts were written directly.

## Blast Radius

- Primary: `kernel/src/platform.rs`, `kernel/src/boot.rs`, new `kernel/src/board*.rs` modules, new root `boards/qemu/virt-riscv64/*`.
- Secondary: `kernel/Cargo.toml`, `Cargo.toml`, `docs/system-architecture.md`, `docs/project-roadmap.md`, CI/scripts only if a validation command needs a named board target.
- Explicitly untouched: `hal/arch/arm/*`, `kernel/src/task/drivers/mmc*`, `kernel/build.rs` RPi3 linker selection, `cells/drivers/*` implementations.

## Inconsistencies to Note but Not Fix

- `board-rpi3` still reaches `hal-arm` through `hal/core/Cargo.toml:40-41`.
- `kernel/src/task/drivers/mmc.rs:16-33` and `kernel/src/task/drivers/mmc/sdhci.rs:18-21` contain board feature logic; this remains deferred.
- `kernel/src/platform.rs:151-209` still hardcodes AArch64 board defaults; this remains deferred.
