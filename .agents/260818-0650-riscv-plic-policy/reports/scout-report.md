# Scout Report: RISC-V PLIC Policy

## Current State

- OBSERVED: `hal/soc/riscv` is `#![no_std]` and explicitly data-only, carrying compatibles and access policies without board wiring or drivers at `hal/soc/riscv/src/lib.rs:1` and `hal/soc/riscv/src/lib.rs:5`.
- OBSERVED: `RiscvSocProfile` currently contains `slug`, compatible lists, and UART/RTC/VirtIO access policies, but no PLIC context policy at `hal/soc/riscv/src/profile.rs:8`.
- OBSERVED: `GENERIC_VIRT`, `JH7110`, and `SG2042` are current profile constants at `hal/soc/riscv/src/catalog.rs:9`, `hal/soc/riscv/src/catalog.rs:21`, and `hal/soc/riscv/src/catalog.rs:28`.
- OBSERVED: `PlatformInfo` already stores `uart_irq`, `plic_base`, `plic_size`, and `[Option<VirtioEntry>; 8]` at `kernel/src/platform.rs:27`.
- OBSERVED: RV64 `platform::init` selects the active SoC profile before DTB parsing at `kernel/src/platform.rs:83` and profile selection gives `board-pioneer` precedence over `board-vf2` at `kernel/src/platform.rs:175`.
- OBSERVED: Boot sets PLIC base from `PlatformInfo` before HAL init at `kernel/src/main.rs:107`; later `kernel/src/main.rs:541` calls `plic::init()`.
- OBSERVED: Snapshot restore reinitializes PLIC via `kernel/src/snapshot.rs:332`, so this caller must be included.

## Policy Leaks To Remove

- OBSERVED: `plic.rs` documents and assumes hart 0 S-mode context 1 at `hal/arch/riscv/src/common/plic.rs:12`.
- OBSERVED: `plic::init()` enables fixed VirtIO IRQs `1..=8` and UART IRQ `10` at `hal/arch/riscv/src/common/plic.rs:92`.
- OBSERVED: RV64 trap dispatch routes `1..=8` to VirtIO and `10` to UART at `hal/arch/riscv/src/rv64/trap.rs:105`.
- OBSERVED: RV64 trap claim and complete use context `1` at `hal/arch/riscv/src/rv64/trap.rs:181` and `hal/arch/riscv/src/rv64/trap.rs:193`.
- OBSERVED: RV64 unclaimed VirtIO ACK derives base by QEMU formula at `kernel/src/task/drivers/virtio_common.rs:131`.

## Existing Runtime Data

- OBSERVED: Board descriptors already store `uart`, `plic`, `clint`, `rtc`, `virtio_mmio`, and `enabled_drivers` at `boards/src/descriptor.rs:67`.
- OBSERVED: QEMU RV64 fallback UART IRQ is `10` at `boards/qemu/virt-riscv64/board.rs:42`.
- OBSERVED: QEMU RV64 fallback VirtIO IRQs are `1..5` with bases `0x1000_1000..0x1000_5000` at `boards/qemu/virt-riscv64/board.rs:62`.
- OBSERVED: Current repo contains only QEMU RV64 descriptor files under `boards/`; VF2 and Pioneer are feature/profile paths, not root board descriptors yet.
- PRIOR: Research says PLIC context policy belongs in `hal/soc/riscv`, while concrete device IRQs remain board/DTB `PlatformInfo` data.

## Caller And Contract Inventory

- OBSERVED: `vi_handle_virtio_irq` and `vi_handle_uart_irq` are internal `extern "Rust"` trap links at `hal/arch/riscv/src/rv64/trap.rs:207`.
- OBSERVED: `virtio_slots()` callers are driver init, input ACK probing, driver-cell registration, and VirtIO common paths at `kernel/src/task/drivers.rs:66`, `kernel/src/task/drivers/input_irq_ack.rs:30`, `kernel/src/task/drivers/driver_cell.rs:30`, and `kernel/src/task/drivers/virtio_common.rs:25`.
- OBSERVED: `virtio_slots()` allocates a `Vec` on RV64 at `kernel/src/task/drivers/virtio_common.rs:46`, so interrupt-time runtime lookup should not call it.
- OBSERVED: `libs/api/` and `libs/types/` are sacred interfaces requiring two confirmations at `docs/code-standards.md:14`.
- OBSERVED: Architecture docs keep root `boards/`, data-only `hal/soc/riscv`, and shared drivers in `cells/drivers/` at `docs/system-architecture.md:52` and `docs/system-architecture.md:58`.

## Verification Notes

- The repo does not contain `docs/coding.md` or `docs/engineering-standards.md`; closest current standards source is `docs/code-standards.md`.
- The plan-sync script `.claude/scripts/set-active-plan.cjs` is absent, so active plan state could not be synced by the requested command.
- Worktree status before planning was clean on branch `fix/structure`; latest commits are `c6a31372 docs(hardware): document RISC-V SoC profiles` and `9372d870 refactor(hal): add RISC-V SoC profiles`.
