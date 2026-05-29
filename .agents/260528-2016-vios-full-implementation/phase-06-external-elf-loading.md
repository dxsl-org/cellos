# Phase 06 — External ELF Loading from /bin/

**Effort:** 60h | **Priority:** P1 | **Status:** complete | **Blockers:** Phase 03, Phase 04

## Overview

Today, Cell binaries (init, config, vfs, shell) are embedded into the kernel ELF via `include_bytes!`. Goal: kernel reads cell ELFs from `/bin/` on disk and spawns them, including PIE relocation. This decouples cell development from kernel rebuilds and is mandatory for a real OS.

## Context Links

- `docs/01-core.md` — Cellular philosophy, linker contract
- `docs/02-memory.md` — SAS layout, registry-of-Cells
- `kernel/src/loader/elf.rs` — current ELF loader (embedded path)
- `kernel/src/loader/reloc.rs` — relocation engine (verify completeness)
- Phase 03 unblocks U-mode entry; Phase 04 unblocks disk reads

## Key Insights

- Cells are compiled as PIE; entry address is relative to load base. Kernel chooses load base from the per-cell registry (SAS-allocated VA range).
- Relevant RISC-V relocations: `R_RISCV_RELATIVE` (most common for PIE), `R_RISCV_64`, `R_RISCV_JUMP_SLOT` (lazy bind not used in cells — eager only).
- ELF segments to map: `LOAD` (PT_LOAD) only. Flags: PF_R / PF_W / PF_X → SV39 PTE R/W/X with **U bit set** (cells are U-mode).
- BSS: zero-initialize after copying file bytes (one `LOAD` may have `memsz > filesz`).
- TLS: cells use thread-local-init via `libs/ostd`; if a cell has `PT_TLS`, kernel must allocate TLS block and set `tp` register on entry.

## Requirements

**Functional**
- `ViSyscall::SpawnFromPath(path: &str) → Result<CellId, ViError>` syscall available
- `init` cell uses it to spawn `config`, `vfs`, `shell` from `/bin/`
- ELF loader handles: PT_LOAD segments, BSS, PIE relocations (R_RISCV_RELATIVE primarily)
- Failed spawn (missing file, malformed ELF) returns clean error, no kernel panic
- Kernel binary no longer needs to embed cell ELFs (drops `include_bytes!`)

**Non-functional**
- Spawn time per cell < 50ms in QEMU
- Loader memory overhead < 32KB per spawn
- All loader unsafe blocks annotated `// SAFETY:`

## Architecture

```
init cell                                  Kernel
  │                                          │
  ├─ ViSyscall::SpawnFromPath("/bin/vfs")───►│
  │                                          ▼
  │                                  loader::spawn_from_path
  │                                  ├─ vfs.open("/bin/vfs") via internal IPC
  │                                  │  (bootstrap path: while VFS Cell not up,
  │                                  │   loader reads directly from block device)
  │                                  ├─ Read full ELF into kernel buffer
  │                                  ├─ Parse: ehdr, phdrs
  │                                  ├─ Allocate VA range from registry
  │                                  ├─ For each PT_LOAD:
  │                                  │   - Allocate frames
  │                                  │   - Map at VA + base (U|R|W|X per phdr flags)
  │                                  │   - Copy filesz bytes
  │                                  │   - Zero (memsz - filesz)
  │                                  ├─ Walk .rela.dyn:
  │                                  │   - R_RISCV_RELATIVE: *(base+offset) = base + addend
  │                                  │   - other types: error
  │                                  ├─ Build TCB { sepc=entry+base, sp, satp, … }
  │                                  ├─ Register cell in registry
  │                                  └─ Enqueue task
  │ ◄────  CellId or ViError ───────────────│
```

Bootstrap order chicken-and-egg: kernel cannot use VFS Cell to load VFS Cell. Solution: kernel-internal "early loader" reads block 0 of a fixed FAT or TAR archive at a known LBA, parses it to find `/bin/vfs` ELF bytes, loads it. After VFS is up, subsequent spawns use the IPC path through VFS.

## Related Code Files

**Modify:**
- `kernel/src/task/syscall.rs` — add `ViSyscall::SpawnFromPath` variant + dispatcher
- `kernel/src/loader/elf.rs` — generalize to load from `&[u8]` (already may be the case) + handle PIE
- `kernel/src/loader/reloc.rs` — confirm R_RISCV_RELATIVE handled; add error for unsupported types
- `kernel/src/loader.rs` — top-level `spawn_from_path` orchestrator
- `kernel/src/cell/registry.rs` — confirm VA-range allocation API exists; add if not
- `kernel/src/task.rs` — `spawn_cell(elf_bytes, args, env)` builder <!-- Updated: Validation Session 1 - correct path -->
- `cells/apps/init/src/main.rs` — switch from embedded spawn to SpawnFromPath
- `kernel/src/main.rs` — remove `include_bytes!` of cell ELFs (keep only `init` embedded as kernel must spawn it before disk is available — OR embed only a stub init that calls SpawnFromPath for everything else)
- `gen_disk.ps1` — bake compiled cell ELFs into `/bin/` of the generated disk image

**Create:**
- `kernel/src/loader/early.rs` — boot-time loader reading disk before VFS comes up
- `tests/integration/spawn_from_path.rs` — boot, assert shell prompt appears, confirms shell was loaded from disk (e.g. via known build-id check)
- `docs/elf-loader-contract.md` — short doc: what relocations supported, what's not, error model

## Implementation Steps

1. **Audit existing loader**:
   - Read `kernel/src/loader/elf.rs` end-to-end; note current entry signature
   - Read `kernel/src/loader/reloc.rs`; list supported relocation types
   - If R_RISCV_RELATIVE absent, add it as first new case
