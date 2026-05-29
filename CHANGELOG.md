# ViOS Changelog

All notable changes to ViOS are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [Unreleased] - v0.2.1-dev

### Added

**Shell (Phase 17)**: Parser (pipes, redirects, background, sequences); job table; 1000-entry history; alias support; built-ins: wc, head, tail, grep, sort, sed, mkdir, rmdir, rm, pwd, uname, free, env, uptime; hot-swap state transfer.

**Network (Phase 15)**: smoltcp 0.11 Cell; DHCP; VirtIO NIC driver; socket IPC API.

**Compositor/GPU (Phase 16)**: Software compositor (z-order, damage, 30 FPS); GpuFlush syscall (300); Surface IPC.

**Input (Phase 14)**: US QWERTY keymap; modifier tracking; focus dispatch.

**Hot Migration (Phase 20)**: ViStateTransfer on Config/Shell/VFS; HotSwap syscall (400); lease auto-revoke; grant chains; scatter/gather IPC (202/203); RecvTimeout (201).

**Scripting (Phase 18)**: Lua 5.4 multi-line REPL + VFS io.open/read/close + shared ostd::repl.

**Benchmarking (Phase 22)**: /bin/bench (4 scenarios); weekly perf CI; regression detection.

**Utilities**: sys-tools (ps,env,uname,date,free,kill,shutdown,hotswap); net-tools (stubs); sort,sed.

**Docs/Infra**: dev-setup.sh+ps1; format-disk.ps1; ROADMAP.md; FAQ.md; hotswap/scripting/vfs/input/display/network API guides; Discussion templates.

### Changed
- Shell help updated with all commands and pipeline/redirect syntax
- VFS IPC: OP_MKDIR(5), OP_RMDIR(6), OP_UNLINK(7)
- ViFileSystem: readdir method added
- CapEntry: lease expiry + grant depth fields

### Fixed
- Lua 9 compiler warnings resolved
- Shell executor now forwards actual parsed arguments to built-in commands

---

## [0.2.0] - 2026-05-01 "Mycelium Alpha"

### Added
- RV64 HAL: SV39, PLIC, SBI, UART; ELF loader with PIE relocation
- Basic shell; VirtIO block (hang fixed); VirtIO keyboard (deadlock fixed)
- AArch64, x86_64, RV32, AArch32 HALs; FileHandle IPC; External ELF loading
- STRIDE threat model; CI/CD with QEMU boot test

[Unreleased]: https://github.com/vi-group/ViCell/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/vi-group/ViCell/releases/tag/v0.2.0
