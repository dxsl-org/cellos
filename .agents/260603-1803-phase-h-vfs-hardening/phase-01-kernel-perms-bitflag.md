# Phase 1: KernelPerms bitflags (replaces `can_block_io: bool`)

## Context links
- `kernel/src/task/tcb.rs:131` — current `pub can_block_io: bool`
- `kernel/src/task/tcb.rs:157` — default `can_block_io: false` in `Task::new`
- `kernel/src/loader.rs:74-80` — grant site for `/bin/vfs`
- `kernel/src/task/syscall.rs:75-82` — `caller_has_block_io` helper
- `kernel/src/task/syscall.rs:1090,1117,1138` — three syscall arms gated by `caller_has_block_io`
- `libs/api/src/cap.rs:30-45` — `CapPerms(u32)` for FILE I/O — **DO NOT TOUCH (Law 1)**

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** Convert the single-purpose `can_block_io: bool` into a kernel-internal
  `KernelPerms(u32)` bitfield so future kernel-only capabilities (GPU flush, etc.) can be
  added without changing the TCB struct or the Law 1 ABI in `libs/api/`.

## Key insights
- `CapPerms` in `libs/api/src/cap.rs` is the **Cell-facing file-I/O capability** (READ/WRITE/SEEK).
  Adding `BLOCK_IO` there would change the stable ABI and trigger the Law 1 2x-confirm gate.
  `KernelPerms` lives entirely inside the kernel crate — no ABI impact, no confirmation needed.
- The bool is read in exactly one place (`caller_has_block_io`) and written in exactly one
  place (`loader.rs`). Compiler will flag any missed caller after the rename.
- `KernelPerms` must be `Copy + Clone + Default` so `Task::new` keeps its struct-literal form
  and the `.lock().as_ref().map(|t| ...)` read path stays unchanged.

## Requirements
**Functional**
- A task carries a `KernelPerms` bitfield; default = empty (no privileges).
- `/bin/vfs` is granted `KernelPerms::BLOCK_IO` at spawn (boot-order independent, as today).
- The three block-device syscall arms (500/501/503) reject callers lacking `BLOCK_IO`.

**Non-functional**
- No behavior change for any cell other than `/bin/vfs`.
- `KernelPerms` API is `const fn` where possible (compile-time bit ops).

## Architecture / data flow
```
spawn /bin/vfs ──► loader.rs grants task.kernel_perms |= BLOCK_IO
                                   │
syscall 500/501/503 ──► caller_has_block_io(caller_id)
                          └─► SCHEDULER.tasks[id].kernel_perms.contains(BLOCK_IO)
                                   └─► true → proceed | false → reject
```
The lock-ordering contract in `caller_has_block_io` (acquire SCHEDULER, drop before return,
no nested BLOCK_DEVICE lock) is preserved — only the field read changes.

## Related code files
**Modify**
- `kernel/src/task/tcb.rs` — add `KernelPerms` type; replace field + default
- `kernel/src/loader.rs` — grant via `.with(KernelPerms::BLOCK_IO)`
- `kernel/src/task/syscall.rs` — import + read via `.contains(...)`

**Create / Delete:** none.

## Implementation steps

### 1a. Add `KernelPerms` in `tcb.rs` (after the `CellId` import / before the `Task` struct)
```rust
/// Kernel-internal capability bitflags for a task.
/// Replaces the single-purpose `can_block_io: bool` (Phase G) with a bitfield
/// that accommodates future kernel-only capabilities without TCB struct changes.
/// Phase I+: add `KernelPerms::GPU` for GPU flush, etc.
/// NOTE: kernel-internal only — distinct from `libs/api` `CapPerms` (file I/O, Law 1).
#[derive(Copy, Clone, Default)]
pub struct KernelPerms(u32);

impl KernelPerms {
    /// Permits raw block-device syscalls (500/501/503). Granted to `/bin/vfs` at spawn.
    pub const BLOCK_IO: Self = Self(1 << 0);

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[inline]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
```

### 1b. Replace the field in the `Task` struct (tcb.rs:128-131)
Replace the doc comment + `pub can_block_io: bool,` with:
```rust
    /// Kernel capability bitfield. Granted at spawn (e.g. BLOCK_IO for `/bin/vfs`).
    /// Empty for every other cell. Replaces the Phase G `can_block_io: bool`.
    pub kernel_perms: KernelPerms,
```

### 1c. Update `Task::new` default (tcb.rs:157)
Replace `can_block_io: false,` with `kernel_perms: KernelPerms::default(),`.

### 1d. Update the grant in `loader.rs:77`
Replace `task.can_block_io = true;` with:
```rust
                task.kernel_perms = task.kernel_perms.with(KernelPerms::BLOCK_IO);
```
Add the import at the top of `loader.rs` (or use the full path inline):
`use crate::task::tcb::KernelPerms;`
Update the stale Phase H TODO comment at loader.rs:73 to reflect this is now done.

### 1e. Update `caller_has_block_io` in `syscall.rs:75-82`
Replace `.map(|t| t.can_block_io)` with `.map(|t| t.kernel_perms.contains(KernelPerms::BLOCK_IO))`.
Add `use crate::task::tcb::KernelPerms;` at the syscall.rs import site.
Update the doc comment at syscall.rs:69 and the Phase H TODO at syscall.rs:71 (now implemented).

### 1f. Compile
```
cargo check -p ViCell-kernel
```

## Callers enumerated (complete — 5 sites)
1. `tcb.rs:131` — field declaration (edit)
2. `tcb.rs:157` — `Task::new` default (edit)
3. `loader.rs:77` — grant write (edit)
4. `syscall.rs:80` — read inside `caller_has_block_io` (edit)
5. `syscall.rs:1090,1117,1138` — call `caller_has_block_io` (NO change; helper signature stable)

Grep used: `can_block_io|caller_has_block_io|kernel_perms` over `kernel/src` — no other matches.

## Todo
- [ ] 1a Add `KernelPerms` type in `tcb.rs`
- [ ] 1b Replace field in `Task` struct
- [ ] 1c Update `Task::new` default
- [ ] 1d Update grant in `loader.rs` + import + comment
- [ ] 1e Update `caller_has_block_io` + import + comments
- [ ] 1f `cargo check -p ViCell-kernel` clean

## Success criteria
- `cargo check -p ViCell-kernel` passes with zero warnings about unused/renamed fields.
- `grep -rn can_block_io kernel/src` returns nothing.
- Boot still shows `[vfs] FAT16 /data volume mounted` (proves `/bin/vfs` retains BLOCK_IO and
  successfully issued block-device syscalls during mount).

## Risk assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Missed caller after rename | Low×Med | Only 5 sites, all enumerated; compiler catches any miss |
| `Default` not derived → `Task::new` breaks | Low×Low | `#[derive(Default)]` on `KernelPerms` |
| Lock-ordering regression | Low×High | Field read replaces field read 1:1; helper body otherwise untouched |

## Security considerations
- Default is **empty** (deny-by-default) — a new cell gets no kernel privileges.
- BLOCK_IO is granted only by path match `ends_with("/bin/vfs")` at spawn, same trust model as Phase G.
- Bitfield is `u32` private — cells cannot construct or forge `KernelPerms` (no public ctor from raw bits).

## Next steps / dependencies
- Independent. Can run in parallel with Phase 2.
- Establishes the `KernelPerms` pattern reused by Phase I (GPU/net capabilities).

## Unresolved questions
- None. (Pattern, callers, and lock-ordering all verified.)
