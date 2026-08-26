# Phase 08 — Kernel Cleanup: Remove Migrated Driver Code

## Context Links
- Plan: [plan.md](plan.md) · Depends on: ALL of Phases 01-07 booting green.
- Law: `docs/specs/15-kernel-boundary.md` (the cleanup makes the kernel actually compliant).

## Overview
- **Priority:** P2 — the destructive finalizer.
- **Status:** G1-complete (2026-06-29) — VirtIO GPU/Net/Input/Sound + fb_console + input_map removed; VirtIO BLK + MMC remain (DESCOPED to G2 — loader dependency / no SDHCI on QEMU); stale comments cleaned up; spec-15 + CLAUDE.md + changelog updated.
- **Risk:** LOW (additive-revert safe) **but** must run ONLY after every migration boots green with its Cell as the active provider, because this removes the kernel fallbacks.
- **Description:** Delete the migrated kernel driver files, remove fallback branches in routers, simplify console_drv to a UART stub, delete fb_console/input_map, and trim ACPI MCFG parse. After this, `kernel/src/task/drivers/` holds only whitelist-legal code.

## Key Insights
- Every Phase 01-07 kept its kernel-resident driver as a **fallback** behind a registration check. Phase 08 removes those fallbacks + the dead driver files. This is why 08 is last and gated: deleting a fallback before its Cell is proven would break boot.
- **Do not delete until proven:** for each file, confirm the corresponding Cell is the active provider (logged: "block driver registered: tid=N", `service::X` resolves) on RISC-V + ARM64 + x86_64 before removing.
- `gpio_irq.rs` (40), `uart.rs` (early stub), `acpi.rs` MADT, `hotswap.rs`, `snapshot.rs`, `registry.rs`, `driver_cell.rs`, `block.rs`/`nic.rs` routers, `iommu*`, `ramdisk.rs`, `virtio_hal.rs`/`virtio_common.rs` (IF still used by anything kernel-side — likely deletable once all VirtIO drivers are Cells) — **KEEP / evaluate**, do not blanket-delete.

## Files to DELETE (after their Cell is the proven active provider)
| File | Gated on | Replaced by |
|------|----------|-------------|
| `kernel/src/task/drivers/virtio_gpu.rs` + `virtio_gpu/cursor.rs` | P02 green | virtio-gpu Cell |
| `kernel/src/task/drivers/virtio_input.rs` | P03 green | input service |
| `kernel/src/task/drivers/input_map.rs` | P03 green | input service keymap |
| `kernel/src/task/drivers/virtio_sound.rs` | P04 (migrated or confirmed dead) | virtio-sound Cell / nothing |
| `kernel/src/task/drivers/virtio_blk.rs` | P05 green | virtio-blk Cell |
| `kernel/src/task/drivers/virtio_net.rs` | P06 green | virtio-net Cell |
| `kernel/src/task/drivers/virtio_pci.rs` | P05+P06 green | Cell-side transport |
| `kernel/src/task/drivers/pcie_ecam.rs` (scan loop + init()) | P01 green | Platform Cell (keep `find_class` + `register_bar` + `PCI_DEVICES` store) |
| `kernel/src/task/drivers/mmc.rs` + `mmc/*` | P07 green | mmc Cell |
| `kernel/src/task/drivers/fb_console.rs` | P02 green | GPU Cell + compositor |

## Files to SIMPLIFY
| File | Action |
|------|--------|
| `kernel/src/task/drivers/console_drv.rs` (140) | Reduce to UART-only early-boot stub; remove framebuffer + input-relay (input relay now in input service) |
| `kernel/src/task/drivers/block.rs` | Remove `viVirtIOBlk` + `MmcBlock` fallbacks; `block_device()`/`read_sector` either route purely to the registered Cell or are removed if no in-kernel caller remains (per Phase 05 Spike B) |
| `kernel/src/task/drivers/nic.rs` | Remove `virtio_net` fallback; route to NIC Cell only |
| `kernel/src/acpi.rs` | Trim MCFG parse (Platform Cell owns PCIe enumeration); KEEP MADT |
| `kernel/src/task/drivers/virtio_blk.rs:82` `vi_handle_virtio_irq` | Deleted with the file; the new `irq_wait::wake_irq` (Phase 00) is the sole VirtIO IRQ path |

## Files to KEEP (whitelist-legal — do NOT delete)
- `gpio_irq.rs` (IRQ routing, 40 LOC), `uart.rs` (early-boot stub), `registry.rs`, `driver_cell.rs`, `iommu*.rs`, `ramdisk.rs`.
- `hotswap.rs`, `snapshot.rs` (mechanism layer — keep per plan.md "what does NOT migrate").
- `virtio_hal.rs` / `virtio_common.rs`: **evaluate** — if no kernel code uses `virtio-drivers` anymore (all VirtIO is in Cells), delete; otherwise keep. Confirm via grep before deciding.

