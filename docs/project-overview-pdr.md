# Cellos Project Overview & PDR

**Project Name**: Cellos
**Version**: 0.2.1-dev (Mycelium Era)  
**Status**: Research and development — Intel x86-64 Cell-to-Cell Anywhere only
**Last Updated**: 2026-10-08 (sole-program direction; retained technical sections keep their own evidence dates)

---

## Executive Summary

Cellos is a Rust-native research OS whose **sole active direction is Cell-to-Cell Anywhere on Intel x86-64**, on one fixed headless hardware configuration. [ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md) is the canonical decision and [current focus](roadmap/current-focus.md) owns the active work sequence.

The target is explicit C2C participation across local execution, LAN, and a gated relay: **Tier 1 native Cells, Tier 2 C/C++ paged domains, and Tier 3 VM guests through explicit adapters**. It is not transparent distribution of arbitrary applications, and this direction does not assert that all tiers or transports are implemented and qualified on Intel today.

Every task must identify a direct C2C-on-Intel deliverable, dependency, or necessary regression. GUI, browser, AI, robotics, general-purpose OS expansion, and new AMD/ARM/RISC-V platform work are paused as independent programs. Prior G1–G5 product stages and the dated phase plans below are retained for history, not independent implementation schedules. Existing code, exact-target evidence, and necessary cross-architecture regression coverage remain; ARM protected-authority evidence may inform an Intel dependency without reopening an ARM platform program.

**Architecture boundary**: Trusted native Cells can share a Cellular Single Address Space (SAS), using Rust language-based isolation within the reviewed compiler/kernel/unsafe-code trust base. LBI is not a hardware isolation boundary for arbitrary untrusted code. Tier 2 and Tier 3 have distinct isolation and admission requirements.

**Evidence ceiling**: x86 QEMU boot/CPL3 evidence does not qualify a physical Intel machine. Intel VMX is incomplete, AMD SVM QEMU evidence does not qualify Intel, x86 Tier 2 admission is test-only, and the C++ shim gap remains open. No physical x86 HCL entry is qualified.

The decision does not authorize hardware purchases, ABI changes, security relaxation, automatic remote execution, or production activation. Root `boards/` continues to own board descriptors, `hal/soc/*` immutable SoC facts, and `hal/traits/arch/src/kernel_abi.rs` shared HAL-to-kernel Rust ABI hook signatures.

---

## Retained Research: Heap Snapshotting

The architecture spec (03-runtime.md §4) designs **Heap Snapshotting**: after first boot, serialize
a versioned address inventory and frame payload into the reserved P3 region of the disk image, and on
a later boot replay those exact physical frames before resuming. The internal format v2, its canonical
checksum and the `EMPTY → WRITING → COMMITTED → CONSUMING → CONSUMED` state machine are implemented
and unit-tested, but **capture and restore are disabled in every shipping image** until all-hart
quiescence, closure, coherent staging and a real save→reset→restore→resume witness on a block-capable
board exist. No warm-boot timing is claimed — there is no measurement and no board witness.

This is retained architecture research, **not a separate active differentiator program**.
The earlier "Phase 29 — COMPLETE (2026-06-07)" status and its sub-100 ms figure had
no witness behind them and are withdrawn. Further work requires a named C2C-on-Intel dependency.

---

## Vision & Philosophy

### Historical Problem Framing

Traditional operating systems (Linux, Windows, macOS) inherit Unix's process model:
- **Process Isolation**: Hardware MMU enforces boundaries (expensive TLB flushes, context switches)
- **Capability Fragmentation**: Global permissions (uid/gid), not fine-grained capabilities
- **Kernel Complexity**: 20+ million LOC to handle process management
- **IPC Overhead**: Message passing across process boundaries requires syscalls + memory copies

**Current goal**: Deliver the bounded Intel C2C program above; the historical OS-wide framing does not authorize general-purpose expansion.

### Architecture Principles

1. **Cellular SAS**: Trusted native Cells share one address space
   - Compiler-enforced safety is subject to the trusted computing base, not a hardware boundary
   - Owned buffers and capability objects support local IPC; cross-tier/remote adapters have distinct contracts
   - No process cleanup on exit (Cells clean up explicitly via Drop)

2. **Language-Based Isolation**: Rust's type system supports the native trust model
   - Safe Cells use `#![forbid(unsafe_code)]`; reviewed driver/FFI exemptions remain explicit
   - Kernel/HAL unsafe operations require documented safety invariants
   - Do not infer protection from a compromised trusted component or arbitrary untrusted binary

