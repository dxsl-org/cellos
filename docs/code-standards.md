# Cellos Code Standards

**Scope**: Rust code across kernel, HAL, libraries, and Cells  
**Edition**: 2021  
**Nightly**: Required for `no_std` bare-metal features  
**Last Updated**: 2026-08-19

---

## The 8 Coding Laws (Non-Negotiable)

### Law 1: Interface is Sacred

- **Scope**: `libs/api/` and `libs/types/`
- **Rule**: Any changes require 2x explicit user confirmation
- **Reason**: These define the stable ABI between kernel and Cells
- **Implementation**:
  - Use `#[repr(C)]` on public structs/enums that cross the ABI boundary
  - Keep public trait method signatures additive and document wire/layout assumptions explicitly
  - Document trait contract in doc comments
  - Preserve method signatures when extending traits

### Law 2: Owned Buffers for Async (SAS Safety)

**Forbidden**:
```rust
async fn process(data: &mut [u8]) { }  // ❌ LIFETIME VIOLATION
```

**Required**:
```rust
async fn process(data: Box<[u8]>) -> Box<[u8]> { }  // ✅ OWNED
```

**Why**: Single Address Space (SAS) means no process boundaries for cleanup. Owned buffers ensure deterministic drop semantics across async boundaries.

**Pattern**:
- Input: `Box<[u8]>` or `Vec<u8>` (caller owns until call)
- Output: `Box<[u8]>` or `Vec<u8>` (callee owns return)
- Channels: `mpsc::Sender<Box<[u8]>>` for zero-copy IPC

### Law 3: Multi-Architecture Awareness

**Forbidden**:
```rust
let addr: u64 = 0xFFFF_FFFF_8000_0000;  // ❌ ASSUMES 64-BIT
```

**Required**:
```rust
let addr = VAddr(0x8000_0000);  // ✅ ARCH-AGNOSTIC
```

**Rules**:
- Never hardcode pointer sizes (`usize`, `u64`)
- Always use `VAddr` for virtual addresses, `PAddr` for physical
- Test on RV32, RV64, and ARM targets (compile checks at minimum)

### Hardware Platform Ownership

- Root `boards/` packages contain only identity/compatibles, boot/firmware
  contract, pinmux/PHY wiring, fallback memory/DT assets, typed SoC identity,
  and enabled shared-driver selection.
- Immutable SoC MMIO, IRQ topology, controller presence, and access quirks live
  under `hal/soc/`.
- Shared HAL↔kernel Rust ABI declarations live in `hal/traits/arch/src/kernel_abi.rs`.
  Architecture code must import that shared module instead of re-declaring
  local `extern "Rust"` blocks, and declaration sites should keep compile-time
  assertions close to the hook they validate.
- For x86 PC-compatible targets, `hal/soc/x86` owns static port/ISA wiring and
  bounded legacy firmware windows. LAPIC, IOAPIC, HPET, and ECAM addresses must
  come from validated ACPI and remain unavailable when firmware evidence fails.
- Register access, interrupt programming, and CPU-architecture mechanisms live
  under `hal/arch/`; shared device mechanisms remain single-copy in the kernel
  integration layer or `cells/drivers/`.
- Cargo board features select integration data. They must not fork UART, SDHCI,
  DesignWare I2C/SPI, GIC/PLIC, VirtIO, or PCIe mechanisms.
- Run `bash scripts/check-board-configs.sh` after changing a board, SoC profile,
  driver-selection boundary, or build feature.

### Law 4: Unsafe Code Management

**Ordinary Rust Cells**:
```rust
#![forbid(unsafe_code)]  // ABSOLUTE
```

Driver, runtime, shim, and FFI cells may carry reviewed unsafe only when listed
in the unsafe allowlist / signing policy checks. Do not describe Cellos as
"every Cell forbids unsafe" without that exception.

**Kernel & HAL**:
- Unsafe only for hardware I/O (CSRs, MMIO)
- **Every `unsafe` block must have a `// SAFETY:` comment** explaining:
  - Why safety invariants are maintained
  - What preconditions the caller must satisfy
  - What could go wrong if misused

**Example**:
```rust
// SAFETY: We assume mmu is initialized and this vaddr is mapped in current page table.
// CSR access is safe: no concurrent hart touches mepc during boot.
unsafe { riscv::register::mepc::write(func as usize); }
```

### Law 5: Modern Module Structure

**Forbidden**:
```
foo/
├── mod.rs      ❌ BANNED
└── bar.rs
```

**Required**:
```
foo.rs          ✅ REQUIRED (parallel file + folder)
foo/
├── bar.rs
└── baz.rs
```

