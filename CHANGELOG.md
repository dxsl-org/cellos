# Changelog

All notable changes to the **ViOS (Jarvis Hybrid OS)** project will be documented in this file.

## [Unreleased] - ViOS Mycelium (Alpha Test) - Update 2026.01

### Added
- **Versioning System**: Introduced the "Mycelium" Era name and hierarchical versioning (CalVer for system updates, SemVer for core components).
- **Core Banner**: Added a standardized boot banner to the kernel displaying the Era, State, and Update timestamp.
- **IPC Zero-Copy**: Initial implementation of `Grant` and `Map` syscalls for high-performance inter-process communication.
- **VirtIO GPU Driver**: Basic support for graphics output in QEMU environments.
- **RISC-V HAL**: Hardware Abstraction Layer for 64-bit RISC-V architecture (QEMU Virt target).
- **Hybrid Architecture**: Defined the "Cells" (Native), "Silos" (Legacy), and "Playground" (WASM) architectural layers.

### Changed
- **Kernel Prelude**: Standardized imports across the kernel modules using a new prelude policy.
- **Task Scheduler**: Optimized context switching for RISC-V and improved task initialization logic.

### Fixed
- Resolved "Illegal Instruction" exceptions during context switching by correctly saving/restoring `gp` and `tp` registers.
- Fixed boot hang issues in QEMU by refining the UART initialization and serial output.

---
## [0.1.0] - 2025-12-30
### Added
- Initial project structure (Monorepo).
- Basic kernel skeleton and bootloader.
- Early architecture documentation (Phases 1-7).