3. **Nano-Kernel Philosophy**: Minimize trusted code
   - Kernel size is tracked by generated project status; driver and orchestration
     residue is addressed only as a named Intel C2C dependency or regression
   - Move filesystem, networking, drivers to userspace Cells
   - Each Cell is independently testable and upgradeable

4. **Capability-Based Access Control**: Fine-grained, no global permissions
   - Cells don't have uid/gid
   - IPC messages include capability grants
   - Revocation is automatic (Drop trait)

5. **Preserve Multi-Architecture Boundaries; Develop for Intel**
   - RV64, AArch64, and x86_64 retain their distinct build/smoke evidence
   - Existing RV32/AArch32 code and necessary regression coverage remain; new non-Intel platform work is paused
   - A successful HAL smoke is not hardware/product qualification or authorization for another platform program

---

## Project Structure

### Crates (111 active workspace members)

```
Kernel & Core
├── kernel              Nano kernel (size reported by generated project status; boundary migrations remain tracked)

Hardware Abstraction
├── hal/core            Facade (feature-gated)
├── hal/traits/*        Pure trait definitions, including arch kernel_abi hooks
├── hal/soc/*           Data-only SoC facts for riscv, arm-virt, bcm27xx, x86
├── hal/arch/riscv      RV64 and RV32 implementations
├── hal/arch/arm        AArch64 FULL (Ring-3 smoke)
└── hal/arch/x86        x86_64 FULL (Ring-3 smoke)

Boards
├── boards              Immutable board descriptors (`cellos-boards`, no_std)
└── boards/...          Seven active descriptors plus placeholder-only board docs

Public API (Stable ABI)
├── libs/types          Core types (VAddr, PAddr, ViError)
├── libs/api            Kernel-Cell boundary traits and syscall ABI
├── libs/ostd           Cells' standard library (syscall wrappers, I/O, alloc)
└── libs/*              attestation, http-core, text-engine, trusted ffi-posix support, ViUI, agent protocol

Cells
├── cells/tools/        init, shell, sys-tools, net-tools, wasm
├── cells/apps/         fb-console, robot-dashboard, Hypha cells
├── cells/demos/        Feature demos and game/demo cells
├── cells/drivers/      Shared hardware drivers (17 crates; not copied per board)
├── cells/services/     System services (12 crates)
├── cells/tests/        Disposable integration/stress cells
└── cells/runtimes/     Lua runtime; MicroPython is historical and not in workspace
```

### Total Codebase
- **Rust Code**: moving file/LOC totals belong in generated project status, not this PDR
- **Design Docs**: normative specifications plus generated status; exact counts are generated
- **Build lanes**: Intel x86-64 is the sole active direction; the documented
  [q35 lane](../boards/qemu/q35-x86_64/README.md) supplies software evidence.
  Existing RV64/ARM reference lanes remain for evidence and necessary regressions,
  not as primary development or new physical-qualification programs.

---

## Product Development Requirements (PDR)

### Current Program Requirements

| Requirement | Acceptance boundary |
|-------------|---------------------|
| One fixed headless Intel x86-64 configuration | Exact VT-x/EPT, VT-d, COM1, HPET, firmware/device and NIC requirements from the [HCL](hardware-compatibility-list.md); qualify the first machine before a second of the same model. No purchase is authorized by this PDR. |
| Tier 1 native C2C | Named native participants and their contracts, with retained signing, identity, capability and lifecycle gates. |
| Tier 2 C/C++ participation | Explicit adapters and on-path admission evidence on Intel; test-only admission and the C++ shim gap are not completion. |
| Tier 3 VM participation | Explicit guest adapters and Intel VMX/EPT evidence; SVM QEMU results cannot satisfy the Intel gate. Tier 3 is part of this target, not a browser-led escape hatch. |
| Local, LAN, and relay paths | Separate bounded witnesses and existing security gates; no implied automatic remote or production activation. |
| Scope discipline | Every task names its direct C2C-on-Intel deliverable, dependency, or necessary regression; retained historical plans cannot schedule independent work. |

Implementation order and evidence owners belong to [current focus](roadmap/current-focus.md). These are program targets, not claims of delivered support or approval to change an ABI.

### Historical Phase Requirements

The Phase 1–4 requirements below preserve prior technical planning and evidence.
Their dates, effort estimates, owners, unchecked items, and “current” labels are
historical snapshots, not active schedules or independent acceptance commitments.
Any resumed item must first satisfy the Intel C2C scope rule above.

