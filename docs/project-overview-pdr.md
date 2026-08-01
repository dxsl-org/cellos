# Cellos Project Overview & PDR

**Project Name**: Cellos (Jarvis Hybrid OS)  
**Version**: 0.2.1-dev (Mycelium Era)  
**Status**: Active Development (Phase 1 - Core Stability)  
**Last Updated**: 2026-07-07 (docs audit: native scripting runtimes unmaintained, kernel LOC + phase-number corrections)

---

## Executive Summary

Cellos is a next-generation operating system designed for the **Edge-to-Cloud era**. It combines innovations from Theseus (Live Evolution), Asterinas (FrameKernel Safety), and Tock (Embedded Efficiency) into a unified architecture.

**Product delivery is framed in two use-case stages** (overlay on the technical phases below — see [project-roadmap.md](project-roadmap.md) → "Two Use-Case Stages"):
- **Stage G1 — Robot & Embedded** (now → ~2026 Q4): complete the OS for robots/embedded. Primary target = Tier A SBC with MMU (RV64/ARM64, RPi-class robot brain); sub-track = Tier B MCU (RV32 <512KB, CHERIoT-Nano) for low-level control. Defining traits: never-die, bounded real-time, fault isolation, peripheral I/O (GPIO/I2C/SPI/UART/CAN), instant-on boot.
- **Stage G2 — Server & Specialized PC** (~2027): expand to servers/PCs. Adds SMP multi-core, full desktop compositor, zero-downtime hot migration, x86_64 full bring-up, large storage. Untrusted code runs in the Tier 3 Linux VM.

**Key Innovation**: Cellular Single Address Space (SAS) using Language-Based Isolation (LBI) via Rust's type system. Software is organized as **Cells** (not processes) sharing one address space, isolated by Rust's compiler rather than hardware MMU.

**Current Focus**: Stabilize the nano-kernel, fix VirtIO hang issue, and achieve multi-architecture HAL with RV64/ARM/x86 support.

---

## Key Differentiator Opportunity

The architecture spec (03-runtime.md) describes **Heap Snapshotting (Instant On)**: after first boot, serialize the full memory state to `system.img`. Subsequent boots load the snapshot directly, bypassing ELF parsing and re-linking — sub-100 ms cold boot for a full OS stack.

No production OS offers this. If implemented, this becomes Cellos's primary competitive differentiator over Linux, Fuchsia, and unikernels. Delivered in Phase 29 (Heap Snapshotting / Instant On) — ✅ COMPLETE (2026-06-07).

---

## Vision & Philosophy

### Problem Statement

Traditional operating systems (Linux, Windows, macOS) inherit Unix's process model:
- **Process Isolation**: Hardware MMU enforces boundaries (expensive TLB flushes, context switches)
- **Capability Fragmentation**: Global permissions (uid/gid), not fine-grained capabilities
- **Kernel Complexity**: 20+ million LOC to handle process management
- **IPC Overhead**: Message passing across process boundaries requires syscalls + memory copies

**Cellos Goal**: Redesign the OS from first principles for 2026+

### Architecture Principles

1. **Cellular SAS**: One address space, multiple isolated execution contexts (Cells)
   - Cells are like "super-processes" with compiler-enforced isolation
   - Zero-copy IPC via owned buffers and capability objects
   - No process cleanup on exit (Cells clean up explicitly via Drop)

2. **Language-Based Isolation**: Rust's type system enforces safety
   - Cells cannot use `unsafe` code (`#![forbid(unsafe_code)]`)
   - Kernel/HAL use `unsafe` only for hardware I/O (documented with `// SAFETY:`)
   - No buffer overflows, no use-after-free in application code

3. **Nano-Kernel Philosophy**: Minimize trusted code
   - Kernel size is tracked by generated project status; in-kernel driver and
     orchestration residue remains scheduled for migration to Cells
   - Move filesystem, networking, drivers to userspace Cells
   - Each Cell is independently testable and upgradeable

4. **Capability-Based Access Control**: Fine-grained, no global permissions
   - Cells don't have uid/gid
   - IPC messages include capability grants
   - Revocation is automatic (Drop trait)

