# Scout Report — G2 Loader Redesign (block/boot coupling)

> Codebase analysis captured at planning time (2026-07-07). Read by `/hc-cook`, `/hc-review`, `/hc-debug` to skip re-scouting within this plan.

## The coupling, in one paragraph
The kernel's block dependency reduces to `kernel/src/task/drivers/block.rs` (39 LOC dispatch) → `virtio_blk.rs` (217) + `virtio_pci.rs` (225, x86 transport, block-only now) + shared `virtio_hal.rs` (74) / `virtio_common.rs` (44). Boot-critical block reads are only three sites: `EarlyLoader::probe/read_file`, `disk_layout::verify_mbr`, and `snapshot.rs`. Userspace reaches block **only** via syscalls 500/501/503 (BlkRead/Write/Flush) + 212 (BlkReadAsync, grant-based) — all funnel through `block::read_sector`. A ramdisk boot path already fully works on x86_64.

## Key files & line anchors
| Concern | File:line | Note |
|---|---|---|
| Boot cell loader | `kernel/src/loader/early.rs:41-146` | `probe()` reads cell table via `block::read_sector`; `read_file()` falls back to VIFS1 ramdisk at **:145** |
| Block dispatch | `kernel/src/task/drivers/block.rs:15-38` | `block_device()` VirtIO→MMC; NVMe already a Cell (:11-14) |
| virtio-blk driver | `kernel/src/task/drivers/virtio_blk.rs` | 217 LOC, block-only; `viVirtIOBlk: ViBlockDevice` (:125-217), `vi_handle_virtio_irq` (:82) |
| virtio-pci transport | `kernel/src/task/drivers/virtio_pci.rs` | 225 LOC, **BLK-only** now (net removed); `init()` :69 |
| Shared HAL/common | `virtio_hal.rs` (74), `virtio_common.rs` (44) | **also used by `virtio_rng`** — do NOT delete |
| Embedded ramdisk | `kernel/src/task/drivers/ramdisk.rs:10` | `ViRamDisk` = `include_bytes!(kernel_fs.img)`, RO FAT16 in .rodata |
| VIFS1 FAT driver | `kernel/src/fs.rs:11`, `fs/fat.rs:11,21` | reads `ViRamDisk` directly (own device, NOT via `block::`) |
| Boot spawn order | `kernel/src/main.rs:386-571` | drivers::init(386)→virtio_pci(414)→snapshot::try_restore(424)→verify_mbr(434)→EarlyLoader::probe(439)→fs::init(447)→spawn platform(527)→spawn init from `include_bytes!` (69,537); init spawns the rest via syscall (546-550) |
| VFS block path | `cells/services/vfs/src/block_stream.rs:42-84` | dual-route: `service::BLOCK_DRIVER` cell (IPC) else `sys_blk_read` fallback |
| Kernel blk syscalls | `kernel/src/task/syscall.rs:2555,2586,2616,2923` | BlkFlush/Read/Write/ReadAsync; gated by `caller_has_block_io()` + `check_block_access()` |
| fast-IPC VFS handler | `kernel/src/fast_ipc.rs:39,47,53`; wired `loader.rs:302-311` | `set_vfs_handler_cell` keyed off granted `block_io` bit |
| Snapshot block use | `kernel/src/snapshot.rs:133,162,190,236,265,326` | warm-boot save/restore — **restore is pre-cell, bootstrap-critical** |
| Disk layout contract | `kernel/src/loader/disk_layout.rs:43-90` | CELL_TABLE_BASE_LBA, CellTableHeader/CellEntry (repr(C), 512B) |
| Existing Block Cell skeleton | `cells/drivers/disk/src/lib.rs:11-40` | userspace `RamDisk: ViBlockDevice` — model the virtio-blk Cell on this + NVMe cell |

## Precedents to copy (do not reinvent)
- **NVMe Driver Cell** + `service::BLOCK_DRIVER` lookup — the userspace-block-over-IPC pattern is already shipped (`block.rs:11-14`, `block_stream.rs:8-13`).
- **virtio-net bounce-buffer CellHal** — the DMA-from-cell-heap solution (cell VA ≠ PA) for a `#![forbid(unsafe_code)]` driver cell using `virtio-drivers`.
- **x86_64 ramdisk boot** — `main.rs:387-398,445` already boots with zero VirtIO block, VIFS1 serves all ELFs.

## Red-team corrections (verified against code, 2026-07-07)
- **`/bin` in VFS proxies to the embedded VIFS1 ramdisk, NOT the disk** — `backend_bootfs.rs:1-8` (`BootFsProxy` → `sys_open`/`sys_read_cap` on `kernel_fs.img`), mount table `manager.rs:34-52` (`/`→RamFS, `/tmp`→RamFS, `/data`→littlefs P4, `/mnt/sd`→FAT32 P1, `/bin`→BootFsProxy, `/srv`→RedoxFS P5). The disk cell store (P2 `CELL_TABLE`) has **no VFS backend** — read only by `early.rs`.
- **P2-only cells** (`gen_disk.ps1:424-473`, absent from VIFS1 set `:336-375`): fb-console, robot-demo, robot-dashboard, Hypha stack (llm-gateway, hypha, tool-fs/sys/spawn), nc/curl/wget/httpd/mqtt, bench/bench-probe, posix-shim-test, input-test, Zig cells. → Phase 03 migrates these to a VFS-served FS.
- **`spawn_from_path` callers (not init-only):** shell `executor.rs:967`, supervisor `hotswap.rs:167`, Hypha `hypha/core/src/main.rs:71-101` + `tools/spawn:56`, Lua `bindings_io.rs:68`, init `init/src/main.rs:120-201,270`. → Phase 04 makes it an ostd wrapper.
- **Grant cell→kernel direction is sound** — `GrantAlloc` returns phys base == identity SAS vaddr, `owner=caller` (`syscall.rs:2679-2701`); kernel reads frames directly. Ceiling = 16 MiB (`:2681-2683`).
- **x86 not fully ramdisk-only** — `EarlyLoader::probe` is riscv/aarch64-gated (`main.rs:438`) so x86 loads cells from VIFS1, BUT x86 data I/O uses `virtio_pci::init` (`main.rs:414`) + `sys_blk_read` fallback (`block_stream.rs:63-83`); modern VirtIO-PCI blk `0x1042` unimplemented in-kernel (`virtio_pci.rs:105-115`). → x86 block pins to NVMe cell.
- **SUM is cross-cutting** — `main.rs:483` (global) + `task.rs:568` per-task `sstatus=0x42120` + secondary harts. → Phase 07 deferred/spike-first.

## Decisions locked (with user, 2026-07-07)
- **Boot:** RAM image / zero kernel device driver (unify RISC-V onto x86's VIFS1 ramdisk path).
- **Post-boot spawn:** init reads ELF from VFS → new `sys_spawn_from_elf(bytes)`; kernel = bootstrap-ramdisk reader + verifier-of-bytes. Rejected kernel-as-IPC-client.
- **Scope:** Full closure (delete kernel virtio_blk stack + scoped-SUM to drop whole-lifetime SUM=1).