### Phase 1: Core Stability (Historical — 2026-06)

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
littlefs at `/data`, and CellosFS Native at `/srv`. QEMU evidence does not replace
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
- [x] Lua network bindings (vnet module); MicroPython binding docs are historical

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

**Current Status**: Milestone 3.3 marked ✅ COMPLETE historically (2026-06-05: typed VFS IPC, io.open, vfs.stat/listdir/remove). Lua remains the active native scripting runtime; Python/R&D work belongs in the Tier 3 Linux VM path. Historical roadmap items about future Lua expansion stay archived.

#### 3.4 MicroPython Runtime Enhancement

**Status**: Historical implementation snapshot. MicroPython is not an active Cargo workspace member; current Python-compatible workloads belong in the Tier 3 Linux VM path.

**Historical requirement**: Python 3 subset execution environment.

**Current Status**: Milestone 3.4 is archived historical text (2026-06-05: `vfs_bridge.rs`, `modvfs.c`, typed VFS IPC). MicroPython is not a current workspace member; Python for R&D runs as full CPython inside the **Tier 3 Linux VM** (`apt install python3 pip numpy torch`), not as a native Cell.

**Success Metric**: Total Phase 3 effort = 500 hours (~12 weeks)

---

### Phase 4: Hot Migration & Advanced Features (2026-12 — 2027-03)

#### 4.1 Hot Migration (State Transfer)

**Requirement**: Update Cell binaries without shutting down.

**Current Status**: 🚧 PARTIAL — the verified supervisory hotswap path is complete (service-only `hotswap` CLI, exact shell-only `/bin/hotswap` launch edge, sender authorization, canonical request validation, and runtime evidence for state preservation / SpawnCap retention / cached FIFO / post-old-TID rejection / unauthorized denial). Broader generic state-transfer coverage remains future work.

**Acceptance Criteria**:
- [ ] Serialize Cell state (memory, registers, handles)
- [ ] Load new binary, restore state
- [ ] Resume execution with preserved file handles
- [ ] Zero-downtime shell update scenario

**Effort**: 120 hours  
**Owner**: TBD

#### 4.2 Advanced IPC

**Requirement**: Leasing, grant chains, bulk message passing.

**Current Status**: Send/Recv/Call/Reply plus lease operations,
SendGather/RecvScatter, and RecvTimeout are implemented. Cross-machine lease
coordination in `net-broker` remains separate unfinished work.

**Acceptance Criteria**:
- [x] Lease: Grant capability for duration, auto-revoke
- [ ] Grant chains: Cell A grants to B, B grants to C (transitive)
- [x] Bulk messages: Multi-buffer sends, gather/scatter
- [x] Timeout support on Recv

**Effort**: 60 hours  
**Owner**: TBD

#### 4.3 RV32 & ARM Support

**Requirement**: Full multi-architecture deployment.

**Current Status**: RV32-Nano boots under QEMU with context switch, traps,
timer, heap, and shell coverage. AArch32 has boot/context code and handoff tests,
but its prerequisites and physical qualification remain explicit gates.

**Acceptance Criteria**:
- [x] RISC-V 32-bit (RV32) HAL and QEMU boot path implemented
- [ ] ARM AArch32 production qualification complete
- [ ] Single binary selectable: `cargo build --features rv32 --release`
- [ ] Boot tests pass on all targets (QEMU simulation)

**Effort**: 200 hours  
**Owner**: TBD

#### 4.4 Benchmarking & Optimization

**Requirement**: Performance analysis, optimization.

**Current Status**: The benchmark cell implements context-switch, IPC,
syscall, memory-footprint, VFS-breakdown, and real-time scenarios. QEMU
integration exercises the suite; physical-target performance evidence remains
hardware-gated.

**Acceptance Criteria**:
- [ ] Context-switch latency < 100 µs
- [ ] Message latency (Send/Recv) < 50 µs
- [ ] Syscall overhead < 10 µs
- [ ] Memory footprint < 10 MB for kernel + 3 services
- [x] Public `ViBenchmark` trait and benchmark runner

**Effort**: 80 hours  
**Owner**: TBD

**Success Metric**: Total Phase 4 effort = 460 hours (~11 weeks)

---

## Technical Constraints & Dependencies

### Hardware Requirements

- **Sole active target**: One fixed, headless Intel x86-64 model, with exact requirements and qualification recorded in the [HCL](hardware-compatibility-list.md).
- **Software evidence lane**: [QEMU q35 x86-64](../boards/qemu/q35-x86_64/README.md); QEMU does not qualify physical hardware or Intel VMX.
- **NIC boundary**: Existing `igb` support is limited to `8086:10c9` (QEMU) and flash-backed i210 `8086:1533`; support is not actual-device qualification.
- **Retained reference**: The earlier RV64 QEMU minimum (128 MB RAM, one hart) is not an Intel sizing or hardware-purchase specification. New non-Intel board programs are paused.

