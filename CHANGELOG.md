# Cellos Changelog

All notable changes to Cellos are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [Unreleased]

### 🚀 Improvements
- platform: enumerate checked inclusive MCFG ECAM ranges across buses with canonical BDFs and per-bus VT-d contexts
- x86_64: install 256 vector/CPL-aware IDT entries and verify them with an isolated `x86-idt-cpl3-test` two-task Ring-3 QEMU oracle
- security: land non-admissible Tier 1 admission catalog/test infrastructure; Phase 04 production evidence remains blocked
- kernel: enforce post-relocation W^X page permissions
- signing: gate cells through F1/F5 admission
- ipc: add bounded per-cell completion queues
- vfs: add inherited directory capabilities
- security: attest caller identity across VFS IPC
- hypha: native AI agent Cell, LLM gateway and chat loop
- net: TLS server certificate verification via embedded-tls
- ostd: http-core crate with HTTP/1.1 and JSON client
- disk: package app-https-demo as /bin/https-demo binary
- security: signed operator policy with Ed25519 in-kernel verify
- security: spawn-time capability intersection and delegation
- shell: Phase 17 parser, pipes, redirects, history, aliases
- net: smoltcp 0.11 Cell with DHCP and VirtIO NIC driver
- compositor: software blending, z-order, damage, 30 FPS
- input: US QWERTY keymap, modifier tracking, focus dispatch
- scripting: Lua 5.4 multi-line REPL with VFS bindings
- scripting: MicroPython v1.24.1 for RISC-V bare-metal
- hot-migration: ViStateTransfer, HotSwap syscall, grant chains
- bench: /bin/bench with 4 scenarios and perf CI integration
- community: CODE_OF_CONDUCT.md and contributor dev tooling
- doom: Freedoom Phase 1 port, boots and renders first frame
- vfs: add OP_MKDIR, OP_RMDIR, OP_UNLINK to IPC protocol
- kernel: add TryRecv, NetTx, NetRx, StateStash, StateRestore syscalls
- performance: release builds for all bootstrap table entries

### 🐛 Fixes
- e1000: fail closed on DMA authorization/OOM and program rings/descriptors with authorized IOVAs
- nvme: retain authorized IOVAs for queue bases, Identify PRPs, and sector I/O buffers
- drivers: publish initialized state before registration and fail closed when registration is rejected
- pcie: preserve exclusive Driver Cell BDF ownership and DMA pins across competing probes or unacknowledged IOMMU publication/teardown, with spec-correct RISC-V IODIR/IOTINVAL/IOFENCE batches that order prior device reads and writes before reclamation
- x86_64: preserve user state and balance CPL3 GS/PKRU across IDT, suspended SYSCALL, fresh IRET, and scheduler switches
- x86_64: correct the bootstrap SysV stack phase with an 8-byte synthetic bottom-frame slot before tail-entering Rust
- tools: validate MemInfo frame conservation and nonzero page size before independently rounding displayed KiB values
- kernel: clean completion waiter lifecycle safely
- boot: reject snapshots from mismatched RAM bases
- build: align cross-target CI and image generation
- hypha: plaintext transport workaround for net cell TLS crash
- hypha: NetClient.tcp_send now handles Data reply correctly
- hypha: foreground shell spawn now waits on child via sys_wait
- net: boot loop caused by wrong WaitForEvent tick unit
- lua: pcall binding, picolibc link, heap sbrk stub
- doom: posix fseek/ftell, vsnprintf precision, fatfs short-read
- embedded-fs: emit FAT16 to fix CorruptedFileSystem on mount
- rv32: gate rv64 module behind target_arch check
- x86_64: AT&T syntax for global_asm, HHDM PDPT NX bit
- git: untrack .logs/hook-log.jsonl causing perpetual dirty state

---

## [0.2.1] - 2026-06-08

### 🚀 Improvements
- viui: RenderCtx bundles canvas and FontContext for paint calls
- viui: FontContext with GlyphAtlas and 8x8 bitmap fallback
- viui: touch events added to Event enum
- viui: ProgressBar, Slider, TouchArea widgets
- viui: Animatable trait, Tween, easing module, AnimatedSignal
- viui: GpuCommandBuffer as struct field for allocation reuse
- readme: accurate build targets and correct GitHub URL

### 🐛 Fixes
- viui: Slider returns subscribe handle from collect_dirty_handles
- viui: ProgressBar label uses font-aware char_width

---

## [0.2.0] - 2026-05-01 "Mycelium Alpha"

### 🚀 Improvements
- rv64: SV39 paging, PLIC, SBI, UART, ELF loader with PIE
- kernel: basic shell, VirtIO block device, VirtIO keyboard
- hal: AArch64, x86_64, RV32, AArch32 HAL implementations
- security: STRIDE threat model and QEMU CI boot test

---

[Unreleased]: https://github.com/dxsl-org/Cellos/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/dxsl-org/Cellos/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/dxsl-org/Cellos/releases/tag/v0.2.0