**Rules**:
- Declare module in parent: `mod foo;`
- Module file: `foo.rs` (re-exports what's in `foo/` folder)
- Submodules: `foo/bar.rs`, `foo/baz.rs`
- Use snake_case: `file_system.rs`, not `FileSystem.rs`

**Why**: Clearer file tree, easier IDE navigation, prevents accidental circular imports.

### Law 6: Cellos Naming Convention

| Category | Rule | Examples |
|----------|------|----------|
| **Public Traits** | `Vi` prefix (Vi-something) | `ViFileSystem`, `ViDriver`, `ViBlockDevice`, `ViNetTcpStack` |
| **Error Types** | `Vi` prefix | `ViError`, `ViResult<T>` |
| **Core Structs** | `Vi` prefix (or generic) | `ViConfig`, `ViBenchmark` |
| **Address Types** | `V` or `P` prefix | `VAddr`, `PAddr` |
| **Filesystem Names** | retired `viFS1` / `viFS2`; active boot FS `VIFS1` | `VIFS1` (kernel BootFS/initramfs) |
| **Modules/Files** | snake_case | `task.rs`, `memory.rs`, `frame_allocator.rs` |
| **Functions** | snake_case | `init_paging`, `handle_interrupt` |
| **Constants** | UPPER_SNAKE | `MAX_CELLS`, `KERNEL_HEAP_SIZE` |
| **Type Params** | PascalCase | `T`, `E`, `CellState` |

### Law 7: Trait Objects for Polymorphism

**Pattern**:
```rust
pub fn register_driver(driver: Arc<dyn ViDriver + Send + Sync>) { }
```

**Rules**:
- Use `dyn Trait` at system boundaries (Cells, drivers, services)
- Always specify bounds: `Send + Sync` for multi-cell safety
- `Box<dyn T>` for single owner (Cell)
- `Arc<dyn T>` for shared resources (kernel registry)
- Implement `Drop` for cleanup (Law 8)

**Why**: Enables dynamic Cell loading without recompilation.

**Filesystem naming note**: use `VIFS1` only for the kernel BootFS/initramfs. Keep
`viFS1` / `viFS2` only in historical references to retired designs.

### Law 8: RAII - Implement Drop

**Rule**: All resources must implement `Drop` for explicit cleanup.

**Pattern**:
```rust
pub struct FileHandle { fd: u32 }

impl Drop for FileHandle {
    fn drop(&mut self) {
        // Close file, release resource
        syscall::close(self.fd).ok();
    }
}
```

**Why**: In SAS, there's no process cleanup. Resources don't auto-free when a task dies. You must manually manage.

**Resources Requiring Drop**:
- `FileHandle`, `DirHandle` — system resources
- `GrantEntry`, `Lease` — capability objects
- `Lock<T>` — mutual exclusion
- Custom allocations — via `alloc` crate

---

## Error Handling

### Result Pattern (Not Panic)

```rust
pub type ViResult<T> = Result<T, ViError>;
```

**Rule**: Use `Result<T, E>` everywhere except kernel invariants.

**ViError Variants**:
```rust
pub enum ViError {
    OutOfMemory,
    InvalidArgument,
    NotFound,
    PermissionDenied,
    AlreadyExists,
    WouldBlock,
    NotSupported,
    IO(String),
    InvalidInput,
    IsADirectory,
    NotADirectory,
    Unknown,
}
```

**Syscall Wrapper Example**:
```rust
pub fn open(path: &str, flags: u32) -> ViResult<FileHandle> {
    let fd = unsafe { syscall(SysCall::Open, path, flags)? };
    Ok(FileHandle { fd })
}
```

---

## Async & Concurrency

### Async Functions

```rust
pub async fn read_file(path: &str) -> ViResult<Vec<u8>> {
    let file = open(path, READ).await?;
    file.read_all().await
}
```

**Rules**:
- Use `async/await` syntax (not `Future` trait directly)
- Owned buffers: `Box<[u8]>`, never `&mut [u8]`
- Spawn tasks with kernel executor: `spawn_async(future)`

### Spinlocks for Synchronization

```rust
static REGISTRY: Spinlock<HashMap<CellId, Cell>> = Spinlock::new(HashMap::new());

fn register(id: CellId, cell: Cell) {
    let mut map = REGISTRY.lock();
    map.insert(id, cell);
}
```

**Why**: Spinlock handles interrupt safety automatically (disables on lock, re-enables on drop).

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_allocation() {
        let alloc = FrameAllocator::new();
        let frame = alloc.allocate().expect("should allocate");
        assert!(frame.0 > 0);
    }
}
```

**Rules**:
- Test critical logic (allocators, scheduler, IPC)
- Use `expect()` with clear messages, not `unwrap()`
- No integration tests in kernel (use architecture tests in `tests/`)

### Integration Tests

Located in `tests/architecture-validation/`:
```
tests/
├── step1_spec_verification.md
├── step2_dependency_analysis.md
└── (20+ checks)
```

**Run**:
```bash
cargo test --test '*' --release
```

### Definition of Done (runtime evidence, not checkboxes)

> Ratified 2026-07-06 after the functional audit (a "23/23 complete" claim
> re-scored to 12 done + 6 partial) and the boot.rs false-green incidents.

A feature, phase, or fix is **done** only when ALL of:

1. **Builds clean** on every architecture it targets (`cargo build --release`,
   the arch feature matrix in CI).
2. **Runs on QEMU with observable evidence**: a boot-log line, an integration
   test, or a shell interaction proving the behavior — captured in the plan or
   commit message. `cargo check` passing is NOT evidence of anything running.
3. **Its integration test is wired into CI** (the `boot-suite` job or a
   dedicated job). A test that only runs on one developer's machine rots —
   the main suite once rotted 4 days because it lived outside CI.
4. **Fails loud, never silent**: no silent-deny, silent-empty-reply, or
   silent-skip paths in the feature OR its tests. `prerequisites_ok()` must
   go through `ci_guard()`; degraded modes must log.
5. **Status text updated in the same commit** (roadmap/plan frontmatter/docs
   body) — stale "✅" markers poison every later planning session.

Plans record status as: 📋 planned → 🔨 code-complete (builds, unverified) →
✅ **verified** (runtime evidence linked). Never mark ✅ from 🔨 without a run.

---

## Comments & Documentation

### Doc Comments (Public Items)

```rust
/// Opens a file from the virtual filesystem.
///
/// # Arguments
/// * `path` - Absolute path (e.g., "/bin/hello")
/// * `flags` - Open flags (READ, WRITE, APPEND)
///
/// # Returns
/// A `FileHandle` or `ViError` if not found or permission denied.
///
/// # Example
/// ```
/// let handle = open("/bin/hello", READ)?;
/// let bytes = handle.read_all().await?;
/// ```
pub async fn open(path: &str, flags: u32) -> ViResult<FileHandle> { }
```

**Rules**:
- Document all public traits, functions, types
- Include # Arguments, # Returns, # Errors sections
- Add examples for complex logic
- Link to related specs: `See docs/specs/03-runtime.md for async safety rules.`

### Safety Comments

```rust
// SAFETY: We guarantee UART is initialized before this point.
// CSR access is atomic: no other hart modifies mepc during boot.
unsafe { riscv::register::mepc::write(func as usize); }
```

**Format**:
```
// SAFETY: [Why it's safe: preconditions, guarantees, no data races]
unsafe { ... }
```

### Inline Comments (Sparse)

Only when WHAT the code does is unclear:

```rust
// Bad:
x = x + 1;  // Increment x

