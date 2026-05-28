# ViOS Project Roadmap

**Project**: ViOS (Jarvis Hybrid OS)  
**Current Version**: 0.2.0 (Mycelium Era)  
**Current Phase**: Phase 1 - Core Stability  
**Last Updated**: 2026-05-29

---

## Overview

ViOS development is organized into 4 major phases, each with specific milestones and acceptance criteria. This document tracks progress, blockers, and next steps.

---

## Phase 1: Core Stability (Current — Target: 2026-06-30)

**Goal**: Fix critical issues (VirtIO hang, keyboard input), stabilize nano-kernel, achieve multi-architecture HAL.

**Start Date**: 2026-04-01  
**Target End Date**: 2026-06-30  
**Effort**: 320 hours (~8 weeks @ 40h/wk)
**Status**: 🚧 60% COMPLETE (Phases 01, 02, 05 complete; Phase 04 partial)

### Milestone 1.1: VirtIO Block Device Fix
**Status**: ✅ PARTIAL (Root Cause Fixed)  
**Owner**: TBD  
**Priority**: P0 (blocking)

**Current State**:
- Root cause identified: Limine does not report MMIO ranges to kernel
- Solution: Explicit identity-mapping of VirtIO MMIO regions (0x1000_0000–0x1001_0000) in `kernel/src/memory/paging.rs`
- Duplicate MMIO entries removed from `kernel/src/boot.rs` FALLBACK_MEMORY_MAP
- Device interrupts now properly delivered via PLIC

**Deliverables**:
- [x] Debug root cause (MMIO identity-mapping missing)
- [x] Implement MMIO explicit mapping for VirtIO regions
- [x] Remove duplicate MMIO entries from fallback map
- [ ] Verify read/write complete within 100ms (testing in progress)
- [ ] Shell loads `/bin/shell` from disk (blocked by Phase 06)

**Completion**: Awaits full integration testing with Phase 06 (external ELF loading)

**Next Action**: Proceed with Phase 06 (External ELF Loading)

---

### Milestone 1.2: Keyboard Input Fix
**Status**: ✅ COMPLETE  
**Owner**: TBD  
**Priority**: P0 (blocking)

**Current State**:
- Root cause identified: VirtIO input IRQ was never acknowledged, leaving `InterruptStatus` register set; PLIC continuously re-fired interrupt, causing kernel hang
- Fix applied: Added `pub static INPUT_DEVICE_IRQ` constant and `pub fn ack_irq(irq: u32) -> bool` to `kernel/src/task/drivers/virtio_input.rs`
- Expanded `vi_handle_virtio_irq()` in `kernel/src/task/drivers/virtio_blk.rs` to dispatch to both block and input devices
- Established IRQ numbering pattern: QEMU VirtIO MMIO slot `i` → IRQ `i+1` (applies to all device types)
- Interrupt storm prevented by proper IRQ acknowledgment

**Deliverables**:
- [x] Multiple keystrokes processed without hang
- [x] IRQ acknowledgment properly implemented for all VirtIO devices
- [x] PLIC dispatch pattern established for block and input devices
- [x] Shell input loop no longer deadlocks on subsequent input
- [x] Async waker path analysis complete (not needed for polling-based shell)

**Completion**: Verified 2026-05-29; ready for Phase 2 shell interaction testing

**Next Action**: Proceed with Phase 03 (Ring 3 Boot) and Phase 06 (External ELF Loading)

---

### Milestone 1.3: Multi-Architecture HAL
**Status**: 🚧 NOT STARTED  
**Owner**: TBD  
**Priority**: P1 (high)

**Current State**:
- RISC-V 64-bit: FULLY IMPLEMENTED (SV39 paging, PLIC, SBI, traps)
- ARM AArch64: STUB (52 LOC)
- x86_64: STUB (46 LOC)

**Deliverables**:
- [ ] ARM AArch64 HAL (MMU, exception handling, timer)
- [ ] x86_64 HAL (paging, exception handling)
- [ ] Single kernel binary via feature flags
- [ ] Boot + basic scheduler on ARM (QEMU)
- [ ] Boot + basic scheduler on x86_64 (QEMU)

**Architecture Decisions**:
- Keep trait-based design (no duplication)
- Implement `hal::Arch`, `hal::PageTableTrait`, `hal::InterruptController` per arch
- Use conditional compilation: `#[cfg(target_arch = "arm")]`

**Next Action**: Create ARM AArch64 bootstrap code (vectors, MMU setup)

---

### Milestone 1.4: External ELF Loading
**Status**: 🚧 NOT STARTED  
**Owner**: TBD  
**Priority**: P1 (high)

**Current State**:
- Init Cell embedded in kernel (static)
- Other Cells hardcoded in RAM disk
- No hot-swap capability

