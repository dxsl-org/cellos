---
phase: 01
title: Unify bootstrap on the RAM ramdisk (RISC-V onto the x86 model)
tier: medium
status: pending
depends_on: []
---

# Phase 01 — Unify bootstrap on the RAM ramdisk

## Context links
- Plan: [plan.md](plan.md) · Scout: [scout-report.md](scout-report.md)
- Spec: `docs/specs/15-kernel-boundary.md §2` (bootstrap root-of-trust)

## Overview
**Priority:** first (unblocks everything). **Status:** pending.
Make RISC-V boot its bootstrap cells from the embedded VIFS1 ramdisk exactly as x86_64 already does, so the boot path no longer depends on the `virtio_blk` cell-bootstrap table. After this phase the kernel can load `{init, platform, block-driver, vfs}` with **no block device touched**.

## Key insights
- x86_64 already runs this model: `ramdisk::init_driver()` + VIFS1 fallback (`main.rs:387-398,445`, `early.rs:145`). RISC-V currently prefers the on-disk `CELL_TABLE` (`early.rs:52-131`) and only falls back to VIFS1.
- `init` is already embedded via `include_bytes!(INIT_ELF)` (`main.rs:69`) — precedent for baking trusted-core cells into the image.
- Bootstrap cells are trusted-core / TCB; embedding them (rare change) is acceptable KISS. Non-bootstrap cells stay on the real disk. Bootloader-provided separate ramdisk module is a **future** refinement, explicitly out of scope.

## Requirements
- **Functional:** RISC-V boots to shell with the `virtio_blk` cell-table path disabled; all bootstrap cell ELFs resolve from VIFS1. x86_64 + aarch64 unchanged.
- **Non-functional:** kernel_fs.img growth stays bounded — only the bootstrap set is added, not every cell. Log the image size at build (`kernel/build.rs`).

## Architecture
- Define the canonical **bootstrap cell set** as a single list (const in `loader/early.rs` or `disk_layout.rs`): `["/bin/platform", "/bin/block", "/bin/vfs", "/bin/config"]` (+ `init` already embedded). `gen_disk.ps1` packs exactly these into `kernel_fs.img`.
  - **M3 (red-team):** `config` MUST be included — init spawns it at index 1, before VFS serves anything (`cells/tools/init/src/main.rs:61-71`). The new `/bin/block` cell (Phase 02) MUST be embedded in `kernel_fs.img` (VIFS1), not only the P2 table — it is bootstrap.
  - **fb-console note:** init spawns `/bin/fb-console` early (`init/src/main.rs:177`) and it is currently P2-only. Decide here: promote fb-console to VIFS1 (bootstrap) OR reorder its spawn to after Phase 03's disk FS is served. Recommended: VIFS1 (early console output must not depend on the disk).
- `EarlyLoader::read_file` for a bootstrap path reads VIFS1 **first** (invert the current block-first order for these paths); the disk `CELL_TABLE` path becomes legacy/unused for bootstrap and is removed in Phase 05.
- No change yet to non-bootstrap spawn (still `spawn_from_path`); that moves in Phase 03.

## Related code files
- Modify: `kernel/src/loader/early.rs` (bootstrap-path resolution order), `kernel/src/main.rs` (boot order — probe VIFS1 before/without virtio table on RISC-V), `gen_disk.ps1` (pack bootstrap set into kernel_fs.img), `kernel/build.rs` (log image size).
- Read-only ref: `kernel/src/task/drivers/ramdisk.rs`, `kernel/src/fs.rs`.

## Implementation steps
1. Add `BOOTSTRAP_CELLS` const + a helper `is_bootstrap_path(path)`.
2. In `EarlyLoader::read_file`, for bootstrap paths try VIFS1 first; keep the block-table branch only for non-bootstrap (temporary until Phase 03/05).
3. Update `gen_disk.ps1` to ensure every `BOOTSTRAP_CELLS` entry is present in kernel_fs.img (fail-fast if missing — reuse `Assert-BuildOk`).
4. On RISC-V, verify `EarlyLoader::probe()` is not required for bootstrap (guard so a missing/blank cell table no longer blocks boot).
5. Boot 3-arch; confirm bootstrap cells load from VIFS1 (log line).

## Todo
- [ ] `BOOTSTRAP_CELLS` + `is_bootstrap_path`
- [ ] VIFS1-first resolution for bootstrap paths
- [ ] gen_disk.ps1 packs + asserts bootstrap set
- [ ] RISC-V boots with cell-table path unused
- [ ] kernel_fs.img size logged & bounded

## Success criteria
- **Runtime evidence:** RISC-V boot log shows each bootstrap cell resolved via VIFS1; boots to shell. x86_64/aarch64 boot logs unchanged. Integration test `boots_to_shell_prompt` green on all three arches.

## Risk assessment
- *RISC-V regression* — mitigate by keeping the block-table branch alive for non-bootstrap until Phase 05; revertable in isolation.
- *Image bloat* — only 3 cells added; log size, alert if >2× prior.

## Security considerations
- Bootstrap cells remain signature-gated at load (the gate in `loader.rs` is unchanged). Embedding in `.rodata` does not weaken the manifest/cap gate.

## Next steps
Phase 02 (the Block Cell) can begin in parallel design but integrates after 01 boots clean.