5. **Multi-Architecture from Day 1**: Single codebase, multiple targets
   - RV64, AArch64, and x86_64 have distinct build/smoke evidence
   - RV32/AArch32 and architecture-specific production qualification remain separate gates
   - A successful HAL smoke is not a blanket hardware/product qualification

---

## Project Structure

### Crates (~40 active)

```
Kernel & Core
├── kernel              Nano kernel (size reported by generated project status; boundary migrations remain tracked)

Hardware Abstraction
├── hal/core            Facade (feature-gated)
├── hal/traits/*        Pure trait definitions
├── hal/arch/riscv      RV64 FULL, RV32 STUB
├── hal/arch/arm        AArch64 FULL (Ring-3 smoke)
└── hal/arch/x86        x86_64 FULL (Ring-3 smoke)

Public API (Stable ABI)
├── libs/types          Core types (VAddr, PAddr, ViError)
├── libs/api            Kernel-Cell boundary traits (ViFileSystem, ViDriver, etc.)
└── libs/ostd           Cells' standard library (syscall wrappers, I/O, alloc)

Cells
├── cells/apps/         Applications (8 crates: init, shell, hello, utils, bench, sys-tools, net-tools, test-isolation)
├── cells/drivers/      Hardware drivers (6 crates)
├── cells/services/     System services (6 crates)
└── cells/runtimes/     VMs (2 crates: lua, micropython)
```

### Total Codebase
- **Rust Code**: moving file/LOC totals belong in generated project status, not this PDR
- **Design Docs**: normative specifications plus generated status; exact counts are generated
- **Build lanes**: RV64 is the primary reference/QEMU CI target; ARM64 is the first
  bare-metal safety-qualification candidate; x86_64 support and qualification are tracked separately

---

## Product Development Requirements (PDR)

### Phase 1: Core Stability (Current — 2026-06)

#### 1.1 VirtIO Block Device Fix

**Status**: ✅ COMPLETE (Root Cause Fixed, Testing In Progress)

**Requirement**: Proper VirtIO block device driver with read/write.

**Implemented**:
- [x] MMIO explicit identity-mapping (0x1000_0000–0x1001_0000)
- [x] IRQ dispatch pattern established
- [x] Device initialization without hang
- [ ] Full read/write integration (awaits Phase 06 external ELF loading)

**Current Status**: Block device reads/writes functional; shell integration awaits external ELF loader.

**Effort**: 40 hours  
**Owner**: Completed in Phase 05

#### 1.2 Keyboard Input Fix

**Status**: ✅ COMPLETE (Verified 2026-05-29)

**Requirement**: Multi-keystroke input without hang.

**Implemented**:
- [x] VirtIO input IRQ acknowledgment
- [x] Multiple consecutive keystrokes
- [x] Backspace, Enter, Ctrl+C handling
- [x] Command history (up/down arrows)
- [x] 100+ character input support

**Root Cause Fixed**: IRQ acknowledgment pattern (was: InterruptStatus left set → PLIC re-fires interrupt → storm)

**Effort**: 20 hours  
**Owner**: Completed in Phase 05

#### 1.3 Multi-Architecture HAL

**Status**: Implemented for RV64, AArch64, and x86_64 with target-specific smoke evidence;
production qualification remains per architecture and board.

**Requirement**: Stable trait-based HAL supporting RV64, ARM AArch64, x86_64.

**Implemented**:
- [x] ARM AArch64 (paging, exception handling, Ring-3 smoke)
- [x] x86_64 (paging, exception handling, Ring-3 smoke)
- [x] Feature-gated builds: `cargo build --features aarch64` / `--features x86_64`
- [x] Architecture validation tests (10/10 score) on RV64
- [x] No `unsafe` in Cells outside the reviewed allowlist (`scripts/unsafe-allowlist.toml`), enforced by `cellos-sign --check`

**Effort**: 120 hours  
**Owner**: Completed in Phase 05

#### 1.4 External ELF Loading

**Status**: ✅ COMPLETE (spawn_from_path verified)

**Requirement**: Load Cell binaries from `/bin/` filesystem.

**Implemented**:
- [x] `syscall::spawn_from_path("/bin/shell")` working
- [x] Config, VFS, Shell loaded from disk
- [x] Hot-swap: Replace shell at runtime
- [x] ELF relocation with PIE support