**Deliverables**:
- [ ] Load Cell binaries from `/bin/` directory
- [ ] Syscall::spawn() reads ELF from disk
- [ ] ELF relocation for position-independent code
- [ ] Hot update: Replace shell at runtime

**Design Decisions**:
- Reuse existing ELF loader (kernel/src/loader.rs)
- Support PIE (Position Independent Executable)
- Cache loaded binaries in memory

**Dependency**: Milestone 1.1 (VirtIO) must be working

**Next Action**: Design syscall::spawn() protocol for `/bin/` loading

---

### Milestone 1.5: Test Coverage
**Status**: 🚧 NOT STARTED  
**Owner**: TBD  
**Priority**: P2 (medium)

**Current State**:
- Architecture validation: 10/10 score
- Unit tests: sparse (40% coverage estimate)
- Integration tests: minimal

**Deliverables**:
- [ ] Frame allocator tests (95%+ coverage)
  - Allocation patterns: sequential, random, fragmentation
  - Stress test: 10,000 alloc/free cycles
- [ ] Scheduler tests (90%+ coverage)
  - Round-robin fairness
  - Preemption under load
  - Task state transitions
- [ ] IPC tests (85%+ coverage)
  - Send/Recv, Call/Reply, timeout behavior
  - Capability grant/revoke
  - Multi-Cell cascading messages
- [ ] Multi-Cell integration (70% coverage)
  - Init → VFS → Shell scenario
  - Config service KV operations

**Run**: `cargo test --all --release`

**Next Action**: Write allocator unit tests first (foundation)

---

### Phase 1 Acceptance Criteria

All milestones complete when:
- ✅ VirtIO block device working (read/write, no hang)
- ✅ Keyboard input responsive (multiple keys, no deadlock)
- ✅ ARM + x86 HAL boot and run shell
- ✅ External ELF loading from `/bin/` functional
- ✅ Unit + integration tests pass (80%+ coverage)
- ✅ Architecture validation score: 10/10
- ✅ Kernel LOC: < 6000

---

## Phase 2: System Services (2026-07 — 2026-08-30)

**Goal**: Complete VFS, input, network, and graphics services.

**Effort**: 530 hours (~13 weeks)  
**Status**: 📋 PLANNED

### Milestone 2.1: Complete VFS Service
**Status**: 📋 PLANNED  
**Priority**: P0

- Write support for FAT32
- Directory creation/deletion/listing
- File permissions (read/write/execute)
- Async file operations (non-blocking)
- Disk quota tracking

**Dependency**: Phase 1 (VirtIO)

---

### Milestone 2.2: Complete Input Service
**Status**: 📋 PLANNED  
**Priority**: P1

- AT keyboard driver (scancode → ASCII)
- PS/2 mouse driver
- Input event queue (with timestamp)
- Compositor integration

**Dependency**: Phase 1 (basic shell)

---

### Milestone 2.3: Complete Network Service
**Status**: 📋 PLANNED  
**Priority**: P1

- TCP/IPv4 stack (basic)
- DHCP client
- Socket syscalls (bind, listen, connect, send, recv)
- VirtIO NIC driver

**Effort**: 200 hours

---

### Milestone 2.4: Complete Compositor & Display
**Status**: 📋 PLANNED  
**Priority**: P2

- VirtIO GPU driver
- Compositor Cell (window management)
- Wayland-like protocol
- 2D graphics rendering

**Effort**: 150 hours

---

## Phase 3: Applications & Runtimes (2026-09 — 2026-11-30)

**Goal**: Feature-rich shell, standard utilities, runtime integration.

**Effort**: 500 hours (~12 weeks)  
**Status**: 📋 PLANNED

### Milestone 3.1: Enhanced Shell
**Status**: 📋 PLANNED  
**Priority**: P1

- Piping: `cat file | grep pattern`
- Redirection: `cmd > file`, `cmd < input`
- Background execution: `cmd &`
- Job control: `fg`, `bg`, `jobs`
- Shell scripts (`.sh` files)
- Tab completion

---

### Milestone 3.2: Standard Utilities
**Status**: 📋 PLANNED  
**Priority**: P1

**File Tools**: cp, mv, rm, mkdir, rmdir, find  
**Text Tools**: grep, sed, awk, sort, uniq, wc  
**System Tools**: top, ps, kill, shutdown, reboot  
**Network Tools**: ping, curl, nc, ifconfig  

**Effort**: 200 hours

---

### Milestone 3.3: Lua Runtime Enhancement
**Status**: 📋 PLANNED  
**Priority**: P2

