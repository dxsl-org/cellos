# ViOS System Architecture

**Audience**: Developers new to ViOS  
**Level**: High-level (conceptual + key components)  
**Version**: 0.2.1-dev (Mycelium Era)  
**Last Updated**: 2026-06-03

---

## Core Philosophy

ViOS is **NOT** a traditional Linux-style OS. It uses:

- **Cellular Architecture**: Software organized as **Cells** (not processes), all sharing one address space
- **Language-Based Isolation**: Rust's type system (not hardware MMU) provides isolation
- **Single Address Space (SAS)**: Kernel and all Cells live in one virtual memory space, with no process boundaries
- **Zero-Copy IPC**: Capability-based message passing using owned buffers

**Impact**: No expensive context switches, no TLB flushes, minimal privilege escalation overhead.

---

## System Layers

```
┌─────────────────────────────────────────┐
│  Cells (Applications, Drivers, Services) │  Apps: hello, shell, lua, micropython
├──────────────────────────────────────────┤  Drivers: disk, gpu, input, net, serial
│  Kernel (Nano Kernel, ~8,700 LOC)       │  Services: vfs, config, compositor, net, power
├──────────────────────────────────────────┤
│  HAL (Hardware Abstraction Layer)        │  RV64 ✅, AArch64 ✅ (Ring-3), x86_64 ✅ (Ring-3)
├──────────────────────────────────────────┤
│  Hardware (QEMU, Bare-metal)             │  Memory, CPU, Devices
└─────────────────────────────────────────┘
```

---

## Kernel (nano-kernel, ~8,700 LOC)

The kernel is **tiny** by design, handling only:

### 1. **Boot & Initialization** (`kernel/src/boot.rs`)
- Limine bootloader integration (fallback: SimpleBootInfo)
- Parse DTB (device tree)
- Initialize UART for logging
- Initialize HAL (interrupts, paging)
- Set up frame allocator
- Initialize memory (paging, heap)
- Initialize scheduler
- Spawn init Cell
- Enable interrupts and enter idle loop

### 2. **Memory Management** (`kernel/src/memory/`)

**Frame Allocator**:
- Bitmap-based allocation (O(1) free, O(n) scan for allocate)
- 128–256 MB physical RAM in QEMU (0x8000_0000–0x8000_0000 + size)
- Tracks allocated vs. free pages (4KB each)

**Virtual Memory (SV39 on RV64)**:
- **Trap Zone**: Low 4KB, unmapped → catches NULL deref
- **User VA**: < 0x8000_0000 (per-task isolation via page tables)
- **Guard Hole**: 0x8000_0000–0x8020_0000 (unmapped, prevents overflow)
- **Kernel VA**: 0x8020_0000+ (identity-mapped)
- **Heap**: 64 MB kernel heap (linked-list allocator)

**Paging Structure** (RV39):
```
User Space: 1 GB (virt addr < 0x8000_0000)
├─ Stack: top of user VA (grows down)
├─ Heap: dynamic (grows up)
└─ Code/Data: loaded from ELF

Kernel Space: (virt addr 0x8020_0000+)
├─ Code: kernel binary
├─ Data: statics, globals
├─ Heap: kernel allocations
└─ Page Tables: per-task
```

### 3. **Task Scheduler** (`kernel/src/task/scheduler.rs`)

**Round-Robin with Time Slices**:
- All Cells scheduled fairly
- Each gets ~10ms time slice (configurable)
- Yield/preempt on timer interrupt

**Task Control Block (TCB)**:
```rust
struct Task {
    id: TaskId,
    state: TaskState,          // Running, Ready, Blocked, Dead
    cpu_context: TrapFrame,    // Registers, PC, SP
    page_table: PageTable,     // Task's virtual memory
    parent: TaskId,            // Parent Cell for tracking
    ipc_queue: Queue<Message>, // Incoming IPC messages
    grants: Vec<Grant>,        // Capability objects
}
```

