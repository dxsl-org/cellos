# Phase 02 — VirtIO GPU Driver Cell

## Context Links
- Plan: [plan.md](plan.md) · Prereq: [phase-00](phase-00-prerequisites.md)
- Source: `kernel/src/task/drivers/virtio_gpu.rs` (183) + `virtio_gpu/cursor.rs`
- Related cleanup: `kernel/src/task/drivers/fb_console.rs` (188) — removed in Phase 08 once GPU Cell + compositor own the framebuffer.
- Existing GPU cell stub: `cells/drivers/gpu/` (check whether this is a pre-existing scaffold to extend vs a new `virtio-gpu`).
- Syscalls today: `GpuFlush=300`, hardware cursor op (`syscall.rs:97-100`). Compositor uses Grant shared memory for surfaces (per memory: compositor-grant-plan).

## Overview
- **Priority:** P2 (parallel after Phase 00; not on the boot-critical path).
- **Status:** complete
- **Risk:** MED — must not break the compositor's framebuffer flush path during transition.
- **Description:** Migrate the VirtIO GPU driver + hardware cursor into a Driver Cell. The compositor talks to it via IPC (replacing the kernel `GpuFlush=300` syscall path) or the Cell registers as a display provider. Cursor sprite/move logic moves with it.

## Key Insights
- **Check first:** `cells/drivers/gpu/` already exists (in workspace). Determine if it's an empty scaffold, a partial impl, or unrelated. If it's the intended target, extend it; do not create a parallel `virtio-gpu`. (Resolve in Step 1 — likely rename target to `cells/drivers/virtio-gpu/` OR fill `cells/drivers/gpu/`.)
- The compositor currently flushes via `GpuFlush=300` (kernel) and uses Grant shared memory for surfaces. After migration, the kernel `GpuFlush` syscall either (a) forwards to the GPU Cell, or (b) is replaced by direct compositor→GPU-Cell IPC. **Decision:** keep `GpuFlush` syscall as a thin kernel→Cell forward during transition, then move compositor to direct IPC (cleaner, but larger change — defer the compositor rewrite, keep the forward).
- Hardware cursor (`virtio_gpu/cursor.rs`, `setup_cursor`/`move_cursor` 64×64) moves into the Cell; the kernel cursor syscall forwards.
- GPU DMA pipeline already exists (per memory: compositor-cursor-gpu) — the Cell reuses `ostd::dma`.

## Requirements
### Functional
1. `cells/drivers/virtio-gpu/` (or fill `cells/drivers/gpu/`): claim VirtIO GPU MMIO/BAR, init display, expose flush + cursor over IPC.
2. Kernel `GpuFlush`/cursor syscalls forward to the GPU Cell when registered; kernel `virtio_gpu.rs` is fallback until Phase 08.
3. Compositor unchanged in this phase (still calls `GpuFlush`); transition is transparent.

### Non-Functional
- `#![forbid(unsafe_code)]` except MMIO/DMA island.
- Damage-driven render preserved (move must set pending_dirty — per memory).

## Architecture
```
Compositor → GpuFlush(rect) [kernel syscall] → forward IPC → GPU Cell → virtqueue → display
                                                              cursor IPC → setup/move_cursor
```
GPU Cell registers a new `service::GPU` (or reuses an existing display-provider id). Kernel `GpuFlush` arm: if GPU Cell registered, forward the rect+grant to it; else kernel `virtio_gpu` fallback.

## Related Code Files
**Create/extend:** `cells/drivers/virtio-gpu/` (Cargo.toml, build.rs, src/main.rs, src/display.rs, src/cursor.rs, src/dispatch.rs).
**Modify:**
- `kernel/src/task/syscall.rs` — `GpuFlush` + cursor arms forward to GPU Cell when registered.
- `kernel/src/task/drivers/driver_cell.rs` — add `GPU_DRIVER_CELL` AtomicUsize (mirror block/nic).
- `kernel/src/loader.rs` — `/bin/virtio-gpu` PcieDriverCap grant.
- `cells/tools/init/src/main.rs` — spawn virtio-gpu before compositor.
- `libs/api/src/syscall.rs` — `service::GPU` const (Law 1).
- `gen_disk.ps1` + root Cargo.toml.

## Implementation Steps
1. Resolve `cells/drivers/gpu/` status (read its Cargo.toml + main.rs). Pick target dir.
2. Scaffold from nvme template; add `service::GPU`.
3. Port `virtio_gpu.rs` init + flush + `cursor.rs` into the Cell (ostd::mmio/dma).
4. Define IPC: flush request (grant handle + rect), cursor request (op + sprite/xy). Reuse Grant shared-memory surfaces.
5. Init: claim GPU MMIO/BAR → init display → register `service::GPU`.
6. Kernel `GpuFlush`/cursor arms: forward when `GPU_DRIVER_CELL != 0`; else fallback.
7. init spawns virtio-gpu before compositor.
8. gen_disk + Cargo member.
9. Boot GUI test (`run-gui.ps1`): compositor renders, cursor moves, robot-dashboard draws.

## Todo List
- [ ] Resolve cells/drivers/gpu status
- [ ] Scaffold + service::GPU
- [ ] Port display + cursor to Cell
- [ ] Flush/cursor IPC protocol
- [ ] Init claim + register
- [ ] Kernel GpuFlush/cursor forward
- [ ] init spawn order
- [ ] gen_disk + member
- [ ] GUI boot test

## Success Criteria
- [ ] Compositor renders through the GPU Cell (kernel `virtio_gpu` static unused).
- [ ] Hardware cursor set + move works (64×64 sprite).
- [ ] robot-dashboard / DOOM render unchanged.
- [ ] Disabling the Cell → kernel fallback renders (rollback proof).

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| Compositor flush latency via IPC forward | Med | Med | Forward is thin; Grant surfaces are zero-copy (no pixel copy) |
| cells/drivers/gpu collision / duplicate | Med | Low | Step 1 resolves dir; no parallel "enhanced" copy (coding rule) |
| Cursor sprite path breaks | Low | Med | Keep kernel fallback until GUI test green |

## Security Considerations
- GPU MMIO claimed exclusively; DMA authorized to GPU BDF. Compositor passes grants, not raw pointers — a buggy compositor cannot make the GPU DMA arbitrary memory.

## Next Steps
- Phase 08 removes `virtio_gpu.rs`, `virtio_gpu/cursor.rs`, and `fb_console.rs` (the GPU Cell + compositor now own all display output).