- Execute `.lua` scripts from shell
- Stdlib access (table, string, math, io, os)
- File I/O via VFS syscalls
- C FFI for kernel calls
- Package manager (luarocks) compatibility

---

### Milestone 3.4: MicroPython Runtime Enhancement
**Status**: 📋 PLANNED  
**Priority**: P2

- Execute `.py` scripts
- Stdlib (builtins, sys, os, math, random, json)
- File I/O, REPL mode
- Pip-like package installation

---

## Phase 4: Advanced Features & Optimization (2026-12 — 2027-03-31)

**Goal**: Hot migration, complete multi-arch support, performance optimization, v1.0 readiness.

**Effort**: 460 hours (~11 weeks)  
**Status**: 📋 PLANNED

### Milestone 4.1: Hot Migration (State Transfer)
**Status**: 📋 PLANNED  
**Priority**: P2

- Serialize Cell state (memory, registers, file handles)
- Load new binary, restore state
- Resume execution seamlessly
- Zero-downtime shell update

**Effort**: 120 hours

---

### Milestone 4.2: Advanced IPC
**Status**: 📋 PLANNED  
**Priority**: P2

- Lease: Capability grant with auto-revoke
- Grant chains: transitive capability delegation
- Bulk message passing (gather/scatter)
- Timeout support on Recv/Call

**Effort**: 60 hours

---

### Milestone 4.3: Complete RV32 & ARM Support
**Status**: 📋 PLANNED  
**Priority**: P2

- RISC-V 32-bit (RV32) full HAL
- ARM AArch32 full HAL
- Boot tests on all targets
- Single binary: `cargo build --features rv32 --release`

**Effort**: 200 hours

---

### Milestone 4.4: Benchmarking & Optimization
**Status**: 📋 PLANNED  
**Priority**: P3

**Targets**:
- Context-switch latency: < 100 µs
- Message latency (Send/Recv): < 50 µs
- Syscall overhead: < 10 µs
- Memory footprint: < 10 MB (kernel + 3 services)

**Deliverables**:
- Benchmark suite (public `ViBenchmark` trait)
- Profiling tools
- Performance regression tests

**Effort**: 80 hours

---

## High-Level Timeline

```
2026
├─ Q2 (Apr-Jun): Phase 1 - Core Stability
│  ├─ W1:    Phase 01 Workspace Cleanup ✅ (2026-05-28)
│  ├─ W1-2:  Phase 02 CI/CD Pipeline ✅ (2026-05-28)
│  ├─ W2-3:  Phase 04 VirtIO Block Fix (PARTIAL) ⚡ (2026-05-28)
│  ├─ W3:    Phase 05 Keyboard Input Fix ✅ (2026-05-29)
│  ├─ W4-5:  Phase 03 Ring 3 Boot + Phase 06 External ELF (PENDING)
│  ├─ W6-7:  Multi-arch HAL (ARM, x86) — Phases 08, 09
│  └─ W8:    Unit + integration tests — Phase 11
│  └─ TARGET: Phase 1 Complete (2026-06-30) [65% likely]
│
├─ Q3 (Jul-Sep): Phase 2 - System Services + Phase 3.1-3.2
│  ├─ VFS, input, network, compositor services
│  └─ Shell enhancements + standard utilities
│  └─ TARGET: Services Stable (2026-08-30)
│  └─ TARGET: User-Ready OS (2026-11-30)
│
└─ Q4 (Oct-Dec): Phase 3.3-3.4 + Phase 4.1-4.2
   ├─ Lua/MicroPython integration
   ├─ Hot migration + advanced IPC
   └─ Performance optimization
   └─ TARGET: v1.0 Production Ready (2027-03-31)
```

---

## Dependency Graph

```
Phase 1 (Core Stability)
├─ 1.1: VirtIO Fix
│  └─ blocks: 1.4 (External ELF loading)
│  └─ blocks: 2.1 (Complete VFS)
│
├─ 1.2: Keyboard Input Fix
│  └─ blocks: 2.2 (Complete Input Service)
│
├─ 1.3: Multi-Arch HAL
│  └─ unblocks: Phase 2+ on ARM/x86
│
└─ 1.5: Test Coverage
   └─ enables: Phase 2 (regression detection)

Phase 2 (System Services)
├─ 2.1: Complete VFS
│  └─ blocks: 3.1 (Enhanced Shell, scripting)
│
├─ 2.2: Complete Input
│  └─ blocks: 2.4 (Compositor)
│
└─ 2.4: Compositor
   └─ enables: GUI applications

Phase 3 (Applications)
├─ 3.1 + 3.2: Shell + Utilities
│  └─ blocks: 3.3, 3.4 (runtime integration)
│
└─ 3.3, 3.4: Runtimes
   └─ unblocks: Phase 4 (advanced features)

Phase 4 (Advanced Features)
└─ All phases complete
   └─ v1.0 Production Ready
```

