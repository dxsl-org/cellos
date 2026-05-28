# ViOS Codebase Summary

**Project**: ViOS (Jarvis Hybrid OS)  
**Version**: 0.2.0 (Mycelium Era)  
**Language**: Rust (nightly, `no_std`)  
**Total LOC**: ~12,600 Rust + supporting files  
**Last Updated**: 2026-05-28

---

## Directory Structure

```
vios/
├── kernel/                    Nano Kernel (~5,300 LOC)
│   ├── src/
│   │   ├── main.rs           Kernel entry, boot orchestration (254 LOC)
│   │   ├── boot.rs           Limine bootloader + SimpleBootInfo fallback (274 LOC)
│   │   ├── boot/             Arch-specific boot code (RISC-V asm)
│   │   ├── boot.rs           Bootloader integration
│   │   ├── cell.rs           Cell registry & metadata (308 LOC)
│   │   ├── cell/             Cell lifecycle
│   │   ├── memory.rs         Frame allocator facade (729 LOC)
│   │   ├── memory/           Memory management
│   │   │   ├── frame_alloc.rs  Bitmap-based frame allocator
│   │   │   ├── heap.rs       Kernel heap management
│   │   │   └── paging.rs     Virtual memory, SV39 page tables
│   │   ├── task.rs           Scheduler & syscalls (31,986 LOC)
│   │   ├── task/             Task management
│   │   │   ├── scheduler.rs  Round-robin scheduler
│   │   │   ├── syscall.rs    10 core syscalls
│   │   │   ├── ipc.rs        Send/Recv/Call/Reply/Grant/Lease
│   │   │   └── tcb.rs        Task control block
│   │   ├── loader.rs         ELF linker & relocator (223 LOC)
│   │   ├── loader/           ELF loading
│   │   ├── fs.rs             Filesystem facade (637 LOC)
│   │   ├── fs/               FAT32 filesystem
│   │   ├── sync.rs           Spinlock synchronization (82 LOC)
│   │   ├── intrinsics.rs     Panic handler, compiler builtins (54 LOC)
│   │   ├── prelude.rs        Common imports
│   │   └── embedded/         Embedded disk images (test binaries)
│   ├── Cargo.toml
│   ├── build.rs
│   └── linker.ld             RV64 linker script
│
├── hal/                       Hardware Abstraction Layer (~1,200 LOC)
│   ├── core/
│   │   └── src/lib.rs        Feature-gated arch facade (47 LOC)
│   ├── traits/                Pure trait definitions (no impl)
│   │   ├── arch/src/lib.rs   Arch trait: init, context switch, interrupts
│   │   ├── paging/src/lib.rs PageTableTrait: map, unmap, translate
│   │   ├── interrupt/src/lib.rs InterruptController: enable/disable/ack
│   │   ├── timer/src/lib.rs  Timer trait (clock)
│   │   ├── uart/src/lib.rs   UART trait (serial I/O)
│   │   └── display/src/lib.rs Display trait (framebuffer)
│   ├── arch/
│   │   ├── riscv/            RV32/RV64 FULLY IMPLEMENTED (~428 LOC)
│   │   │   ├── src/lib.rs
│   │   │   ├── src/rv64.rs   RV64 entry (SV39 paging, PLIC, SBI, NS16550A)
│   │   │   ├── src/rv32.rs   RV32 stub (4 LOC)
│   │   │   ├── src/common.rs Shared utilities
│   │   │   ├── src/common/timer.rs SBI clock
│   │   │   ├── src/common/uart_ns16550a.rs UART driver
│   │   │   ├── src/common/sbi.rs SBI calls
│   │   │   ├── src/rv64/boot.rs RISC-V assembly entry
│   │   │   ├── src/rv64/context.rs Context switch (trap frames)
│   │   │   ├── src/rv64/paging.rs SV39 page table walker
│   │   │   └── src/rv64/trap.rs Exception/interrupt handler
│   │   ├── arm/              AArch64 STUBS ONLY (~53 LOC)
│   │   └── x86/              x86_64 STUBS ONLY (~46 LOC)
│   ├── Cargo.toml (core), arch/riscv/Cargo.toml, etc.
│
├── libs/                      Public APIs & Utilities (~3,000 LOC)
│   ├── types/
│   │   └── src/lib.rs        Core types: VAddr, PAddr, CellId, ViError (135 LOC)
│   ├── api/                  PUBLIC ABI (Kernel-Cell boundary)
│   │   └── src/
│   │       ├── lib.rs        Trait exports (1,271 LOC)
│   │       ├── fs.rs         ViFileSystem, ViFile traits
│   │       ├── block.rs      ViBlockDevice trait
│   │       ├── net.rs        ViTcpStack, ViTcpStream traits
│   │       ├── driver.rs     ViDriver trait
│   │       ├── runtime.rs    ViVmRuntime for VM cells
│   │       ├── config.rs     ViConfig trait
│   │       ├── benchmark.rs  ViBenchmark trait
│   │       ├── posix.rs      POSIX C Library shim (stdio, stdlib, string)
│   │       ├── state_transfer.rs ViStateTransfer (hot migration)
│   │       └── async_traits.rs Async versions (ViAsyncFile, etc.)
│   └── ostd/                 Cells' Standard Library (~1,543 LOC)
│       └── src/
│           ├── lib.rs
│           ├── syscall.rs    Syscall wrappers (Send, Recv, Call, Reply, etc.)
│           ├── io.rs         I/O macros (println!, eprintln!)
│           ├── alloc.rs      Allocator interface
│           ├── prelude.rs    Common exports
│           └── fs.rs         VFS wrappers
│
├── cells/                     Applications, Drivers, Services (~1,800 LOC)
│   ├── apps/
│   │   ├── init/             Bootstrap orchestrator (114 LOC)
│   │   │   └── src/main.rs   Spawns config, vfs, shell services
│   │   ├── shell/            Interactive REPL (571 LOC)
│   │   │   ├── src/main.rs   Async shell event loop
│   │   │   ├── src/shell.rs  Command parsing & execution
│   │   │   ├── src/commands.rs Echo, cat, ls, pwd, cd, help
│   │   │   ├── src/config_client.rs Config KV client
│   │   │   ├── src/async_utils.rs Read stdin, timer utilities
│   │   │   ├── build.rs      Embedding binary assets
│   │   │   └── shell.ld      Linker for shell binary
│   │   ├── hello/            Hello World test (12 LOC)
│   │   ├── utils/            cat, echo, ls binaries (39 LOC)
│   │   ├── test-isolation/   Compile-time isolation check (32 LOC)
│   │   └── app.ld            Link script for apps
│   ├── drivers/              Hardware drivers (~227 LOC, mostly STUBS)
│   │   ├── disk/             RamDisk + VirtIO block (IMPLEMENTED)
│   │   ├── gpu/              VirtIO GPU (STUB)
│   │   ├── input/            Keyboard/mouse (STUB)
│   │   ├── net/              NIC driver (STUB)
│   │   ├── serial/           Serial output (STUB)
│   │   └── wasm/             WASM runtime (STUB)
│   ├── services/             System services (~270 LOC)
│   │   ├── vfs/              Virtual filesystem (RamFS, IMPLEMENTED)
│   │   │   └── src/main.rs   File handle serving, /bin/, /dev/
│   │   ├── config/           Key-value store (IMPLEMENTED)
│   │   │   └── src/main.rs   IPC protocol for config reads
│   │   ├── compositor/       Graphics compositor (STUB)
│   │   ├── input/            Input event routing (STUB)
│   │   ├── net/              Network stack (STUB)
│   │   └── power/            Power management (STUB)
│   └── runtimes/             VM/Script runtimes
│       ├── lua/              Lua 5.4 FFI bindings
│       │   ├── build.rs      C compilation via cc crate
│       │   ├── src/c/        Lua 5.4 source
│       │   └── src/lib.rs    Rust bindings
│       └── micropython/      MicroPython 1.24.1 FFI bindings
│
├── tests/                    Architecture validation suite
│   ├── architecture-validation/
│   │   ├── step1_*.md        Spec verification checks
│   │   └── step2_*.md        Dependency analysis (10/10 score)
│
├── tools/
│   └── mkfat32.py            Disk image creation script
│
├── docs/                     Design specifications & guides
│   ├── 00-context.md         Prime directive & 8 laws
│   ├── 00-fork.md            Forking from other projects
│   ├── 01-core.md            Cellular philosophy & linker
│   ├── 02-memory.md          SAS, HHDM, registry
│   ├── 03-runtime.md         Async safety & owned buffers
│   ├── 04-hardware.md        Multi-arch HAL
│   ├── 05-application.md     Native/WASM/VM apps
│   ├── 06-graphics.md        Graphics & compositor
│   ├── 07-networking.md      Network stack
│   ├── 08-power.md           Power management
│   ├── 09-vfs.md             Filesystem (VFS)
│   ├── 10-testing.md         Testing strategy
│   ├── 11-shell.md           Shell design
│   ├── ARCHITECTURE.md       Full system design (32KB)
│   ├── CODING_GUIDE.md       Coding patterns (24KB)
│   ├── API.md                Complete API reference (22KB)
│   ├── ONBOARDING.md         Developer onboarding (23KB)
│   ├── PATTERNS.md           Common patterns (20KB)
│   ├── INSTALLATION.md       Build & setup (16KB)
│   ├── TECH_STACK.md         Tech stack details (17KB)
│   └── 99-roadmap.md         Development roadmap
│
├── .github/workflows/        CI/CD pipelines
│   ├── ci.yml                Lint, build, security checks
│   └── test.yml              Architecture tests
│
├── .cargo/config.toml        Cargo settings (RISC-V target defaults)
├── Cargo.toml                Workspace manifest (21 crates)
├── Cargo.lock                Dependency lock
├── CLAUDE.md                 AI agent guidelines (auto-loaded)
├── README.md                 Project overview
└── repomix-output.xml        Full codebase dump (for LLM analysis)
```