**Effort**: 60 hours  
**Owner**: Completed in Phase 10

#### 1.5 Test Coverage

**Requirement**: Unit tests for allocator, scheduler, IPC; integration tests for multi-Cell scenarios.

**Current Status**: 10/10 architecture validation score; limited unit tests.

**Acceptance Criteria**:
- [ ] Frame allocator: alloc/free/fragment tests (95%+ coverage)
- [ ] Scheduler: round-robin fairness, preemption, task switching (90%+ coverage)
- [ ] IPC: Send/Recv/Call/Reply, blocking, timeout (85%+ coverage)
- [ ] Multi-Cell: 3+ Cells communicating, cascade messages (70% coverage)
- [ ] All tests pass: `cargo test --all --release`

**Effort**: 80 hours  
**Owner**: TBD

**Success Metric**: Total Phase 1 effort = 320 hours (~8 weeks @ 40h/wk)

---

### Phase 2: System Services (2026-07 — 2026-08)

#### 2.1 Complete VFS Service

**Requirement**: Full filesystem abstraction (FAT32, ext4 support planned).

**Current Status**: MountTable VFS with BootFS, RamFS, FAT write support, default-enabled
littlefs at `/data`, and staged RedoxFS activation. QEMU evidence does not replace
real-board power-cut qualification.

**Acceptance Criteria**:
- [x] Write support for FAT32
- [ ] Directory creation/deletion
- [ ] File permissions (read/write/execute bits)
- [ ] Async file operations (non-blocking I/O)
- [ ] Disk quota tracking

**Effort**: 100 hours  
**Owner**: TBD

#### 2.2 Complete Input Service

**Requirement**: Unified keyboard + mouse input routing.

**Current Status**: ✅ COMPLETE (Milestone 2.2, 2026-06-12). PS/2 mouse deferred to G2 (VirtIO mouse/touchpad supported).

**Acceptance Criteria**:
- [x] Keyboard driver (VirtIO input scancode → ASCII)
- [ ] PS/2 mouse driver (deferred to G2 — VirtIO pointer works)
- [x] Input event queue with IPC forwarding (`dispatch_pending()` on IRQ)
- [x] App focus registration + focused-Cell routing (`request_input_focus()`, `collect_input_events()`); E2E CI test `input_keyboard_e2e`

**Effort**: 80 hours  
**Owner**: TBD

#### 2.3 Complete Network Service

**Requirement**: TCP/IP stack for Cells.

**Current Status**: ✅ COMPLETE (Phases A–B, E complete)

**Implemented**:
- [x] TCP/IPv4 stack (smoltcp 0.11, no IPv6 yet)
- [x] DHCP client for automatic IP assignment (verified: 10.0.2.15/24 on QEMU)
- [x] Socket API via syscalls (SOCKET_TCP, SOCKET_UDP, BIND, LISTEN, ACCEPT, CONNECT, SEND, RECV, SENDTO, RECVFROM, CLOSE)
- [x] TCP data-path (client + server with LISTEN/ACCEPT)
- [x] UDP data-path with SENDTO/RECVFROM
- [x] DNS resolver (static table + IPv4 literal + UDP A-record fallback)
- [x] QEMU VirtIO network device support
- [x] net-tools binaries: ping (stub), curl (HTTP/1.0), wget, nc (multi-conn relay), httpd, mqtt (skeleton)
- [x] Lua + MicroPython network bindings (vnet module)

**Effort**: 200 hours (actual: phases A–B–E ~120 hours)  
**Owner**: Completed Phases A–B–E (2026-06-03 to 2026-06-05)

#### 2.4 Compositor & Display

**Requirement**: Graphics framebuffer + window compositing.

**Current Status**: 🚧 PARTIAL (Milestone 2.4 still PLANNED overall). Zero-copy Grant surfaces + damage-driven render + FONT8X8 + `ViSurface` COMPLETE (2026-06-09); basic framebuffer + opt-in GPU (Phase 16). Full desktop windowing/Z-order deferred to G2.