---

## Known Blockers & Issues

### High Priority

| Issue | Impact | Mitigation | Owner |
|-------|--------|-----------|-------|
| VirtIO hang | Can't load from disk | QEMU tracing, swap to different device | TBD |
| Keyboard deadlock | Can't type multiple chars | Async/await audit, add logging | TBD |

### Medium Priority

| Issue | Impact | Mitigation |
|-------|--------|-----------|
| ARM/x86 HAL missing | Multi-arch not viable | Incremental implementation, RV64 primary |
| Async safety unclear | Potential lifetime bugs | Code review, property-based tests |

---

## Completed Work (Before Phase 1)

✅ **Phase 0 (Alpha)**
- Kernel skeleton (~5300 LOC)
- RISC-V 64-bit HAL with SV39 paging
- Round-robin scheduler with 10 core syscalls
- ELF loader + relocation
- FAT32 filesystem (read-only)
- VFS service (RamFS)
- Config service (KV store)
- Basic shell (echo, cat, ls, pwd, cd)
- Lua 5.4 + MicroPython 1.24.1 bindings
- Architecture validation (10/10 score)

---

## Next Steps (Immediate)

### This Week (2026-05-28 — 2026-06-03)

1. **Create GitHub Project Board**
   - Organize Phase 1 tasks
   - Set sprint deadlines

2. **Debug VirtIO Hang**
   - Enable QEMU `-trace` mode
   - Analyze device initialization sequence
   - Check interrupt handling

3. **Keyboard Input Analysis**
   - Add `eprintln!` logs to shell input loop
   - Trace async task state
   - Reproduce hang scenario

### Next 2 Weeks (2026-06-04 — 2026-06-17)

- Implement fixes based on debugging
- Start ARM AArch64 HAL stub → implementation
- Write allocator unit tests
- Document findings in ARCHITECTURE.md

### End of Month (2026-06-18 — 2026-06-30)

- All Phase 1 milestones complete
- Prepare Phase 2 kickoff
- Tag v0.2.1 release

---

## Success Metrics

### Phase 1 (Target: 2026-06-30)

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| VirtIO working | ✅ Yes | ⚡ Root cause fixed, testing | PARTIAL |
| Keyboard input | ✅ Multi-key | ✅ Reliable, no deadlock | ✅ COMPLETE |
| IRQ dispatch | ✅ All devices ack'd | ✅ Block + input | ✅ COMPLETE |
| CI/CD pipeline | ✅ 4-job matrix | ✅ Implemented | ✅ COMPLETE |
| Workspace warnings | ✅ 0 | ✅ 0 | ✅ COMPLETE |
| Multi-arch HAL | ✅ RV64+ARM+x86 | RV64 only | IN PROGRESS (Phases 08/09) |
| External ELF | ✅ Working | Embedded | PENDING (Phase 06) |
| Test coverage | ✅ 80%+ | 40% | PENDING (Phase 11) |
| Architecture tests | ✅ 10/10 | 10/10 | ✅ MET |
| Kernel LOC | ✅ < 6000 | ~5300 | ✅ MET |

---

## Release Planning

### v0.2.0 (Current — Mycelium Era)
- Stable basic kernel
- Working RV64 HAL
- Basic shell REPL
- Architecture validated

### v0.2.1 (Target: 2026-06-30)
- VirtIO block device fixed
- Keyboard input fixed
- Multi-arch HAL (RV64, ARM, x86)
- External ELF loading
- Unit test suite

### v0.3.0 (Target: 2026-09-30)
- Complete system services (VFS, input, network, compositor)
- Enhanced shell (piping, redirection, scripts)
- Standard utilities (grep, sed, awk, etc.)

### v1.0.0 (Target: 2027-03-31)
- Hot migration support
- Full multi-arch (RV32, RV64, ARM32, ARM64, x86_64)
- Production-grade performance
- Complete documentation
- Permissive license (MIT or Apache 2.0)

---

## Review & Update Cadence

- **Weekly**: Milestone status updates (every Monday)
- **Bi-weekly**: Blocker review + sprint planning
- **Monthly**: Phase progress review + roadmap adjustments
- **Quarterly**: Strategic review, Phase kickoff

**Last Review**: 2026-05-28 (PDR + roadmap creation)  
**Next Review**: 2026-06-04 (Phase 1 progress check)

---

## See Also

- **project-overview-pdr.md** — Detailed PDR + requirements
- **codebase-summary.md** — Current code structure
- **code-standards.md** — Development rules
- **system-architecture.md** — Architecture overview
- **99-roadmap.md** — Original roadmap (archive)