2. **Define syscall variant** in `libs/api/src/syscall.rs`:
   ```rust
   pub enum ViSyscall<'a> {
       …
       SpawnFromPath { path: &'a str, args: &'a [&'a str] },
       …
   }
   ```
   ABI: arg0 = path ptr, arg1 = path len, arg2 = args ptr, arg3 = args len.
3. **Implement kernel dispatcher** in `kernel/src/task/syscall.rs`:
   - Validate path (`copy_from_user`, len bounds, UTF-8)
   - Route to `loader::spawn_from_path`
4. **Implement early loader** `kernel/src/loader/early.rs`:
   - Reads block 0..N as a known archive (decision: TAR for simplicity, or RedoxFS if Phase 13 schedule allows)
   - Provides `fn read_file(path: &str) -> Result<Box<[u8]>, …>`
   - Used during boot to load VFS Cell ELF
5. **Implement `spawn_from_path` orchestrator** `kernel/src/loader.rs`:
   - If VFS Cell registered → call into it via IPC `OpenFile(path)` + `Read(handle)`
   - Else → call `early::read_file(path)`
   - Pass bytes to `elf::load_pie(bytes, base_va) → (entry, segments)`
   - Build TCB; register in cell registry; enqueue
6. **Update `gen_disk.ps1`**:
   - After cells build, copy `target/.../release/{init,config,vfs,shell,…}` into staging dir
   - Build a small TAR (or filesystem image) containing `/bin/*`
   - Write to disk image at LBA 0
7. **Modify `init` cell** (`cells/apps/init/src/main.rs`):
   - Replace any `spawn_embedded(CONFIG_ELF)` with `ViSyscall::SpawnFromPath("/bin/config")`
   - Repeat for vfs, shell
   - Sequential spawn with error logging
8. **Remove embedded ELFs**:
   - In `kernel/src/main.rs`, remove `include_bytes!("..config.elf")` etc.
   - Keep ONLY `init` embedded (since it must spawn before any disk is loaded — alternatively embed a tiny bootstrap init that just calls SpawnFromPath for the real init; either approach is fine, pick whichever yields smaller kernel)
9. **Boot test**:
   - Boot QEMU; expect log lines: `[init] spawning /bin/config`, `[loader] mapped 0x… len 0x…`, `[init] spawning /bin/vfs`, `[init] spawning /bin/shell`, shell prompt visible
10. **Error path test**:
   - In a debug build path, attempt to spawn `/bin/nonexistent`; expect `ViError::NotFound`, kernel logs but does not panic
   - Attempt to spawn a malformed ELF (truncated); expect `ViError::InvalidElf`

## Todo List

- [x] Audit elf.rs and reloc.rs (catalog supported relocations)
- [x] Add R_RISCV_RELATIVE to reloc.rs if missing
- [x] Add SpawnFromPath variant to libs/api/syscall.rs
- [x] Wire kernel dispatcher for SpawnFromPath
- [x] Implement loader/early.rs (boot-time disk read)
- [x] Implement spawn_from_path orchestrator
- [x] Update gen_disk.ps1 to bake /bin/
- [x] Modify cells/apps/init to use SpawnFromPath
- [x] Remove embedded ELFs from kernel binary
- [ ] Boot test → shell prompt from disk-loaded shell (QEMU test pending)
- [ ] Error path tests (not-found, malformed) (QEMU test pending)
- [ ] CI integration test `tests/integration/spawn_from_path.rs` (QEMU test pending)
- [ ] Document loader contract in `docs/elf-loader-contract.md` (deferred)

## Success Criteria

- Kernel binary size reduced (embedded ELFs removed) — measurable via `ls -la target/.../kernel` before/after
- `init` spawns config, vfs, shell from `/bin/` on every boot
- Spawn time per cell < 50ms (log timestamps)
- Error path: missing file returns clean error without kernel panic
- Integration test `tests/integration/spawn_from_path.rs` green

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Early loader needs a filesystem before Phase 13 lands | High | Med | Use TAR archive at fixed LBA (single-block-read concatenated layout); migrate to RedoxFS in Phase 13 |
| PIE relocation bug crashes spawned cell | Med | High | Test in debug build with `objdump -R` matching the loader's relocation table walk |
| TLS not supported initially, some cells break | Med | Med | Make TLS optional: if cell has no PT_TLS, skip; document in elf-loader-contract.md |
| Disk image layout mismatch between gen_disk.ps1 and kernel early loader | Med | High | Encode the TAR layout in a shared constant module `kernel/src/loader/disk_layout.rs`; both kernel and gen script consume same constants |
| init cell now strictly depends on disk → fails on RAM-only QEMU runs | Low | Med | Keep `-Dvios.disk=ram` env that falls back to embedded ELFs (debug builds only) |

## Security Considerations

- All path inputs from U-mode validated: length ≤ 256, no NUL, must start with `/bin/` (whitelist)
- ELF parser is a known attack surface; bound all length fields against file size before dereference
- Kernel zeroes new frames before mapping (prevent info leak from previous owner)
- Cap depth of relocations (≤ 65536 per cell) to bound parsing time

## Rollback

Revert spawns Cells from embedded ELFs as before. Disk-based loading is opt-in via the syscall; reverting the init cell's calls and re-embedding ELFs restores prior behavior in one PR.

## Next Steps

Unblocks Phase 07 (FileHandle IPC), Phase 13 (full VFS), Phase 17 (shell can spawn arbitrary binaries from /bin/), Phase 20 (hot migration loads new ELF from disk).
