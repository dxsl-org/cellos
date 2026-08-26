# Current Cellos Driver Gap Research

Status: planner-synthesized. Collaboration subagents were requested by the active planning protocol, but this runtime did not expose `spawn_agent` to this worker. Facts below are directly verified unless marked PRIOR/ASSUMED.

## Findings

- G1/G2/G3 are use-case overlays, not technical phase renumbering: `docs/project-roadmap.md:169`, `docs/project-roadmap.md:175`, `docs/project-roadmap.md:177`, `docs/project-roadmap.md:182`, `docs/project-roadmap.md:192`.
- G1 means robot/embedded on ARM64/RV64 SBCs plus later RV32 MCU; G2 means x86_64 and multi-core RV64/ARM64 server/PC; G3 is NPU-native and hardware-gated: `docs/project-roadmap.md:177`, `docs/project-roadmap.md:182`, `docs/project-roadmap.md:192`.
- Hardware spec prioritizes G1 GPIO/UART/I2C/SPI/PWM/ADC/CAN and G2 PCIe ECAM -> IOMMU -> NVMe -> real NIC: `docs/specs/04-hardware.md:65`, `docs/specs/04-hardware.md:77`.
- Spec 13 marks current G1 peripheral work as PARTIAL: bit-bang I2C/SPI/PWM, ADC simulation, and CAN loopback are v1 verification baselines while real hardware controllers are deferred: `docs/specs/13-peripherals.md:4`, `docs/specs/13-peripherals.md:160`, `docs/specs/13-peripherals.md:176`.
- `DriverId` currently lacks I2C/SPI/PWM/ADC/CAN/NPU identifiers, so controller promotion requires Phase 02 selection-data work first: `boards/src/descriptor.rs:20`.
- G3 must not freeze `ViAccelerator` before real NPU experience: `docs/project-roadmap.md:195`, `docs/project-roadmap.md:203`, `docs/specs/04-hardware.md:126`.
- Current board descriptors already carry typed `DriverId` selection and validation: `boards/src/descriptor.rs:20`, `boards/src/descriptor.rs:94`, `boards/src/descriptor.rs:124`.
- RPi3 descriptor enables mini UART, BCM IRQ/timer, Arasan SDHCI, and BCM GPIO: `boards/raspberry-pi/3-model-b/board.rs:7`.
- q35 x86_64 descriptor enables COM1, IOAPIC, HPET, ECAM, NVMe, e1000: `boards/qemu/q35-x86_64/board.rs:7`.
- Kernel board selection validates descriptors before platform use: `kernel/src/board.rs:31`, `kernel/src/board.rs:70`, `kernel/src/board.rs:103`.
- Driver Cell safety substrate exists: `libs/ostd/src/mmio.rs:14`, `libs/ostd/src/mmio.rs:40`, `libs/ostd/src/dma.rs:1`, `libs/ostd/src/dma.rs:53`.
- Driver Cell registration exists for block/NIC/GPU/input roles: `kernel/src/task/drivers/driver_cell.rs:1`, `kernel/src/task/drivers/driver_cell.rs:14`, `kernel/src/task/drivers/driver_cell.rs:17`, `kernel/src/task/drivers/driver_cell.rs:23`.
- G2 PCIe/IOMMU/NVMe/e1000 primitives exist but need readiness hardening before promotion: `kernel/src/task/drivers/pcie_ecam.rs:1`, `kernel/src/task/drivers/iommu.rs:1`, `cells/drivers/nvme/src/main.rs:1`, `cells/drivers/e1000/src/main.rs:1`.
- Pioneer is not a storage/network G2 execution target yet: its descriptor enables only SBI DBCN, PLIC, and CLINT, and SG2042 disables VirtIO/MMIO storage helpers: `boards/milk-v/pioneer/board.rs:7`, `hal/soc/riscv/src/catalog.rs:104`.
- x86 VT-d currently carries a q35 hardcoded MMIO base risk: `kernel/src/task/drivers/iommu_x86.rs:59`.
- Test gates separate compile/QEMU from real hardware: `scripts/check-board-configs.sh:134`, `scripts/qemu-boot-test.sh:2`, `scripts/qemu-aarch64-test.sh:2`, `scripts/qemu-x86_64-test.sh:2`.

## Gaps

- `docs/coding.md` and `docs/engineering-standards.md` requested by role are absent in this checkout; used `docs/code-standards.md` and hc-plan references instead.
- Active dirty files include driver/IRQ/HAL work; implementation must rebase/inspect before touching: `docs/TODO.md`, `kernel/src/task/drivers/*`, `hal/traits/arch/*`.
- WSL `rg` is unusable in this session because it resolves to a WindowsApps binary; use `git grep`, `grep`, or PowerShell `Select-String`.