**Acceptance Criteria**:
- [x] VirtIO GPU driver (linear framebuffer mode, opt-in)
- [~] Compositor Cell manages windows + Z-order (grant surfaces done; full window management G2)
- [x] Window rendering (software rasterizer via `ViCanvas`)
- [ ] Wayland-like protocol between Cells (G2)

**Effort**: 150 hours  
**Owner**: TBD

**Success Metric**: Total Phase 2 effort = 530 hours (~13 weeks)

---

### Phase 3: Applications & Runtimes (2026-09 — 2026-11)

#### 3.1 Enhanced Shell

**Requirement**: Feature-rich interactive shell.

**Current Status**: Basic REPL (echo, cat, ls, pwd, cd, help).

**Acceptance Criteria**:
- [ ] Piping: `cat file | ls`
- [ ] Redirection: `cmd > file`, `cmd < input`
- [ ] Background execution: `cmd &`
- [ ] Job control: `fg`, `bg`, `jobs`
- [ ] Scripting: `.sh` files with variables, loops, conditionals
- [ ] Tab completion for binaries + paths

**Effort**: 120 hours  
**Owner**: TBD

#### 3.2 Standard Utilities

**Requirement**: Core Unix-like tools.

**Current Status**: echo, cat, ls only.

**Acceptance Criteria**:
- [ ] File tools: `cp`, `mv`, `rm`, `mkdir`, `rmdir`
- [ ] Text tools: `grep`, `sed`, `awk`, `sort`, `uniq`
- [ ] System tools: `top`, `ps`, `kill`, `shutdown`
- [ ] Network tools: `ping`, `curl`, `nc`
- [ ] POSIX compliance where applicable

**Effort**: 200 hours  
**Owner**: TBD

#### 3.3 Lua Runtime Enhancement

**Requirement**: Full Lua 5.4 execution, stdlib access.

**Current Status**: Milestone 3.3 marked ✅ COMPLETE historically (2026-06-05: typed VFS IPC, io.open, vfs.stat/listdir/remove). **Native runtime is NOT actively maintained** — treated as a half-measure. Scripting/R&D story is now Python via the Tier 3 Linux VM. Package manager (luarocks) and further enhancement targets are cancelled.

#### 3.4 MicroPython Runtime Enhancement

**Requirement**: Python 3 subset execution environment.

**Current Status**: Milestone 3.4 marked ✅ COMPLETE historically (2026-06-05: `vfs_bridge.rs`, `modvfs.c`, typed VFS IPC). **Native runtime is NOT actively maintained** and the full enhancement targets below (pip, REPL, stdlib expansion) are **dropped**. Python for R&D runs as full CPython inside the **Tier 3 Linux VM** (`apt install python3 pip numpy torch`), not as a native Cell.

**Success Metric**: Total Phase 3 effort = 500 hours (~12 weeks)

---

### Phase 4: Hot Migration & Advanced Features (2026-12 — 2027-03)

#### 4.1 Hot Migration (State Transfer)

**Requirement**: Update Cell binaries without shutting down.

**Current Status**: Syscall structure exists (ViStateTransfer trait), not implemented.

**Acceptance Criteria**:
- [ ] Serialize Cell state (memory, registers, handles)
- [ ] Load new binary, restore state
- [ ] Resume execution with preserved file handles
- [ ] Zero-downtime shell update scenario

**Effort**: 120 hours  
**Owner**: TBD

#### 4.2 Advanced IPC

**Requirement**: Leasing, grant chains, bulk message passing.

**Current Status**: Basic Send/Recv/Call/Reply only.

**Acceptance Criteria**:
- [ ] Lease: Grant capability for duration, auto-revoke
- [ ] Grant chains: Cell A grants to B, B grants to C (transitive)
- [ ] Bulk messages: Multi-buffer sends, gather/scatter
- [ ] Timeout support on Recv/Call

**Effort**: 60 hours  
**Owner**: TBD

#### 4.3 RV32 & ARM Support

**Requirement**: Full multi-architecture deployment.

**Current Status**: Stubs for RV32 (4 LOC), ARM (53 LOC), x86 (46 LOC).

