## RISC-V SoC slice is already committed
**Verdict:** There is nothing left to commit for the last verified HAL checkpoint; the repo is clean and the RISC-V SoC-profile slice already landed on Monday, August 17, 2026.
- `HEAD` is `c6a31372` (`docs(hardware): document RISC-V SoC profiles`) and the paired code commit is `9372d870` (`refactor(hal): add RISC-V SoC profiles`).
- `git status --short --branch` shows `## fix/structure` with no pending file changes.
- The changelog records the slice as complete, including verification coverage for RV64 default, `board-vf2`, `board-pioneer`, AArch64 default, `board-rpi3`, release build, and QEMU boot.
**Source:** `git log --oneline -6`, `git status --short --branch`, `docs/project-changelog.md:5`

## PLIC IRQ policy is the smallest safe next slice
**Verdict:** If work resumes, the next narrow extraction should move RISC-V PLIC policy data into `hal/soc/riscv` while keeping PLIC register access and trap mechanics in `hal/arch/riscv`.
- `hal/soc/riscv` already owns RISC-V compatible lists and fail-closed access policies, including the PLIC compatible strings; this is the natural home for per-SoC IRQ context and enable lists.
- The remaining SoC-specific assumptions are still hardcoded in arch code: `hal/arch/riscv/src/common/plic.rs` assumes S-mode context `1`, enables only IRQs `1..=8` and `10`, and labels that as valid for QEMU virt and JH7110.
- Trap handling also bakes in context `1` for claim/complete in `hal/arch/riscv/src/rv64/trap.rs`, so policy is split across two arch files today.
- `kernel/src/main.rs` only injects `plic_base`; `PlatformInfo` remains stable and does not need new fields for a policy-only move.
**Source:** `hal/soc/riscv/src/catalog.rs:3`, `hal/arch/riscv/src/common/plic.rs:20`, `hal/arch/riscv/src/rv64/trap.rs:102`, `kernel/src/main.rs:107`

## Starting BCM2837/RPi3 next would be a larger coupled change
**Verdict:** Do not start the AArch64/RPi3 extraction as the next checkpoint unless the goal explicitly expands to board memory maps, UART choice, IRQ routing, and SDHCI/MMC wiring together.
- `kernel/src/platform.rs` hardcodes the RPi3 UART base, IRQ behavior, no-PLIC/no-VirtIO/no-RTC defaults, and the board log string in a dedicated AArch64 branch.
- RPi3 board facts are also duplicated in boot fallback memory, resource-registry MMIO allowlists, paging MMIO mapping, UART initialization branches, and multiple MMC/SDHCI modules.
- The MMC stack is already real-board validated and spans `mmc.rs`, `sdhci.rs`, `pinmux_rpi3.rs`, and RPi3-specific cfg branches; moving only part of that would create a split-source configuration hazard.
- The roadmap and the completed SoC-profile plan both explicitly defer AArch64/RPi3/SDHCI extraction after the RISC-V SoC slice.
**Source:** `kernel/src/platform.rs:101`, `kernel/src/boot.rs:347`, `kernel/src/resource_registry.rs:56`, `kernel/src/memory/paging.rs:263`, `kernel/src/task/drivers/mmc.rs:16`, `.agents/260818-0513-hal-soc-riscv-profiles/plan.md:18`, `docs/project-roadmap.md:27`

## Scope boundary for the next slice should stay strict
**Verdict:** Keep the next move data-only and preserve both `PlatformInfo` and the public ABI boundary.
- `docs/code-standards.md` marks `libs/api/` and `libs/types/` as sacred interfaces requiring explicit confirmation for change.
- The completed plan for the SoC slice preserved `PlatformInfo`, root `boards/`, shared drivers in `cells/drivers/`, and board feature semantics; the next slice should keep the same contract.
- The architecture docs repeat the same ownership model: root `boards/` for board descriptors, `hal/soc/riscv` for data-only SoC facts, shared drivers still in `cells/drivers/`.
- There is minor metadata drift: the completed plan file says `created: 2026-08-18` even though the current date is Monday, August 17, 2026; treat that as plan-artifact noise, not implementation evidence.
**Source:** `docs/code-standards.md:12`, `.agents/260818-0513-hal-soc-riscv-profiles/plan.md:22`, `docs/system-architecture.md:50`, `.agents/260818-0513-hal-soc-riscv-profiles/plan.md:11`

## Rollback and verification for a PLIC-policy slice
**Verdict:** The rollback should be one focused commit restoring `hal/arch/riscv/src/common/plic.rs` and `hal/arch/riscv/src/rv64/trap.rs`; verification must stay RV64-centric and hardware-honest.
- Minimum rollback target: restore hardcoded context/IRQ policy in those two arch files without touching `boards/`, `PlatformInfo`, or shared drivers.
- Compile gates should cover RV64 default, `board-vf2`, and `board-pioneer`; AArch64 default and `board-rpi3` remain regression checks only.
- Runtime evidence should remain QEMU-only unless new physical-board logs exist; do not claim Pioneer or VF2 hardware validation from compile success.
- Exact commands: `cargo fmt --all -- --check`; `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu`; `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf`; `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2`; `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-pioneer`; `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat`; `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`; `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf`; `scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`.
**Source:** `hal/arch/riscv/src/common/plic.rs:90`, `hal/arch/riscv/src/rv64/trap.rs:176`, `docs/project-changelog.md:17`, `docs/project-roadmap.md:29`
