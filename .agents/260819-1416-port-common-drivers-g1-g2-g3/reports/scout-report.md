# Scout Report: Common Driver Port Plan

## Relevant Files

- Stage definitions: `docs/project-roadmap.md:169-203`.
- Driver priorities and reference strategy: `docs/specs/04-hardware.md:48-146`, `docs/specs/13-peripherals.md:4-5`, `docs/specs/13-peripherals.md:16-25`, `docs/specs/13-peripherals.md:160-177`.
- Current immediate TODOs: `docs/TODO.md:8`, `docs/TODO.md:28`.
- Ownership standard: `docs/code-standards.md:58-74`.
- Board descriptor contract: `boards/src/descriptor.rs:20-45`, `boards/src/descriptor.rs:94-127`.
- Board selection: `kernel/src/board.rs:31-56`, `kernel/src/board.rs:67-90`, `kernel/src/board.rs:93-115`.
- Platform data: `kernel/src/platform.rs:28-41`, `kernel/src/platform.rs:135-182`.
- MMIO/DMA substrate: `libs/ostd/src/mmio.rs:14-38`, `libs/ostd/src/mmio.rs:40-132`, `libs/ostd/src/dma.rs:1-66`.
- Driver registration: `kernel/src/task/drivers/driver_cell.rs:1-24`, `kernel/src/task/drivers/driver_cell.rs:48-70`.
- G2 substrate: `kernel/src/task/drivers/pcie_ecam.rs:1-15`, `kernel/src/task/drivers/iommu.rs:1-8`, `kernel/src/task/drivers/iommu_x86.rs:59-60`.
- Pioneer blockers: `boards/milk-v/pioneer/board.rs:7-11`, `boards/milk-v/pioneer/board.rs:33-53`, `hal/soc/riscv/src/catalog.rs:104-124`.
- Existing driver cells: `cells/drivers/*`, including `nvme`, `e1000`, `virtio-blk`, `virtio-net`, `gpio*`, and fallback/prototype `i2c-gpio`, `spi-gpio`, `pwm-gpio`, `adc-sim`, `can-loopback`.

## Patterns

- Boards select shared mechanisms via typed `DriverId`; current `DriverId` lacks I2C/SPI/PWM/ADC/CAN/NPU entries, so real controllers need descriptor work before promotion.
- Boards must not fork UART/SDHCI/DesignWare I2C/SPI/GIC/PLIC/VirtIO/PCIe mechanisms.
- Kernel uses board/SoC policy before early platform and driver initialization.
- Driver Cells claim MMIO through Resource Registry and DMA through grant/IOMMU paths.
- Spec 13 marks real hardware I2C/SPI/PWM/ADC/CAN controllers as deferred; bit-bang/sim/loopback crates are test/fallback baselines, not promotion targets.
- `cells/drivers/nvme` and `cells/drivers/e1000` already model PCIe Driver Cells but must be treated as prototype/bring-up until hardware and security gates pass.

## Precedents

- `ecb26b6d refactor(hal): restore board and SoC separation` and `0b1ff8a3 revert(hal): roll back premature HAL split merge` show the cost of broad HAL changes without tight gates.
- `309d401b refactor(hal): add x86 board and SoC separation`, `a74569ef refactor(hal): drive RISC-V selection from board data`, and `14141053 refactor(boards): enforce SoC-owned hardware facts` are the file-footprint checklist for new board/driver selection.
- `5513e5cd refactor(kernel): reuse RPi3 UART debug driver` is precedent for removing duplicated hardware access rather than cloning per-board code.

## Prior Failures

- No `.agents/failure-history.jsonl` found.
- No `.agents/incidents/` found.
- `docs/coding.md` and `docs/engineering-standards.md` were requested by role but are absent.
- `.claude/scripts/set-active-plan.cjs` is absent, so active-plan sync could not run.

## Blast Radius

- High: `boards/src/descriptor.rs`, `kernel/src/board.rs`, `kernel/src/platform.rs`, `kernel/src/task/drivers/{pcie_ecam,iommu,driver_cell,mmc,uart,gpio_irq,irq_dispatch}.rs`.
- Medium: `libs/api/src/abi/syscall.rs`, `libs/ostd/src/{mmio,dma,syscall}.rs`, `cells/drivers/*`, `scripts/check-board-configs.sh`, QEMU scripts.
- Low: reference reports and docs updates after code lands.

## Inconsistencies to Resolve

- Spec says G2 PCIe/NVMe/e1000 are planned, while current tree already contains partial implementations. The plan treats them as code present but not promotion-complete.
- q35 x86_64 is the first G2 lane; Pioneer is blocked because SG2042 currently selects SBI DBCN/PLIC/CLINT only and has no storage/network substrate.
- x86 VT-d currently has a hardcoded q35 MMIO base; Phase 05 must retire or gate that before claiming server hardware readiness.
- G3 docs intentionally forbid detailed trait freeze before hardware; plan only includes a readiness envelope.