**Acceptance Criteria**:
- [ ] RISC-V 32-bit (RV32) HAL fully implemented
- [ ] ARM AArch32 HAL fully implemented
- [ ] Single binary selectable: `cargo build --features rv32 --release`
- [ ] Boot tests pass on all targets (QEMU simulation)

**Effort**: 200 hours  
**Owner**: TBD

#### 4.4 Benchmarking & Optimization

**Requirement**: Performance analysis, optimization.

**Current Status**: No benchmarks collected.

**Acceptance Criteria**:
- [ ] Context-switch latency < 100 µs
- [ ] Message latency (Send/Recv) < 50 µs
- [ ] Syscall overhead < 10 µs
- [ ] Memory footprint < 10 MB for kernel + 3 services
- [ ] Public `ViBenchmark` trait for app profiling

**Effort**: 80 hours  
**Owner**: TBD

**Success Metric**: Total Phase 4 effort = 460 hours (~11 weeks)

---

## Technical Constraints & Dependencies

### Hardware Requirements

- **Primary**: QEMU virt machine (RV64 target)
- **Minimum**: 128 MB RAM, 1 hart
- **Future**: Bare-metal boards (HiFive Unleashed, Raspberry Pi 5, x86 boards)

### Software Stack

| Layer | Technology | Version | Status |
|-------|-----------|---------|--------|
| Bootloader | Limine | Latest | ✅ Working |
| Kernel | Rust nightly | 2024+ | ✅ Compiling |
| HAL | Custom traits | N/A | RV64/AArch64/x86_64 implemented with different smoke/qualification levels |
| Filesystems | MountTable: BootFS/RamFS/FAT/littlefs/RedoxFS | Existing | FAT writes and littlefs `/data` shipped; RedoxFS and hardware qualification are phased |
| Runtimes | Lua / MicroPython | 5.4 / 1.24.1 | ⚠️ Native runtimes unmaintained (dropped); Python = Tier 3 VM |

### Key Dependencies

```toml
spin = "0.9"              # Spinlock (workspace dep)
virtio-drivers = "0.7.0"  # VirtIO block/GPU/input
xmas_elf = "0.9"          # ELF parsing
fatfs = "0.3"             # FAT32 filesystem
riscv = "0.16.0"          # RISC-V CSR access
```

### Breaking Changes

None documented yet (Phase 1 still stabilizing).

---

## Success Metrics (Phase 1)

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| Kernel boundary | Core excludes driver/service policy | See generated project status | 🚧 Driver/orchestration residue remains |
| Architecture Tests | 10/10 | 10/10 | ✅ Met |
| Build Time | < 60s | No retained benchmark artifact | 🚧 Measurement gate open |
| VirtIO Block | Working | ✅ Working | ✅ Complete |
| Keyboard Input | Multi-key | ✅ Multi-key | ✅ Complete |
| Multi-Arch HAL | RV64+ARM+x86 | Implemented; evidence differs by target | 🚧 Qualification is target-specific |
| Unit Test Coverage | 80%+ | Not currently measured by a committed artifact | 🚧 Measurement gate open |
| Documentation | Current and cross-checked | No synthetic completion percentage | 🚧 Drift reconciliation ongoing |

---

## Risk Assessment

### High-Risk Items

1. **VirtIO Device Hang** (Severity: High, Probability: Medium)
   - **Impact**: Shell cannot load binaries from disk
   - **Mitigation**: Fallback to RamDisk (current workaround); debug with QEMU trace

2. **Multi-Architecture Complexity** (Severity: High, Probability: High)
   - **Impact**: Paging, exception handling differ significantly
   - **Mitigation**: Comprehensive trait abstraction (HAL), early testing on QEMU

3. **Async Safety in SAS** (Severity: Medium, Probability: Low)
   - **Impact**: Lifetime violations if owned buffers not enforced
   - **Mitigation**: Compiler checks (forbid references), code review

### Medium-Risk Items

1. **Performance Regression** — SAS overhead vs. process isolation
2. **Scheduler Fairness** — Round-robin may not suit all workloads
3. **External ELF Loading** — Relocation complexity, security implications
4. **Spectre v1/v2 in SAS** — Compromised Tier 1 cell reads entire kernel + other cells
5. **Spec–Reality IPC Gap** — IPC is 100–1000× slower than architecture spec claims (syscall vs. direct call)
6. **No Per-Cell Memory Quota** — Single cell OOM kills entire system
7. **KASLR Absent** — Kernel address predictable from first bytecode execution

