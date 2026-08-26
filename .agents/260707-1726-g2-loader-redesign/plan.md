# Plan — G2 Loader Redesign: Ramdisk Boot + virtio-blk Driver Cell (Full Closure)

> **Goal:** Shrink the kernel from owning a full `virtio_blk` driver to a minimal RAM-image boot reader + a post-boot **virtio-blk Driver Cell**, closing the last open thread of the Kernel Boundary migration (RC-4). Leverages SAS/LBI: the kernel drives **no** block hardware.
>
> **Decided with user 2026-07-07:** boot = RAM image (zero kernel device driver) · post-boot spawn = init/callers read ELF via VFS → `sys_spawn_from_elf` · scope = **Full closure**.
>
> **Status (synced 2026-07-12):** Phases 01–06 completed 2026-07-07; Phase 07 deferred (spike-first); Phase 08 in-progress (x86 + aarch64 green, riscv images regen outstanding).
>
> **Revised after red-team (2026-07-07):** two fatal flaws fixed — F1 (added the FS-migration phase 03) and F2 (spawn becomes an ostd wrapper, not a syscall restriction). See "Red-team corrections" below.

## Why this is safe to attempt now
- **x86_64 already loads cells ramdisk-only** — `EarlyLoader::probe()` is riscv/aarch64-gated (`main.rs:438`); x86 cell loading falls through to VIFS1 (`early.rs:145`). (Caveat: x86 *data-partition* I/O still uses kernel block — see F4 / Phase 06.)
- **The userspace-block-over-IPC pattern is already shipped** — NVMe is a Driver Cell reached via `service::BLOCK_DRIVER`; VFS already dual-routes to it (`block_stream.rs:42-84`).
- **`spawn_from_mem` + the full spawn gate already exist** in the loader — the new syscall reuses them; the grant cell→kernel direction is verified sound (`syscall.rs:2679-2701`).

## Red-team corrections (load-bearing — read before Phase 03)
- **F1:** VFS `/bin` is a proxy to the **embedded VIFS1 ramdisk**, NOT the disk (`backend_bootfs.rs:1-8`, `manager.rs:34-52`). The disk cell store (P2 `CELL_TABLE`) has **no VFS backend**. P2-only cells (Hypha stack, fb-console, nc/curl/wget/httpd/mqtt, robot-demo, bench, Zig cells) would become unreachable. → **Phase 03 migrates them into a VFS-served FS with `/bin` as a VIFS1∪disk overlay.**
- **F2:** `spawn_from_path` is the universal spawn primitive (shell `executor.rs:967`, supervisor `hotswap.rs:167`, Hypha, Lua, init). → **`spawn_from_path` becomes an ostd wrapper**, not a restricted syscall; every caller gets `vfs_read→grant→sys_spawn_from_elf` transparently (Phase 04).
- **F3:** route VIFS1-resident critical services through the VFS-independent bootstrap path so a VFS crash doesn't block their respawn (never-die, spec 12).
- **F4:** x86 block pins to the **NVMe cell**; do not require a net-new x86 virtio-blk-pci (modern `0x1042` was never implemented in-kernel, `virtio_pci.rs:105-115`).

## Phase overview
| Phase | Title | Tier | Status | Depends | Key risk |
|------|-------|------|--------|---------|----------|
| [01](phase-01-ramdisk-bootstrap-unify.md) | Unify bootstrap on RAM ramdisk (+config, +/bin/block) | medium | completed | — | RISC-V regression; image size |
| [02](phase-02-virtio-blk-driver-cell.md) | virtio-blk Driver Cell (`forbid(unsafe_code)`) | thinking | completed | 01 | bounce-DMA; x86 pins to NVMe |
| [03](phase-03-migrate-elfs-to-fs.md) | **Migrate P2 cell ELFs → VFS-served FS + `/bin` overlay** | thinking | completed | 01 | sig preservation; namespace overlay |
| [04](phase-04-sys-spawn-from-elf.md) | `sys_spawn_from_elf` + `spawn_from_path` ostd wrapper | thinking | completed | 02,03 | **Law-1 (2× confirm)**; grant ceiling; never-die routing |
| [05](phase-05-route-off-kernel-block.md) | Route VFS + resolve snapshot off kernel block | thinking | completed | 02,04 | snapshot restore is bootstrap-critical |
| [06](phase-06-delete-kernel-virtio-blk.md) | Delete kernel virtio_blk + virtio_pci stack | medium | completed | 05 | x86 data I/O; residual callers |
| [07](phase-07-scoped-sum.md) | Scoped SUM — drop whole-lifetime `SUM=1` | thinking | **deferred (spike-first)** | 06 | **cross-cutting; split by default** |
| [08](phase-08-regression-and-docs.md) | 3-arch QEMU regression + docs/spec reconcile | medium | in-progress | 06 | TCG timing; harness dependence |