---

## Crate Organization

### Workspace Members (21 total)

**Kernel & Core**
- `kernel` — Nano kernel (5,300 LOC)

**HAL (Hardware Abstraction)**
- `hal/core` — Facade & re-exports
- `hal/traits/arch`, `timer`, `interrupt`, `uart`, `display`, `paging` — Pure traits
- `hal/arch/riscv`, `arm`, `x86` — Arch implementations (RV64 done, ARM/x86 stubs)

**Libraries (Public ABI)**
- `libs/types` — VAddr, PAddr, ViError, CellId
- `libs/api` — Kernel-Cell ABI (ViFileSystem, ViDriver, ViNetTcpStack, etc.)
- `libs/ostd` — Cells' standard library (syscall wrappers, I/O, alloc)

**Cells - Drivers**
- `cells/drivers/disk`, `gpu`, `input`, `net`, `serial`, `wasm`

**Cells - Services**
- `cells/services/vfs` — Virtual filesystem (RamFS)
- `cells/services/config` — Key-value store
- `cells/services/compositor`, `input`, `net`, `power` (stubs)

**Cells - Apps**
- `cells/apps/init` — Bootstrap
- `cells/apps/shell` — Interactive REPL
- `cells/apps/hello` — Test app
- `cells/apps/utils` — cat, echo, ls utilities
- `cells/apps/test-isolation` — Compile-time checks