// Good:
// Align heap pointer to next 4KB boundary (page size)
heap_ptr = (heap_ptr + 0xFFF) & !0xFFF;
```

---

## Code Organization

### Imports

```rust
// System imports (std, no_std)
use core::ptr;

// External crates
use spin::Spinlock;
use xmas_elf::ElfFile;

// This crate
use crate::memory::{VAddr, PAddr};
use crate::task::Task;

// Pub re-exports at module level
pub use crate::types::{ViError, ViResult};
```

**Order**: System → External → Internal → Re-exports.

### File Size & Directory Organization

- **Limit**: 200-300 LOC per file
- **Exceeding**: Split into submodules
- **Example**: `task.rs` (1000 LOC) → `task/scheduler.rs`, `task/syscall.rs`, `task/ipc.rs`

### Cells Directory Structure

Cellos organizes cells into 8 semantic groups (parallel to code, not functionality):

```
cells/
├─ tools/        — System utilities (shell, init, sys-tools, net-tools)
├─ apps/         — User applications (choose tier by execution boundary and runtime profile)
├─ demos/        — Demonstrations & graphical showcases (periph-demo, sensor-demo, doom, tetris*, audio-demo, etc.)
├─ drivers/      — Hardware device drivers (trusted native cells; gpio, i2c, spi, uart, etc.)
├─ services/     — System services (vfs, net, input, compositor, silo, hypervisor, etc.)
├─ runtimes/     — Trusted native runtime profiles (lua; MicroPython historical only)
├─ tests/        — Integration & stress test cells (bench, vfs-test, etc.)
└─ guests/       — Hypervisor guests (silo-guest, aarch64-unknown-none)
```

**Classification rules:**
- **tools/** — Always-running infrastructure (shell, init, system daemons)
- **apps/** — Interactive/rich user applications; use the tier/runtime profile docs to pick the execution boundary
- **demos/** — Showcases of system capabilities: hardware drivers, rendering, audio, scripting, games. Run on-demand from the shell; never auto-spawned at boot.
- **drivers/** — Hardware devices + driver Cells (trusted native cells, mapped via kernel Resource Registry or IPC)
- **services/** — Long-lived stateful services with IPC servers (VFS, net, input, compositor, broker-style cells). Cross-machine brokers must fail closed by default; readable configuration is neither authorization nor secret storage.
- **runtimes/** — Trusted native runtime profiles (Lua; MicroPython is historical only)
- **tests/** — Integration test & benchmark cells spawned by CI or manual runs (disposable, single-purpose)
- **guests/** — Hypervisor guest binaries (bare-metal or minimal OS images, non-x86/ARM64 targets)

### HAL / Board Ownership

- Root `boards/` stays outside HAL; it owns immutable board descriptors and fallback assets.
- `hal/soc/riscv` owns RISC-V SoC profile facts only: compatible-string sets and fail-closed access policies.
- `hal/soc/x86` owns PC-compatible COM/ISA wiring and legacy firmware windows;
  ACPI-discovered MMIO stays fail-closed in the kernel integration boundary.
- Shared drivers stay in `cells/drivers/`; do not copy UART, SDHCI, DW I2C/SPI, GIC/PLIC, or PCIe drivers per board.

### Visibility

```rust
// Kernel only
fn internal_fn() { }