**States**:
- `Running` — executing on CPU
- `Ready` — waiting for CPU
- `Blocked` — waiting for IPC message or I/O
- `Dead` — finished, pending cleanup

### 4. **IPC System** (`kernel/src/task/ipc.rs`)

10 core syscalls (vs. Linux's 300+):

| Syscall | Purpose |
|---------|---------|
| `Send(to, msg, cap)` | Send message to Cell, optionally grant capability |
| `Recv(from_filter, timeout)` | Receive message (blocks if none) |
| `Call(to, msg, cap)` | Send + wait for reply (RPC) |
| `Reply(to, msg)` | Reply to caller |
| `Spawn(binary, argv)` | Create new Cell |
| `Exec(binary, argv)` | Replace self with new Cell |
| `SpawnFromMem(ptr, size)` | Load Cell from memory buffer |
| `Exit(code)` | Terminate self |
| `Yield()` | Voluntarily yield CPU |
| `Log(msg)` | Print to kernel log |

**Capability-Based Access Control**:
```rust
pub struct Capability {
    rights: u32,  // Read, Write, Execute, etc.
    target: CellId,
}

pub struct Grant {
    cap: Capability,
    from_cell: CellId,
    to_cell: CellId,
    // Revoked on drop
}
```

### 5. **ELF Loader** (`kernel/src/loader.rs`)

- Parse ELF header
- Load segments (allocate frames, map to vaddr)
- Apply relocations (position-independent code)
- Set up stack, heap pointers
- Enter user-space at `_start`

### 6. **Filesystem (FAT32)** (`kernel/src/fs/`)

- Read-only FAT32 parser for boot
- Contains: `/bin/shell`, `/bin/hello`, `/bin/lua`, `/bin/cat`, `/bin/ls`
- Kernel uses this to spawn init Cell

---

## Hardware Abstraction Layer (HAL)

### Traits (Pure Interfaces)

```rust
// hal/traits/arch/lib.rs
pub trait Arch {
    fn init();
    fn switch_context(old: &TrapFrame, new: &TrapFrame);
    fn enable_interrupts();
    fn disable_interrupts();
}

// hal/traits/paging/lib.rs
pub trait PageTableTrait {
    fn map(&mut self, va: VAddr, pa: PAddr, flags: u32);
    fn unmap(&mut self, va: VAddr);
    fn translate(&self, va: VAddr) -> Option<PAddr>;
}

// hal/traits/interrupt/lib.rs
pub trait InterruptController {
    fn init();
    fn enable_irq(irq: u32);
    fn disable_irq(irq: u32);
    fn ack_irq(irq: u32);
}
```

### Implementations

**RISC-V 64-bit (RV64) — FULLY IMPLEMENTED** ✅
- `hal/arch/riscv/src/rv64/context.rs` — Trap frame, context switch
- `hal/arch/riscv/src/rv64/paging.rs` — SV39 page table walker
- `hal/arch/riscv/src/rv64/trap.rs` — Exception/interrupt handler
- `hal/arch/riscv/src/rv64/boot.rs` — Assembly entry (_start, trap setup)
- `hal/arch/riscv/src/common/uart_ns16550a.rs` — Serial UART
- `hal/arch/riscv/src/common/sbi.rs` — SBI calls (shutdown, time)
- `hal/arch/riscv/src/common/timer.rs` — SBI timer (scheduling)

**ARM AArch64 — FULLY IMPLEMENTED** ✅ (Ring-3 smoke testing in QEMU)  
**x86_64 — FULLY IMPLEMENTED** ✅ (Ring-3 smoke testing in QEMU)  
**RV32, AArch32 — TRAIT STUBS** (trait impls only, no boot code)

### Multi-Architecture Strategy

Use `#[cfg(target_arch = "riscv64")]` to conditionally compile:

```rust
#[cfg(target_arch = "riscv64")]
mod riscv;

#[cfg(target_arch = "arm")]
mod arm;

pub use crate::riscv::*;  // Or arm::* depending on build
```

---

## VirtIO Device Integration

### MMIO Memory Mapping

**Problem**: Limine bootloader does not report MMIO ranges in its memory map, causing device registers to become inaccessible after kernel paging is activated.

**Solution**: Explicit identity-mapping in `kernel/src/memory/paging.rs::init_kernel_paging()`:

```rust
// QEMU virt machine MMIO layout (RV64)
// CLINT (Core Local INTerrupt)
map(VAddr(0x0200_0000), PAddr(0x0200_0000), 0x10000, READABLE | WRITABLE | VALID);

// PLIC (Platform Level Interrupt Controller)
map(VAddr(0x0C00_0000), PAddr(0x0C00_0000), 0x0400_0000, READABLE | WRITABLE | VALID);

// UART0 + VirtIO MMIO devices (slot 0–7)
map(VAddr(0x1000_0000), PAddr(0x1000_0000), 0x0001_0000, READABLE | WRITABLE | VALID);
```

All MMIO regions are identity-mapped (VA = PA) for simplicity and to preserve bootloader-assigned addresses.

### VirtIO IRQ Dispatch Pattern

VirtIO devices on QEMU `virt` machine use PLIC IRQs with slot-based numbering:

| Device | MMIO Slot | Base Address | IRQ |
|--------|-----------|--------------|-----|
| UART0  | —         | 0x1000_0000  | 10  |
| VirtIO Block | 0 | 0x1000_1000 | 1 |
| VirtIO Input | 1 | 0x1000_2000 | 2 |
| VirtIO Net | 2 | 0x1000_3000 | 3 |
| ... | i | 0x1000_(i+1)000 | i+1 |

**IRQ Dispatch**: `kernel/src/task/drivers/virtio_blk.rs::vi_handle_virtio_irq(irq: u32)`

```rust
pub fn vi_handle_virtio_irq(irq: u32) -> bool {
    match irq {
        1 => virtio_blk::block_device_irq(),     // VirtIO block (slot 0)
        2 => virtio_input::input_device_irq(),   // VirtIO input (slot 1)
        3 => virtio_net::net_device_irq(),       // VirtIO net (slot 2)
        _ => false,  // Unknown IRQ
    }
}
```

**Per-Device Handler Responsibilities** (Phase 05 established):
1. Drain the used ring to retrieve completed requests and process data
2. **Acknowledge the IRQ** via `ack_irq(irq)` to clear device `InterruptStatus` register
3. Re-arm the device by publishing empty buffers back to the available ring
4. Wake any blocked tasks waiting on device I/O

**Interrupt Flow (Correct Pattern)**:
```
Device generates interrupt
  ↓
PLIC sets bit in Pending register
  ↓
PLIC delivers IRQ to CPU
  ↓
Kernel trap handler calls vi_handle_virtio_irq(irq)
  ↓
Device handler:
  - Process available data/requests
  - Call ack_irq(irq) to clear InterruptStatus
  - Refill available ring
  ↓
PLIC acknowledges via plic_complete()
  ↓
Device can fire next interrupt (if new data arrives)
```

**Critical Fix (Phase 05)**: Input device was not calling `ack_irq()`, leaving `InterruptStatus` register set. PLIC would immediately re-fire the same interrupt after `plic_complete()`, creating an infinite interrupt storm. This caused kernel to hang on first keystroke. Fix: Added `pub static INPUT_DEVICE_IRQ` and `pub fn ack_irq()` to `kernel/src/task/drivers/virtio_input.rs`; expanded `vi_handle_virtio_irq()` to dispatch to input device handler.

### FAT16 Persistence & Graceful Shutdown (Phase E)

**Hardening** (safety fixes, no behavior change):
- `cells/services/vfs/src/block_stream.rs` — SeekFrom::Current now validates result ≥ 0 before u64 cast (prevents underflow→seek to arbitrary LBA)
- `kernel/src/task/syscall.rs` — BlkRead/BlkWrite now reject sectors ≥ CELL_TABLE_BASE_LBA (82,000) to prevent cell from corrupting kernel bootstrap table

**Clean Shutdown Path**:
- Syscall 502 (raw, no `ViSyscall` enum entry) — kernel SBI SRST handler calls OpenSBI to power off
- `cells/apps/shell/src/cmd_sys.rs` — `shutdown` built-in command triggers graceful QEMU exit
- Test harness `wait_for_natural_exit()` allows disk image to flush before reboot

**Integration Test** (`vfs_fat16_reboot_persistence`):
- Writes marker to FAT16 `/data/`, issues shutdown, waits for QEMU clean exit
- Reboots against same disk image, reads marker back to prove write durability across power cycle
- **Critical bug fixed during this phase**: `shell.rs` had pre-parser echo handler that split by whitespace, completely bypassing redirect parser. Removed handler; echo now correctly goes through parser and supports OP_WRITE redirects.

---

## Public API (Kernel-Cell Boundary)

Located in `libs/api/`, these traits define the stable ABI:

### Filesystem (`ViFileSystem`, `ViFile`)
```rust
pub trait ViFileSystem {
    async fn open(&self, path: &str, flags: u32) -> ViResult<Box<dyn ViFile>>;
    async fn read_dir(&self, path: &str) -> ViResult<Vec<DirEntry>>;
}

pub trait ViFile {
    async fn read(&mut self, buf: Box<[u8]>) -> ViResult<Box<[u8]>>;
    async fn write(&mut self, data: &[u8]) -> ViResult<usize>;
    async fn seek(&mut self, pos: u64) -> ViResult<u64>;
}
```

### Block Devices (`ViBlockDevice`)
```rust
pub trait ViBlockDevice {
    async fn read(&self, sector: u64, count: u32) -> ViResult<Box<[u8]>>;
    async fn write(&self, sector: u64, data: &[u8]) -> ViResult<u32>;
}
```

### Networking (`ViTcpStack`, `ViTcpStream`)
```rust
pub trait ViTcpStack {
    async fn listen(&self, addr: &str, port: u16) -> ViResult<Box<dyn ViTcpListener>>;
    async fn connect(&self, addr: &str, port: u16) -> ViResult<Box<dyn ViTcpStream>>;
}
```

### Drivers (`ViDriver`)
```rust
pub trait ViDriver {
    fn name(&self) -> &str;
    fn probe(&mut self) -> ViResult<()>;
    fn capabilities(&self) -> u32;
}
```

### Runtime (`ViVmRuntime`)
```rust
pub trait ViVmRuntime {
    fn load(&mut self, bytecode: &[u8]) -> ViResult<()>;
    fn execute(&mut self, function: &str, args: &[Value]) -> ViResult<Value>;
}
```

---

## Cells (User-Space Software)

### What is a Cell?

A **Cell** is an isolated execution context (like a process) but:
- Shares kernel's address space (no context-switch overhead)
- Cannot use `unsafe` code (Rust enforces this)
- Communicates via syscalls (IPC, filesystem, logging)
- Has its own task control block, page table, and message queue

### Cell Types

**Applications**: Shell, hello world, Lua/MicroPython runtimes
```
cells/apps/shell/     — Interactive REPL (parser, executor, aliases, jobs, history)
cells/apps/init/      — Bootstrap (spawns vfs, config, shell)
cells/apps/hello/     — Test app
```

**Drivers**: Hardware device drivers
```
cells/drivers/disk/   — VirtIO block passthrough (✅ working)
cells/drivers/gpu/    — VirtIO GPU (opt-in framebuffer)
cells/drivers/input/  — VirtIO input passthrough
cells/drivers/net/    — VirtIO NIC wrapper
```

**Services**: System services with long-lived state
```
cells/services/vfs/   — RamFS + FAT32 (✅ read working)
cells/services/config/— Key-value store (✅ ViStateTransfer impl)
cells/services/compositor/ — Software blending + z-order
cells/services/input/ — Input event routing
cells/services/net/   — smoltcp TCP/IP + DHCP (✅ DHCP working)
```

**Runtimes**: VMs/interpreters for scripting
```
cells/runtimes/lua/       — Lua 5.4 via FFI (✅ REPL verified)
cells/runtimes/micropython/ — MicroPython 1.24.1 via FFI (✅ REPL verified)
```

### Cell Lifecycle

```
1. Boot kernel
   ↓
2. Kernel spawns "init" Cell from embedded binary
   ↓
3. Init spawns "config" service (KV store)
   ↓
4. Init spawns "vfs" service (filesystem server)
   ↓
5. Init spawns "shell" application (interactive REPL)
   ↓
6. User types commands → shell sends IPC to vfs/config
   ↓
7. Shell displays output from services
   ↓
8. Ctrl+A X to shutdown
```

---

## Boot Sequence (Visual)

```
┌─────────────────────────────────────────────────┐
│ Bootloader (Limine or OpenSBI)                  │
│ Sets up: memory, DTB, argc/argv                 │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ kernel/src/boot.rs: kmain(hartid, dtb)          │
│ 1. Initialize UART for logging                  │
│ 2. Parse bootloader info (memory map, DTB)      │
│ 3. Initialize HAL (traps, interrupt handler)    │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ kernel/src/main.rs: _km_start()                 │
│ 4. Frame allocator (bitmap)                     │
│ 5. Virtual memory (SV39 paging)                 │
│ 6. Heap allocator (64 MB)                       │
│ 7. PLIC (interrupt controller)                  │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ kernel/src/task.rs: init_scheduler()            │
│ 8. Task allocator (TCB pool)                    │
│ 9. Load "init" Cell from embedded FAT32         │
│ 10. Enter scheduler loop                        │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ cells/apps/init/src/main.rs: main()             │
│ 11. Spawn "config" service via syscall::spawn() │
│ 12. Spawn "vfs" service                         │
│ 13. Spawn "shell" application                   │
│ 14. Idle (let scheduler handle)                 │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ cells/apps/shell/src/main.rs: main()            │
│ 15. Print prompt: "viosh> "                     │
│ 16. Read user input (async)                     │
│ 17. Parse command (echo, cat, ls, etc.)         │
│ 18. Send IPC to vfs/config services             │
│ 19. Display response                            │
│ 20. Loop to step 15                             │
└─────────────────────────────────────────────────┘
```

---

## Memory Layout (SV39 RV64)

```
Virtual Address Space (64-bit, SV39 = 39-bit VA)
┌───────────────────────────────────┐
│  User Space (< 0x8000_0000)       │  Per-task, isolated via page table
│  - Stack (top, grows down)        │
│  - Heap (dynamic, grows up)       │
│  - Code/Data (ELF loaded here)    │
└─────────────────────────────────────┘  0x7fff_ffff

┌───────────────────────────────────┐
│  Guard Hole (unmapped)            │  0x8020_0000 - 0x7fff_ffff
│  Prevents user/kernel overflow    │
└───────────────────────────────────┘  0x8020_0000

┌───────────────────────────────────┐
│  Kernel Space (≥ 0x8020_0000)     │  Identity-mapped, shared
│  - Code: kernel binary            │
│  - Data: statics, globals         │
│  - Heap: kernel allocator         │
│  - Page tables (per-task)         │
│  - Task pool (TCBs)               │
└───────────────────────────────────┘  0xffff_ffff_ffff_ffff

Physical RAM: 0x8000_0000–0x8800_0000 (default: 128 MB in QEMU)
```

---

## IPC & Message Passing

### Send Message (Async)

```
┌────────────────────────────────────┐
│ Cell A (shell)                     │
│ syscall::send(vfs_id, msg, grant) │
│ (doesn't block, returns immediately)
└────────────────────┬───────────────┘
                     ↓
            ┌─────────────────┐
            │ Kernel          │
            │ - Validates msg │
            │ - Queues in VFS │
            │ - Wakes VFS     │
            └────────┬────────┘
                     ↓
            ┌─────────────────┐
            │ Cell B (vfs)    │
            │ woken by kernel │
            │ syscall::recv() │
            └─────────────────┘
```

### Call & Reply (RPC)

```
┌────────────────────────────────────┐
│ Cell A (shell)                     │
│ syscall::call(vfs_id, req, cap)   │
│ BLOCKS, waiting for reply          │
└────────────────────┬───────────────┘
                     ↓
            ┌─────────────────┐
            │ Kernel          │
            │ - Queues msg    │
            │ - Blocks Cell A │
            └────────┬────────┘
                     ↓
            ┌──────────────────────┐
            │ Cell B (vfs)         │
            │ syscall::recv()      │
            │ → gets request       │
            │ process...           │
            │ syscall::reply(A, rsp)
            └────────┬─────────────┘
                     ↓
            ┌─────────────────┐
            │ Kernel          │
            │ - Unblocks A    │
            │ - Delivers rsp  │
            └────────┬────────┘
                     ↓
            ┌──────────────────────┐
            │ Cell A resumes       │
            │ receives reply       │
            │ continues...         │
            └──────────────────────┘
```

---

## Current Status (2026-06-03)

### ✅ Implemented (Phases 01, 02, 05, 10, 14, 15, 16, 18, 20, C, D)
- **RV64, AArch64, x86_64** HAL with paging (SV39/4K/4K respectively)
- **Nano kernel** (~8,700 LOC) with round-robin scheduler
- **48 syscall variants** (IPC, memory, task, FS, GPU, network, state)
- **Block I/O syscalls** (raw 500/501 for FAT16 persistence)
- Frame allocator (bitmap) and virtual memory
- ELF loader with PIE relocation support
- **VFS service** (RamFS read/write, FAT16 write via block device)
- **FAT16 filesystem** (LBA 0–81919 on VirtIO disk, /data/* paths persistent)
- **Config service** (KV store with ViStateTransfer)
- **Interactive shell** with pipes, redirection, background jobs, history, aliases, echo built-in
- **Lua 5.4** runtime (multi-line REPL, VFS I/O FFI, ViStateTransfer) — verified
- **MicroPython 1.24.1** runtime (REPL, 256KB heap) — verified
- **Keyboard input** (VirtIO, multi-key support, no deadlock)
- **Network** (smoltcp, DHCP verified, data-path stub)
- **GPU framebuffer** (opt-in, basic compositor)
- **HotSwap orchestrator** (5-step live Cell replacement, kernel + shell + config + vfs verified)
- **Workspace consolidated** with 0 cargo warnings
- **CI/CD pipeline** with architecture validation (10/10 score)

### 🚧 In Progress / Partial
- **Network opcodes** (SOCKET_STATE 0x19 added; LISTEN/ACCEPT partial; full multi-connection server deferred)
- **KASLR** (not implemented)

### ⏳ Planned (Later phases)
- Per-Cell SATP (address space isolation)
- Audit logging
- Ed25519 signing (spec only)
- Additional architecture ports

---

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Single Address Space | Reduce context-switch overhead, simplify memory management |
| Language-Based Isolation | Rust's type system enforces isolation better than hardware |
| Round-Robin Scheduler | Simple, fair, predictable for embedded real-time systems |
| Capability-Based Access | Fine-grained control, no global permissions |
| Owned Buffers in Async | Deterministic cleanup in SAS (no process teardown) |
| Nano Kernel (~8,700 LOC) | Keep TCB, minimize trusted code, move features to Cells |
| Trait-Based HAL | Multi-architecture support without code duplication |
| No mod.rs | Clearer module boundaries, IDE-friendly |

---

## See Also

- **CLAUDE.md** — 8 Coding Laws & quick reference
- **api-reference.md** — Full trait & syscall reference
- **patterns.md** — Common code patterns
- **codebase-summary.md** — File structure & LOC counts
- **code-standards.md** — Code style & naming
- **Specs**: `docs/specs/0X-*.md` — Detailed subsystem specifications