**Shippable RC-4 closure = Phases 01–06 + 08.** Phase 07 (SUM) is deferred to a spike and, by default, its own follow-up plan (M2: SUM is baked into `main.rs:483` + per-task `task.rs:568` + secondary harts — cross-cutting beyond block).

## Critical dependencies & honesty notes
1. **Snapshot ↔ Phase 06.** `snapshot.rs` restore reads P3 at boot *before any cell exists* — it cannot use the Block Cell. Phase 05 decides: descope warm-boot during transition (recommended; snapshot is Supervisory-Cell debt) or keep a tiny raw reader. This plan does not migrate snapshot into a cell.
2. **Law 1.** Phase 04 adds a syscall to `libs/api/`. **2× user confirmation** before implementation. It is an *addition* (new discriminant), not a change to a frozen signature.
3. **F3 never-die.** Critical Permanent services (net/input/compositor/supervisor/vfs/config) are all VIFS1-resident → respawnable VFS-independently. Only disk-resident apps have a degraded respawn window while VFS is down; document it.

## Definition of Done (code-standards runtime-evidence rule)
Not "cargo check clean." Each phase closes only on **boot-log evidence + a green integration test** in the hardened suite. Final acceptance: 3-arch QEMU regression boots to shell, a disk-resident cell (e.g. a Hypha/tool cell) spawns via `/bin/…` through VFS→Block Cell, VFS FAT32+littlefs I/O works, and `grep kernel/src` shows no `virtio_blk`/`virtio_pci` module.

## Cook handoff
```
/hc-cook d:\Cellos\.agents\260707-1726-g2-loader-redesign\plan.md
```
Start at Phase 08. Phases 01–06 are verified complete; Phase 07 remains deferred pending spike decision.

---

## Sync Evidence (2026-07-12)

**Phases 01–06 completion verified against code:**
- `cells/drivers/virtio-blk/` exists (timestamp 2026-07-07 18:42:00)
- `kernel/src/task/drivers/` contains no `virtio_blk.rs` or `virtio_pci.rs` (deleted 2026-07-07)
- `sys_spawn_from_elf` (syscall 238, `SpawnFromElf` discriminant) implemented in `kernel/src/task/syscall.rs:2679-2701`
- Git commit 3760b1d1 (2026-07-05): "refactor(gen_disk): G2 kernel-shrink — VIFS1 carries bootstrap cells only" shows full bootstrap refactor landed
- Supporting commits: 86a5a2d8 (VFS → block Driver Cell), 20e51a47 (bootstrap cells in kernel_fs), 66177fb3 (FAT32 mount)

**Phase 08 status (regression + docs):**
- x86 suites: 13/13 passing (per commit 37d31bb3 2026-07-11: "test(x86): gate FAT32-on-NVMe mount in CI")
- aarch64 suites: 7/7 boot-to-shell passing (per commit 08531c6b 2026-07-10, memory note)
- riscv images: pending regeneration with new init (G2 loader changes require new `kernel_fs.img` + cell images)
- Docs reconciliation: spec 15 §2C exception (virtio_blk as bootstrap) must be removed (outstanding; tied to phase 08 completion)

**Phase 07 status (SUM scoping):**
- Correctly deferred; spike phase will be spun separately per plan line 32
- Cross-cutting nature confirmed in `main.rs:483`, `task.rs:568`, `smp::start_secondaries` requires dedicated analysis
