---
phase: 04
title: sys_spawn_from_elf + spawn_from_path ostd wrapper
tier: thinking
status: pending
depends_on: [02, 03]
law1_confirmation: required
origin: red-team F2/F3 reworked
---

# Phase 04 — `sys_spawn_from_elf` + `spawn_from_path` becomes an ostd wrapper

## Context links
- Plan: [plan.md](plan.md) · Scout: [scout-report.md](scout-report.md)
- Law 1: `libs/api/` ABI addition — **2× user confirmation before implementation.**
- Red-team F2 (fatal), F3 (serious), F5 (grant OK), M1 (grant ceiling).

## Overview
Move post-boot spawn off the kernel's disk reader **without breaking the six caller classes**. Add `sys_spawn_from_elf(grant, len, path_hint)`; the kernel runs the identical spawn gate over the supplied bytes. Then reshape the **ostd `spawn_from_path(path)` wrapper** so shell, supervisor, Hypha, Lua, and init all transparently get `vfs_read → grant → sys_spawn_from_elf` — call sites unchanged.

## Key insights (red-team-corrected)
- **F2:** `spawn_from_path` is NOT init-only. Callers: shell externals `executor.rs:967`, supervisor hotswap `hotswap.rs:167`, Hypha `hypha/core/src/main.rs:71-101` + `tools/spawn:56`, Lua `bindings_io.rs:68`, init `init/src/main.rs:120-201,270`. → **Do not restrict the syscall and migrate one caller.** Change the shared ostd wrapper they all call.
- **F5 (verified OK):** `GrantAlloc` returns `grant_id == phys base == identity-mapped SAS vaddr`, `owner=caller` (`syscall.rs:2679-2701`). Kernel owns the frames and reads them directly — cell→kernel direction is sound. Reuse `BlkReadAsync` grant discipline (`syscall.rs:2923`).
- **M1:** `MAX_GRANT_PAGES` = 16 MiB ceiling (`syscall.rs:2681-2683`), `GrantAlloc` returns `0` (OOM) above it. DOOM ELF ≈ 16 MiB sits at the edge. → spawn path must handle >16 MiB via chunked/multi-grant, or raise the ceiling for this path.
- **F3:** routing respawn through VFS is a never-die regression if VFS is down. → The wrapper routes **VIFS1-resident bootstrap/critical cells** through the VFS-independent `sys_spawn_from_path` (kept alive, bootstrap-only), and only **disk-resident** cells through the VFS path.

## Requirements
- **Functional:** all existing spawn call sites keep working unchanged. VIFS1 cells spawn VFS-independently; disk cells spawn via VFS→Block Cell. Caps/signing/allowlist identical to today.
- **ABI (Law 1):** one new syscall discriminant + ostd wrapper + kernel handler. No change to frozen signatures.

## Architecture
- **Kernel:** extract the spawn gate from `spawn_from_path` into `fn spawn_gated(bytes, path_hint, spawner)` (DRY — shared by both entry points: sig verify → manifest privilege → cap intersect → policy → allowlist → cluster → quota → measurement). New handler `SpawnFromElf { grant, len, args, path_hint } → tid` maps the grant, asserts `owner==caller`, calls `spawn_gated`. `path_hint` is advisory metadata for the `/bin/` privilege check + policy + measurement label — NOT a filesystem read. `sys_spawn_from_path` stays but its `read_file` is ramdisk/VIFS1-only (bootstrap).
- **ostd wrapper** `spawn_from_path(path)`: `if is_vifs1_bootstrap(path) { sys_spawn_from_path(path) }  else { let elf = vfs_read(path)?; let g = grant_from(&elf)?; sys_spawn_from_elf(g, elf.len(), path) }`. All six caller classes call this wrapper (verify none call the raw syscall directly; if any do, point them at the wrapper).
- **Large ELF:** `grant_from` chunks into ≤16 MiB grants or the handler accepts a grant list (M1).

## Related code files
- Modify (Law 1): `libs/api/src/syscall.rs` (discriminant), `libs/ostd/src/syscall.rs` (raw wrapper), `kernel/src/task/syscall.rs` (handler).
- Modify: `kernel/src/loader.rs` (extract `spawn_gated`; restrict kernel `spawn_from_path` to bootstrap), `libs/ostd/src/…` (the `spawn_from_path` wrapper + `is_vifs1_bootstrap`).
- Verify (no change expected): `cells/tools/shell/src/executor.rs:967`, `cells/services/supervisor/src/hotswap.rs:167`, `cells/apps/hypha/**`, `cells/runtimes/lua/src/bindings_io.rs:68`, `cells/tools/init/src/main.rs`.

## Implementation steps
1. **Confirm Law-1 ABI addition with user (2×).** Do not code until confirmed.
2. Extract `spawn_gated` (shared gate).
3. Add `SpawnFromElf` discriminant + ostd raw wrapper + kernel handler (grant-map → `spawn_gated`); handle multi-grant for >16 MiB.
4. Reshape the ostd `spawn_from_path` wrapper with the `is_vifs1_bootstrap` split (F3).
5. Restrict kernel `sys_spawn_from_path` to bootstrap paths (safe: wrapper only calls it for bootstrap).
6. Confirm all six caller classes route through the wrapper unchanged.
7. Tests: (a) parity — spawn a signed cell via elf vs path, identical decision; tampered → `PermissionDenied`; over-declaring user cell → denied. (b) disk-cell spawn: `/bin/<P2-cell>` runs. (c) never-die: kill VFS, confirm a VIFS1 Permanent service still respawns.

## Todo
- [ ] Law-1 confirmation (2×) recorded
- [ ] `spawn_gated` extracted + parity test
- [ ] `SpawnFromElf` ABI + wrapper + handler (+ >16 MiB)
- [ ] ostd `spawn_from_path` wrapper with VIFS1/disk split
- [ ] kernel `sys_spawn_from_path` bootstrap-only
- [ ] all six callers verified unchanged
- [ ] disk-cell spawn + never-die (VFS-down respawn) tests

## Success criteria
- **Runtime evidence:** boot log shows a disk-resident cell (e.g. a Hypha/tool cell) spawned via `/bin/…` through VFS→Block Cell; shell externals still run; tampered-cell denied; with VFS killed, a VIFS1 Permanent service still respawns. Suite green.

## Risk assessment
- *Gate divergence* — one shared `spawn_gated` + parity test.
- *A caller using the raw syscall not the wrapper* — audit in step 6; that caller would break, so it must be found.
- *Grant lifetime / >16 MiB* — free grant after segment copy; multi-grant path tested with DOOM-sized ELF.

## Security considerations
- Signature/manifest/cap gate is over the bytes → unchanged trust; init is root TCB. Change *reduces* kernel surface (loader no longer parses a filesystem for arbitrary paths). `path_hint` is advisory — the `/bin/` privilege gate keys off it, so a caller lying about `path_hint` can only *lose* privilege (non-`/bin/` hint ⇒ user ceiling), never gain it.

## Next steps
Phase 05 routes the remaining kernel block consumers (VFS fallback, snapshot) off `block::read_sector`.