**Cells - Runtimes**
- `cells/runtimes/lua` — Lua 5.4 FFI
- `cells/runtimes/micropython` — MicroPython 1.24.1 FFI

---

## Key Metrics

| Aspect | Count |
|--------|-------|
| Total Rust LOC | ~12,600 |
| Kernel LOC | ~5,300 |
| HAL LOC | ~1,200 |
| Libraries LOC | ~3,000 |
| Cells LOC | ~1,800 |
| Crates | 21 |
| Traits | 35+ |
| Syscalls | 10 core |
| Design docs | 21 files (15,000+ LOC) |

---

## Build Configuration

**Target**: `riscv64gc-unknown-none-elf`  
**Edition**: 2021  
**Profile**: Release with LTO + size optimization  
**Nightly Features**: `asm`, `const_generics`, `ptr_to_from_bits`, etc.

---

## Module Pattern

All files follow **no mod.rs** rule:
- ✅ `foo.rs` parallel to `foo/` directory
- ❌ Never `foo/mod.rs`

Example: `kernel/src/memory.rs` + `kernel/src/memory/` folder

---

## Naming Conventions

| Category | Prefix | Examples |
|----------|--------|----------|
| Public Traits | `Vi` | ViFileSystem, ViDriver, ViBlockDevice |
| Error Types | `Vi` | ViError, ViResult |
| Addresses | `V`/`P` | VAddr, PAddr |
| Filesystems | `vi` | viFS1, viFS2 |
| Modules | snake_case | `memory.rs`, `task.rs` |

---

## Unsafe Code Policy

| Context | Allowed | Rule |
|---------|---------|------|
| Cells | ❌ NO | `#![forbid(unsafe_code)]` |
| Kernel/HAL | ✅ YES | Hardware I/O only, `// SAFETY:` required |

---

## See Also

- **Detailed Architecture**: `docs/ARCHITECTURE.md`
- **Coding Guide**: `docs/CODING_GUIDE.md`
- **API Reference**: `docs/API.md`
- **Patterns**: `docs/PATTERNS.md`
- **Specs**: `docs/0X-*.md` (numbered specifications)
