---
phase: 02
title: Loader Dynamic Symbol Resolution (Global Symbol Table) + Kernel-Owned fast-IPC
priority: P0
status: in-progress
depends_on: ["01"]
risk: high
approach: A (chosen 2026-06-06) — loader resolves cell→kernel symbol imports
---

# Phase 02 — Loader Dynamic Symbol Resolution + Kernel-Owned fast-IPC

## Decision (2026-06-06)
Approach **A** chosen: implement the spec's "Global Symbol Table" — the loader resolves a cell
ELF's **undefined symbols** against a **kernel export table**, so cells can call kernel-resident
functions directly (not only via `ecall`). This fixes blocker #3 properly AND unblocks the fast-IPC
sharing flaw (one kernel-owned dispatch pointer all cells resolve to).

> Scope expansion acknowledged: this is a real feature (~Phase 27 size done right), larger than a
> build fix. Chosen deliberately over the minimal stopgap. Verification requires a green build+boot.

## Root cause (verified)
- Kernel `extern`s `vi_set/clear_fast_ipc_vfs_cell`; they're defined in `libs/ostd` (heap/panic
  lang-items → kernel can't link ostd). → undefined symbol at kernel link.
- `libs/ostd` `VFS_HANDLER_PTR` is a plain `static` linked into *each* ELF separately → no shared
  instance → fast-IPC never works cross-cell; kernel clear-on-fault can't reach cells' pointer.
- Loader [reloc.rs:75-104](../../kernel/src/loader/reloc.rs) only applies `R_RISCV_RELATIVE` and
  `R_RISCV_64`(sym=0); it **rejects** symbol-bearing relocations. No cross-ELF symbol import exists.

## Design

### 1. Kernel-owned fast-IPC + export table
- New `kernel/src/fast_ipc.rs`: the canonical `VFS_HANDLER_PTR`/`VFS_HANDLER_CELL` statics +
  `register_vfs`, `set_vfs_handler_cell`, `clear_vfs_if_cell`, `call_vfs` dispatch — all
  `#[no_mangle] pub extern "Rust"`. Kernel owns the single instance.
- `vi_set/clear_fast_ipc_vfs_cell` become thin kernel-defined wrappers (resolves the link error
  in loader.rs/task.rs immediately — these are kernel→kernel calls now).
- A kernel **export table**: `static KERNEL_EXPORTS: &[(&str, usize)]` mapping exported symbol
  names → addresses (`fn as usize`). Hand-maintained, KISS — avoids parsing the kernel's own
  symbol table. Initial entries: `register_vfs`, `call_vfs` (the symbols cells import).

### 2. Loader dynamic symbol resolution
Extend the loader (`reloc.rs` + a new `dynsym.rs`) to, for each spawned cell:
- Read `.dynsym` + `.dynstr` (via `get_section`); build index→(name, is_undefined).
- For each relocation in `.rela.dyn`/`.rela.plt` with a non-zero symbol index:
  - Resolve the symbol name in `KERNEL_EXPORTS`. Found → patch `*offset = addr (+ addend)`.
  - Handle `R_RISCV_JUMP_SLOT` (PLT/GOT entry) and `R_RISCV_64`(sym≠0). Unknown name → error
    (fail the spawn loudly; do not silently leave it unresolved).
- Keep existing `R_RISCV_RELATIVE`/`R_RISCV_64`(sym=0) behavior unchanged.

### 3. Cell build emits the imports
- ostd `fast_ipc.rs`: replace the local statics with `extern "Rust" { fn register_vfs(...); fn call_vfs(...); }`
  declarations (kernel-resident). Cells calling these emit undefined dynamic symbols + relocations.
- Cell linker scripts must keep `.dynsym`/`.dynstr` and allow undefined symbols at cell-link
  (`-pie` already; add `--unresolved-symbols=ignore-all` / `-z undefs` if the linker drops them).
  Verify the toolchain emits `R_RISCV_JUMP_SLOT`/`R_RISCV_64` for the extern calls.

### 4. Preserve semantics
- VFS cell calls `register_vfs` (now kernel-resident) at startup → kernel's single pointer is set.
- Client cells `call_vfs` → reads the kernel's pointer (shared). Fast path works cross-cell.
- Kernel fault path `clear_vfs_if_cell` nulls the same pointer → clear-on-fault actually protects
  clients (closes the stale-pointer hole; ties into Reliability H5).

## Implementation Steps (incremental, build-checked each)
1. Create `kernel/src/fast_ipc.rs` (statics + functions + `vi_set/clear` wrappers + KERNEL_EXPORTS);
   `pub mod fast_ipc;` in main.rs. → kernel links (blocker #3 cleared for the kernel itself).
2. `cargo build --release -p vicell-kernel` → expect **0 errors** (kernel self-contained now).
3. Add loader dynsym resolution (`dynsym.rs` + extend `reloc.rs`); wire into `spawn_from_path`.
4. Update ostd `fast_ipc.rs` to `extern` the kernel symbols; rebuild ostd + cells.
5. Build cells, regenerate disk image; boot; verify VFS registers + a client `cat` resolves.
6. Verify clear-on-fault: kill VFS → client `call_vfs` falls back (pointer nulled).

## Todo List
- [ ] kernel/src/fast_ipc.rs (kernel-owned statics + funcs + vi_set/clear wrappers)
- [ ] KERNEL_EXPORTS table
- [ ] `cargo build --release -p vicell-kernel` = 0 errors
- [ ] dynsym.rs: parse .dynsym/.dynstr, resolve undefined syms vs KERNEL_EXPORTS
- [ ] reloc.rs: handle R_RISCV_JUMP_SLOT + R_RISCV_64(sym≠0) via resolver
- [ ] ostd fast_ipc.rs → extern kernel symbols; rebuild cells
- [ ] Cell linker: keep .dynsym, allow undefined; verify reloc types emitted
- [ ] Boot: VFS register + client call_vfs resolves; clear-on-fault nulls pointer

## Success Criteria
- `cargo build --release -p vicell-kernel` = 0 errors (after step 1).
- Cells with kernel-symbol imports load without `NotSupported`; boot to shell.
- A client cell's `call_vfs` reaches the VFS-registered handler (fast path), and after killing
  VFS the pointer is nulled (client falls back to ecall).

## Risk Assessment
- **Toolchain reloc reality (High).** The exact reloc types emitted for an extern-fn call under
  `-pie` may differ (R_RISCV_CALL_PLT vs JUMP_SLOT vs GOT). *Mitigation:* inspect a built cell's
  `.rela.*` with `llvm-readobj -r` before finalizing the resolver; handle the actual types.
- **Cell link drops undefined symbols (High).** Linker may error on undefined refs at cell-link.
  *Mitigation:* `--unresolved-symbols=ignore-all` / dynamic; confirm `.dynsym` retains them.
- **KASLR slide (Med).** Export addresses are post-slide kernel addresses; with slide=0 (direct
  `-kernel` boot) addresses are link-time. Resolve against the kernel's *runtime* addresses.
- **Security (Med).** Cells can now call kernel-exported functions directly — only export
  intentionally-safe symbols; this is the same trust model as fast-IPC's TrustedHandle.

## Next Steps
- Phase 03 build-verification gate ensures this can't silently regress.