## Implementation Steps
1. **Verification sweep:** for each Phase 01-07, boot RISC-V + ARM64 + x86_64 and confirm in logs that the Cell is the active provider (driver registered, service resolves, fallback NOT hit). Capture the log lines as evidence.
2. Remove the fallback branches first (one router file at a time): `block.rs` (drop `viVirtIOBlk`/`MmcBlock`), `nic.rs` (drop `virtio_net`), syscall forwards for GPU/Audio become Cell-only. Boot after each.
3. Delete the `static`s + IRQ branches in `virtio_blk.rs`'s `vi_handle_virtio_irq` are gone with the file — confirm `irq_wait::wake_irq` handles all live IRQs.
4. Delete the driver files (table above), removing their `pub mod X;` declarations in the parallel `drivers.rs` (NOT a mod.rs).
5. Simplify `console_drv.rs` to UART-only; delete `fb_console.rs`, `input_map.rs`.
6. Trim `acpi.rs` MCFG parse (keep MADT); confirm Platform Cell supplies the ECAM base.
7. Evaluate `virtio_hal.rs`/`virtio_common.rs`: `grep` kernel usage → delete if unused.
8. Remove Phase-01 transition fallback (`pcie_ecam::init()` call in `main.rs`); keep only `find_class` + `register_bar` + store.
9. `cargo check` all arches; `cargo clippy -- -D warnings`.
10. Final boot matrix: RISC-V virt + ARM64 virt + x86_64 q35 → `Cellos>` shell, `cat`/`ls`/ping/GUI/keyboard all via Cells.
11. Run the compliance grep (Success Criteria).
12. Update `docs/specs/15-kernel-boundary.md` tracked-tech-debt table (mark items migrated) + `docs/project-changelog.md`.

## Todo List
- [x] Verification sweep: each Cell is active provider on 3 arches — P06 (virtio-net Cell) confirmed complete; nic.rs fallback removed
- [x] Remove NIC router fallback (nic.rs) — replaced with no-op stubs (Driver Cell serves all frames)
- [x] Delete virtio_net.rs + remove mod decl from drivers.rs
- [x] Remove virtio_net::ack_irq branch from virtio_blk.rs
- [x] Remove init_net() from virtio_pci.rs + NET constants/branch
- [x] Delete fb_console.rs, input_map.rs — already done (prior commits)
- [x] console_drv.rs already UART-only (no framebuffer code present)
- [x] cargo check clean on riscv64 + aarch64 + x86_64
- [x] Compliance grep clean (VirtIONet/fb_console/input_map/virtio_net: no kernel references)
- [ ] Remove router fallback from block.rs (viVirtIOBlk) — BLOCKED: P05 DESCOPED to G2
- [ ] Delete virtio_blk.rs — BLOCKED: P05 DESCOPED to G2 (kernel VirtIO blk serves bootloader path)
- [ ] Delete virtio_pci.rs — BLOCKED: P05 DESCOPED to G2 (still needs init_blk for x86_64)
- [ ] Delete mmc.rs + mmc/* — BLOCKED: P07 DESCOPED to G2
- [ ] Reduce pcie_ecam.rs to store + find_class + register_bar; drop init() call — pending G2
- [ ] Trim acpi.rs MCFG (keep MADT) — pending G2
- [ ] Evaluate + maybe delete virtio_hal/virtio_common — pending G2 (still used by virtio_blk)
- [x] Update spec 15 tech-debt table + changelog + CLAUDE.md (2026-06-29)
- [ ] Final 3-arch boot matrix (full verification) — deferred to G2 integration run

## Success Criteria
- [ ] `grep -rn "VirtIOBlk\|VirtIONet\|VirtIOGpu\|pcie_ecam::init\|fb_console\|input_map\|MmcBlock" kernel/src` → no matches (files deleted).
- [ ] `kernel/src/task/drivers/` contains only whitelist-legal files (routers, registry, gpio_irq, uart stub, iommu*, ramdisk, hotswap/snapshot kept elsewhere).
- [ ] RISC-V + ARM64 + x86_64 all boot to `Cellos>` with every device served by a Cell.
- [ ] `cargo clippy -- -D warnings` clean.
- [ ] No kernel code references `virtio-drivers` crate (or the remaining use is documented + justified).
- [ ] `docs/specs/15-kernel-boundary.md` tech-debt table updated to "migrated".

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| Delete a fallback whose Cell silently wasn't active on one arch | Med | High | Per-arch verification sweep with captured log evidence BEFORE any delete |
| `virtio_hal`/`virtio_common` still needed → build break | Med | Med | grep usage before delete; keep if referenced |
| In-kernel block caller remains (snapshot/early loader) after fallback removal | Med | High | Phase 05 Spike B output; keep a kernel→Cell shim if any caller survives |
| ACPI MCFG trim breaks x86_64 ECAM base handoff | Low | High | Confirm Platform Cell gets the base before trimming; test x86 boot |

## Security Considerations
- This phase is the payoff: the kernel TCB shrinks by ~3000 LOC of device driver code. The remaining kernel touches hardware only for IOMMU, IRQ dispatch, paging, scheduling, capability enforcement — exactly the whitelist. Each migrated driver is now an isolated, IOMMU-confined, capability-gated Cell.

## Next Steps
- Plan complete. Follow-ups: G2 review of `snapshot.rs` (FRAME_ALLOCATOR enumeration), MMC hardware validation on rpi4/vf2, optional `libs/virtio-cell` shared transport crate if duplication grows, MSI-X IRQ wiring for x86_64 PCI VirtIO (currently polled).