### Software Stack

| Layer | Technology | Version | Status |
|-------|-----------|---------|--------|
| Bootloader | Limine | Latest | ✅ Working |
| Kernel | Rust nightly | 2024+ | ✅ Compiling |
| HAL | Custom traits | N/A | RV64/AArch64/x86_64 implemented with different smoke/qualification levels |
| Filesystems | MountTable: BootFS/RamFS/FAT/littlefs/CellosFS Native | Existing | FAT writes and littlefs `/data` shipped; `/srv` runs CellosFS Native with hardware qualification still phased |
| Runtimes | Lua active; MicroPython historical | 5.4 / archived 1.24.1 text | Python = Tier 3 VM |

### Key Dependencies

```toml
spin = "0.9"              # Spinlock (workspace dep)
virtio-drivers = "0.7.0"  # VirtIO block/GPU/input
xmas_elf = "0.9"          # ELF parsing
fatfs = "0.3"             # FAT32 filesystem
riscv = "0.16.0"          # RISC-V CSR access
```

### Breaking Changes

This direction approves no ABI change. Existing ABI review and compatibility gates remain mandatory.

---

## Historical Success Metrics (Phase 1)

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

## Historical Risk Assessment

The following is the prior phase risk snapshot, not the active work queue.
Use [current focus](roadmap/current-focus.md) and the [open risk register](roadmap/open-risk-register.md)
for current C2C-on-Intel gates; old mitigation priorities below do not authorize side programs.

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

## Historical Development Timeline

The G1 robot, G2 organization-server/office-PC, and G3–G5 product overlays are
retained in [product stages](./roadmap/product-stages.md) as historical context.
They are not independently scheduled or automatically unblocked by software closure.
The dated timeline below is historical planning, not an Intel C2C delivery promise,
physical qualification, or production commitment.

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
├─ Lua integration; MicroPython archived
└─ Milestone: User-Ready OS (2026-11-30)

Phase 4: Advanced Features (2026-12 — 2027-03)
├─ Hot migration
├─ Full RV32/ARM support
├─ Performance optimization
└─ Milestone: Production-Ready v1.0 (2027-03-31)
```

---

## Retained Non-Functional Targets

These earlier targets are not achieved guarantees or independent programs.
Only requirements tied to a named Intel C2C deliverable are active; measurement,
isolation, and hardware claims require their own evidence.

| Requirement | Target | Method |
|-------------|--------|--------|
| **Reliability** | 99.5% uptime | Watchdog timers, graceful shutdown |
| **Performance** | < 100 µs context switch | Benchmarking suite |
| **Security** | Reviewed trust/admission boundaries; no blanket SAS isolation claim | Rust compiler checks plus explicit unsafe-code, Tier 2/Tier 3, and security gates |
| **Maintainability** | Responsibility-bounded kernel with generated total/core nLOC trend | Spec 15 + [generated metrics](code-metrics.generated.md) |
| **Scalability** | Per-request profile goal: 1000 simultaneous isolated cells after staged 64/128/256/512 measurements | Shared immutable image frames, demand-paged stacks, profile quotas, dynamic tables |
| **Portability** | Preserve existing RV64/ARM/x86 contracts; Intel x86-64 alone is active | Feature-gated HAL and necessary regression coverage |

---

## Stakeholders

- **Core Team**: DXSL (tinyong@vigroup.ai)
- **Advisors**: Theseus (UC Santa Cruz), Asterinas (TBD), Tock (Google)
- **Community**: Open source contributors (GitHub)

---

## Retained Engineering Success Criteria

This list preserves engineering evidence and gaps, not an independent multi-architecture
roadmap. Current program acceptance is defined above and in ADR-0022/current focus.

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

- [ADR-0022 — Intel x86-64 C2C-only direction](decisions/0022-intel-x86-64-c2c-only-direction.md)
- [Current focus — active program gates](roadmap/current-focus.md)
- **codebase-summary.md** — File structure & metrics
- **code-standards.md** — Coding rules & conventions
- **system-architecture.md** — High-level design
- **project-roadmap.md** — Phase progress tracking
- **CLAUDE.md** — 8 Coding Laws (auto-loaded)
- **docs/0X-*.md** — Detailed specifications