// Public to cells (part of syscall ABI)
pub unsafe fn syscall_handler() { }

// Public trait (stable ABI)
#[repr(C)]
pub trait ViFileSystem {
    fn open(&self, path: &str) -> ViResult<Box<dyn ViFile>>;
}
```

---

## Build & Compilation

### Cargo Features

```toml
[features]
default = ["riscv64"]
riscv32 = []
riscv64 = []  # Primary target
arm64 = []
x86_64 = []
```

**Conditional Code**:
```rust
#[cfg(target_arch = "riscv64")]
pub fn init_paging() { /* SV39 */ }

#[cfg(target_arch = "arm")]
pub fn init_paging() { /* ARMv8 */ }
```

### Compiler Flags

```toml
[profile.release]
panic = "abort"        # No unwinding in kernel
lto = true             # Whole program optimization
opt-level = "z"        # Size + speed tradeoff
```

---

## Common Patterns

### Global State (Kernel)

```rust
static SCHEDULER: Spinlock<RoundRobin> = Spinlock::new(RoundRobin::new());

pub fn schedule() {
    let mut sched = SCHEDULER.lock();
    sched.next_task();
}
```

### Capability Object (Syscall)

```rust
pub struct Grant {
    capability: Capability,
    from: CellId,
    to: CellId,
}

impl Drop for Grant {
    fn drop(&mut self) {
        // Revoke capability on drop
    }
}
```

### Async Executor Task

```rust
pub async fn read_with_timeout(path: &str, timeout_ms: u64) -> ViResult<Vec<u8>> {
    select! {
        result = read_file(path) => result,
        _ = timer::sleep(timeout_ms) => Err(ViError::WouldBlock),
    }
}
```

### App Development (Cell Writing)

Use the Cellos App SDK (`libs/ostd/`) to eliminate boilerplate:

The App SDK is one family of named modules/layers, not a numbered tier system.
Application Tier 1/2/3 describes the execution/isolation boundary; runtime
profiles and SDK modules describe how code is built and which APIs it uses.

Manifest v2 uses a protection-class byte for the x86 PKU floor. New Rust code
should use `PROTECTION_CLASS_*`, `CellManifest::protection_class()`, and
`granted_protection_class()`; `tier`, `TIER_*`, `tier()`, and the existing
manifest macro forms remain compatibility surfaces. The Rust record remains
16 bytes, while Zig cells intentionally emit the legacy 8-byte v1 record.

**Before (manual dispatch)**:
```rust
#![no_std]
extern crate alloc;

use api::{declare_manifest, sys_recv, sys_send, MessageBuf};

declare_manifest!(spawn = true);

#[no_mangle]
pub extern "C" fn main() {
    let mut buf = MessageBuf::new();
    loop {
        if sys_recv(&mut buf, Some(100)).is_ok() {
            // Handle message...
            sys_send(buf.sender, &[0x00]).ok();
        }
    }
}
```

**After (app_entry! macro)**:
```rust
use api::{app_entry, CellRuntime, VfsClient};

