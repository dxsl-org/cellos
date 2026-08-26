# Scout Report: RISC-V SoC Profile Slice

## Relevant Files and Roles

- `Cargo.toml:34` lists HAL workspace members; new `hal/soc/riscv` belongs beside `hal/arch/*`, not under `boards/`.
- `kernel/Cargo.toml:33` scopes RV64-only dependencies; `cellos-boards` is already RV64-only at `kernel/Cargo.toml:37`.
- `kernel/Cargo.toml:84` keeps `qemu-virt-1g`; `kernel/Cargo.toml:87` keeps `board-vf2`; `kernel/Cargo.toml:90` keeps `board-pioneer`; `kernel/Cargo.toml:99` keeps `board-rpi3` and still propagates to HAL.
- `kernel/src/platform.rs:25` defines the public `PlatformInfo` shape used by drivers and paging.
- `kernel/src/platform.rs:81` is the RV64 platform entrypoint; `kernel/src/platform.rs:91` contains the Pioneer-specific policy mutation.
- `kernel/src/platform.rs:203`, `kernel/src/platform.rs:214`, `kernel/src/platform.rs:223`, and `kernel/src/platform.rs:231` contain DTB compatible arrays that are profile facts.
- `kernel/src/platform.rs:271` collects VirtIO MMIO nodes and should stay in kernel because it returns kernel-local `VirtioEntry`.
- `kernel/src/main.rs:109`, `kernel/src/memory/paging.rs:184`, `kernel/src/task/drivers/virtio_common.rs:46`, and `kernel/src/task/drivers/uart.rs:143` are observed `PlatformInfo` consumers.
- `kernel/src/boot.rs:291` keeps VF2 fallback DRAM data; this plan defers moving fallback memory.
- `boards/src/descriptor.rs:59` defines board descriptor ownership; `boards/qemu/virt-riscv64/board.rs:95` is the first descriptor instance.
- `hal/core/Cargo.toml:40` shows the existing ARM board-feature leak; explicitly deferred.
- `hal/arch/riscv/src/common/plic.rs:92` and `hal/arch/riscv/src/rv64/trap.rs:103` still encode interrupt-source policy; explicitly deferred.

## Patterns to Preserve

- Early boot data must be static/no-alloc.
- Keep `PlatformInfo` stable unless all observed consumers are updated.
- Shared drivers remain under `cells/drivers/`; SoC profiles select facts/policy only.
- Board descriptors keep boot contract, fallback memory, wiring, and enabled driver lists.

## Precedents

- `c0096ade refactor(hal): add board descriptor layer` added root `boards/` and changed `kernel/src/platform.rs`/`kernel/src/boot.rs`.
- `9427482f docs(hardware): document board split` recorded the completed board descriptor slice.
- `45d4a175 docs(hardware): define board support priorities` documents architecture -> SoC -> board progression.

## Prior Failures / Incidents

- No `.agents/failure-history.jsonl` or `.agents/incidents/*` files were present.
- Prior WSL instability is environmental, not a Cellos failure; use native WSL Git/build and record host-gated checks honestly.
- `.claude/scripts/set-active-plan.cjs` is absent, so active-plan sync could not be run.

## Blast Radius

- Planned source: `Cargo.toml`, `kernel/Cargo.toml`, `kernel/src/platform.rs`, `hal/soc/riscv/Cargo.toml`, `hal/soc/riscv/src/lib.rs`.
- Planned docs after verification: `docs/system-architecture.md`, `docs/project-roadmap.md`, `docs/project-changelog.md`.
- Explicitly untouched: `kernel/src/boot.rs`, `boards/**`, `cells/drivers/**`, `hal/core/**`, `hal/arch/arm/**`, `kernel/src/task/drivers/mmc*`, `libs/api/**`, `libs/types/**`.

## Inconsistencies to Note but Not Fix

- ARM/RPi3 board feature still reaches `hal-arm` through `hal/core/Cargo.toml:40`.
- PLIC IRQ enable/dispatch still hardcodes VirtIO 1-8 and UART 10 in `hal/arch/riscv`.
- VF2 fallback memory still lives in `kernel/src/boot.rs:291` because this plan keeps fallback maps as board/firmware contract.