### Mitigation Strategies

- Weekly architecture review meetings
- Early benchmarking (Phase 24 immediate priority)
- Community feedback on design decisions
- Conservative feature additions (one major change per week)
- Direct IPC fast path (Phase 27) to close spec gap
- Priority scheduler (Phase 25) for real-time isolation
- Untrusted third-party code isolated via the Tier 3 Linux VM

---

## Development Timeline

> **Use-case stage overlay** (maps onto the technical phases below):
> - **G1 Robot & Embedded** (now → ~2026 Q4): Core Stability ✅ + Phases 24–26, 29–30 + Peripheral Driver track 🆕 + ARM64 full bring-up 🆕 + VFS robustness + RV32-Nano sub-track (tail) + reference robot demo 🆕.
> - **G2 Server & PC** (~2027): Phase 32 (SMP), Phase 27-3 (direct IPC), full compositor/desktop, hot migration (M4.1), x86_64 full bring-up 🆕, full utilities, throughput benchmarks.

```
Phase 1: Core Stability
├─ Week 1-2:  VirtIO debug + fix
├─ Week 3-4:  Keyboard input fix
├─ Week 5-7:  ARM/x86 HAL implementation
├─ Week 8:    External ELF loading + tests
└─ Milestone: Phase 1 Complete (2026-06-30)

Phase 2: System Services (2026-07 — 2026-08)
├─ VFS enhancements
├─ Input/network/compositor services
└─ Milestone: Services Stable (2026-08-30)

Phase 3: Applications & Runtimes (2026-09 — 2026-11)
├─ Shell enhancements
├─ Utility binaries
├─ Lua/MicroPython integration
└─ Milestone: User-Ready OS (2026-11-30)

Phase 4: Advanced Features (2026-12 — 2027-03)
├─ Hot migration
├─ Full RV32/ARM support
├─ Performance optimization
└─ Milestone: Production-Ready v1.0 (2027-03-31)
```

---

## Non-Functional Requirements

| Requirement | Target | Method |
|-------------|--------|--------|
| **Reliability** | 99.5% uptime | Watchdog timers, graceful shutdown |
| **Performance** | < 100 µs context switch | Benchmarking suite |
| **Security** | No buffer overflows in Cells | Rust compiler enforcement |
| **Maintainability** | Responsibility-bounded kernel with generated total/core nLOC trend | Spec 15 + [generated metrics](code-metrics.generated.md) |
| **Scalability** | Per-request profile goal: 1000 simultaneous isolated cells after staged 64/128/256/512 measurements | Shared immutable image frames, demand-paged stacks, profile quotas, dynamic tables |
| **Portability** | RV64, ARM, x86 | Feature-gated HAL |

---

## Stakeholders

- **Core Team**: DXSL (tinyong@vigroup.ai)
- **Advisors**: Theseus (UC Santa Cruz), Asterinas (TBD), Tock (Google)
- **Community**: Open source contributors (GitHub)

---

## Success Criteria (Overall)

1. ✅ Passes architecture validation (10/10)
2. 🚧 Kernel boundary target — generated size/status must show tracked driver and orchestration migrations complete
3. ✅ No `unsafe` in Cells outside the reviewed allowlist — enforced at the signing gate; driver/FFI cells hold documented exemptions
4. 🚧 Multi-architecture HAL (RV64, ARM, x86) — implemented, with qualification tracked per target
5. 🚧 Coverage target (80%+) — unverified until coverage output is generated and retained
6. 🚧 Production-ready documentation — drift reconciliation and link checks remain continuous gates
7. 🚧 Reproducible builds — bit-for-bit CI comparison harness not yet verified
8. ✅ Open source with permissive license

---

## See Also

- **codebase-summary.md** — File structure & metrics
- **code-standards.md** — Coding rules & conventions
- **system-architecture.md** — High-level design
- **project-roadmap.md** — Phase progress tracking
- **CLAUDE.md** — 8 Coding Laws (auto-loaded)
- **docs/0X-*.md** — Detailed specifications