app_entry!(handler = run);

async fn run() {
    let vfs = VfsClient::new();
    let data = vfs.read_file("/data/config.txt").await.ok();
    println!("Config loaded");
}
```

**Pattern summary**:
- Use `app_entry!` or `service_entry!` macros to declare entry point
- Access services via typed client facades (`VfsClient`, `NetClient`, `InputClient`)
- `CellRuntime` handles manifest generation, permission sets, lifecycle
- Apps declare minimal syscall set; kernel enforces via allowlist

---

## I/O Trait Layers (embedded-io Integration)

Cellos integrates [`embedded-io`](https://docs.rs/embedded-io) for byte-stream I/O. The two systems serve distinct purposes and must not be conflated:

### Which trait system to use

| Layer | Use | Avoid |
|---|---|---|
| **Stream I/O** (byte streams) | `embedded_io::Read + Write + Seek` | Custom `ViRead`/`ViWrite` |
| **Hardware peripherals** (GPIO, I2C, SPI, ADC, PWM) | `Vi*` HAL traits | `embedded_io` (no coverage) |
| **Async IPC wire format** | `Box<[u8]>` owned buffers (Law 2) | `embedded_io_async` at Cell boundary |
| **Intra-cell async I/O** | `embedded_io_async::Read + Write` | (safe — borrow stays on Cell stack) |

### Rules for App Cell developers

- **Only import from `ostd::*`** — never import `embedded_io` directly in app code.
- `ostd::fs::File`, `ostd::io::Stdin`/`Stdout`, and `ostd::clients::TcpStream` already implement `embedded_io::Read + Write`. Pass them directly to ecosystem crates that accept `impl embedded_io::Read`.
- `embedded_io` is re-exported as `ostd::embedded_io` if explicit trait bounds are needed.

### Rules for Driver Cell developers

- Hardware device cells implement `Vi*` HAL traits (`ViGpio`, `ViI2c`, `ViSpi`, `ViAdc`, `ViPwm`, `ViCan`).
- Byte-stream devices (UART/serial, TCP, file) additionally implement `embedded_io::Read + Write` via the `OstdError` newtype bridge in `ostd::io`.
- A driver may implement both a `Vi*` trait and `embedded_io` traits if appropriate.

### ostd stream handles

| Handle | Traits implemented | Backed by |
|---|---|---|
| `ostd::io::Stdin` | `Read` | `sys_read` |
| `ostd::io::Stdout` | `Write` | `sys_log` |
| `ostd::fs::File` | `Read`, `Write` | `sys_read_cap`, VFS IPC |
| `ostd::clients::TcpStream` | `Read`, `Write` | IPC → net service |

> **Note — File Seek:** `embedded_io::Seek` on `File` requires a `SeekCap` syscall (not yet implemented). Adding it is a Law 1 change — two confirmations required. Until then, use `ReadGrant` IPC for offset-based reads.

---

## Deprecations & Breaking Changes

When changing public API in `libs/api/`:

```rust
#[deprecated(since = "0.3.0", note = "use ViAsyncFileSystem instead")]
pub trait ViFileSystem {
    // old impl
}

pub trait ViAsyncFileSystem {
    // new impl
}
```

**Changelog Entry** (in `docs/project-changelog.md`):
```markdown
## [0.3.0] - 2026-06-15
### Deprecated
- `ViFileSystem::open()` → use `ViAsyncFileSystem::open().await` instead
```

---

## Quick Reference Card

| Rule | Status | Enforcement |
|------|--------|-------------|
| No mod.rs | ❌ FORBIDDEN | CI lint |
| Owned buffers in async | ❌ FORBIDDEN | Compiler error |
| Unsafe requires SAFETY comment | ❌ FORBIDDEN | Code review |
| Ordinary Rust Cells can't use unsafe | ❌ FORBIDDEN | `#![forbid(unsafe_code)]`; audited FFI/runtime/driver exceptions only |
| Vi prefix for public traits | ✅ REQUIRED | Code review |
| Result<T, E> over panic! | ✅ REQUIRED | Code review |
| Implement Drop | ✅ REQUIRED | Code review |
| 200-300 LOC per file | ✅ GUIDELINE | Code review |

---

## See Also

- **CLAUDE.md** — Quick agent reference
- **patterns.md** — Deep patterns & examples
- **system-architecture.md** — System design
- **api-reference.md** — Full trait reference
- Specs: **docs/specs/0X-*.md** — Feature specifications
