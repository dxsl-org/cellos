# Phase 16 — Complete Compositor & GPU

**Effort:** 150h | **Priority:** P2 | **Status:** pending | **Blockers:** Phase 14

## Overview

Replace stub compositor & VirtIO GPU code with a working window manager + framebuffer pipeline. Cells expose Surfaces; compositor blits them to the VirtIO GPU's framebuffer with Z-order and damage tracking. Input cell (Phase 14) routes pointer/key events to focused Surface. After this phase, `run-gui.ps1` produces a multi-window graphical shell session in QEMU.

## Context Links

- `docs/06-graphics.md` — compositor architecture
- `cells/services/compositor/src/lib.rs` — current stub
- `cells/drivers/gpu/src/lib.rs` — VirtIO GPU stub
- `kernel/src/task/drivers/virtio_gpu.rs` — kernel-side GPU driver
- Phase 14 dependency: input cell publishes focus + routes pointer events

## Key Insights

- VirtIO GPU 2D mode (no 3D): the kernel driver creates a 2D resource via `VIRTIO_GPU_CMD_RESOURCE_CREATE_2D`, attaches host backing storage via `VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING`, then the compositor `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH` after each frame.
- **Protocol decision (Validation Session 1): Custom `ViSurface` trait** in `libs/api/src/display.rs`. NOT Wayland wire protocol — simpler, native ViOS design. Wayland compat deferred to v1.x. <!-- Updated: Validation Session 1 -->
- Surface = a per-cell framebuffer plus a position + Z. Surfaces are capabilities (Phase 07 pattern); a cell receives a Surface cap from compositor on `CreateSurface`.
- Damage tracking: cells declare a damage rect via `DamageSurface(cap, rect)`; compositor only re-blits the damaged area. Crucial for performance on slow QEMU display.
- Software rendering only in v1.0; no GPU-accelerated commands. Compositor blends in CPU.
- Resolution: read from VirtIO GPU `GET_DISPLAY_INFO`; default 1024×768 on QEMU.

## Requirements

**Functional**
- Compositor cell exposes IPC API: `CreateSurface(w, h) → SurfaceCap`, `DamageSurface(cap, rect)`, `MoveSurface(cap, x, y)`, `RaiseSurface(cap)`, `DestroySurface(cap)`
- VirtIO GPU driver shows compositor's blended framebuffer
- Multi-window: spawn 2 surfaces, both visible, Z-ordered correctly
- Pointer events routed to the surface under the cursor; click → focus change
- Shell can be the first surface (terminal-in-window using fb_console)

**Non-functional**
- 30 FPS sustained when one full-screen surface marks itself damaged each frame
- Per-frame CPU < 50% in QEMU on a 1024×768 single-surface workload
- Zero `unsafe` in compositor cell; kernel GPU driver has documented unsafe

## Architecture

```
Cell A (e.g., shell)         Cell B (e.g., clock)
   │  CreateSurface(640,480)    │  CreateSurface(200,100)
   │  draw into surface buf     │
   │  DamageSurface(rect)        │  DamageSurface(rect)
   ▼                             ▼
   ┌─────────────────────────────────────┐
   │ Compositor Cell                     │
   │  ├─ surface table CapId → Surface   │
   │  ├─ z-order list                    │
   │  ├─ damage region accumulator       │
   │  ├─ render loop (vsync ≈ 30Hz)      │
   │  │    blend damaged regions into FB │
   │  │    issue TRANSFER_TO_HOST_2D     │
   │  │    issue RESOURCE_FLUSH          │
   │  └─ input subscription (Phase 14)   │
   └──────────────┬──────────────────────┘
                  │ FB upload commands
                  ▼
   Kernel VirtIO GPU driver
                  │
                  ▼ DMA to host
   QEMU display (SDL window or VNC)
```

## Related Code Files

**Modify:**
- `cells/services/compositor/src/lib.rs` — full compositor
- `cells/drivers/gpu/src/lib.rs` — thin cell-side wrapper of kernel GPU driver
- `kernel/src/task/drivers/virtio_gpu.rs` — implement RESOURCE_CREATE_2D, ATTACH_BACKING, TRANSFER_TO_HOST_2D, RESOURCE_FLUSH, SET_SCANOUT
- `libs/api/src/display.rs` (CREATE — not present yet) — `Surface`, `Rect`, `PixelFormat`, IPC schema
- `cells/apps/shell/src/main.rs` — when graphical mode enabled, create a Surface and use it as terminal
- `kernel/src/task/drivers/fb_console.rs` — generalize fb_console to write into an arbitrary surface (currently writes direct to GPU FB)

**Create:**
- `libs/api/src/display.rs` — display API types
- `cells/services/compositor/src/surface_table.rs` — CapId-keyed surface state
- `cells/services/compositor/src/z_order.rs` — surface z-ordering list ops
- `cells/services/compositor/src/render.rs` — blending + damage-region rasterizer
- `cells/services/compositor/src/input_routing.rs` — translate pointer event → surface under cursor → focus switch via input cell
- `cells/services/compositor/src/cursor.rs` — software cursor sprite
- `tests/integration/compositor_basic.rs` — create 2 surfaces, fill each with color, verify framebuffer pixels (read back via VirtIO GPU CAPTURE if available, else by-hash via known seed)
- `docs/display-api.md` — IPC schema, pixel format, damage rules

## Implementation Steps

1. **Define types in `libs/api/src/display.rs`**:
   ```rust
   #[repr(C)] pub struct Rect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }
   #[repr(C)] pub enum PixelFormat { Bgra8888, Rgba8888 }
   #[repr(transparent)] pub struct SurfaceCap(CapId);
   impl SurfaceCap {
       pub async fn write_pixels(&self, rect: Rect, data: Box<[u8]>) -> Result<…>;
       pub async fn damage(&self, rect: Rect) -> Result<…>;
       pub async fn move_to(&self, x: i32, y: i32) -> Result<…>;
       pub async fn destroy(self) -> Result<…>;
   }
   ```
2. **Kernel VirtIO GPU driver `virtio_gpu.rs`**:
   - Send `GET_DISPLAY_INFO` → store {width, height, rect[0]}
   - Allocate one host-side resource of W×H BGRA8888 via `RESOURCE_CREATE_2D`
   - Allocate a backing buffer (HHDM frames), attach via `RESOURCE_ATTACH_BACKING`
   - `SET_SCANOUT(scanout_id=0, resource_id=…)`
   - Expose `transfer_and_flush(rect)` to the cell-side wrapper
3. **Cell-side GPU wrapper** `cells/drivers/gpu/src/lib.rs`:
   - Single function: `flush(rect: Rect)` which sends IPC to kernel driver
4. **Compositor render loop** `cells/services/compositor/src/render.rs`:
   - Maintain a "screen FB" in the compositor's address space (W*H*4 bytes)
   - On wake (timer 33ms OR damage event):
     - Walk z-order from bottom → top
     - For each surface whose damage region intersects the accumulated dirty rect, blend its content into screen FB
     - Blit cursor sprite at top
     - Call GPU wrapper's flush(dirty_rect)
5. **Surface table** `cells/services/compositor/src/surface_table.rs`:
   - `BTreeMap<SurfaceCap, SurfaceState { x, y, w, h, z, pixels: Box<[u8]>, damage: Option<Rect> }>`
6. **Z-order** `cells/services/compositor/src/z_order.rs`:
   - `Vec<SurfaceCap>` from bottom to top; `RaiseSurface` moves to end
7. **Input routing** `cells/services/compositor/src/input_routing.rs`:
   - Subscribe to InputEvent::MouseMove/Button from input cell
   - Maintain cursor x,y
   - On click: find topmost surface under cursor → call input cell `set_focus(cell_id_owning_surface)`
8. **Generalize fb_console** so shell-in-surface works:
   - `fb_console.rs::FbConsole::new(target_buf: &mut [u8], stride: u32, w: u32, h: u32)`
   - Move kernel's hardcoded GPU FB write into a Surface, owned by shell cell when graphical
9. **Shell-in-surface mode**:
   - On boot, shell checks `config.get("display.mode") == "graphical"`
   - If yes: `CreateSurface(640, 480)` from compositor, use FbConsole to render text to its pixels, DamageSurface on each new line
10. **Integration test** `tests/integration/compositor_basic.rs`:
    - Spawn 2 dummy cells creating 2 surfaces of solid colors
    - Read back framebuffer (compositor exposes `dump_fb` debug API)
    - Assert pixel at known positions equals expected color
11. **Document** `docs/display-api.md`.

## Todo List

- [ ] Define `libs/api/src/display.rs`
- [ ] Implement kernel VirtIO GPU commands (CREATE_2D, ATTACH_BACKING, SCANOUT, TRANSFER, FLUSH)
- [ ] Cell-side GPU wrapper
- [ ] Compositor render loop with damage
- [ ] Surface table + z-order
- [ ] Cursor sprite
- [ ] Input routing (click → focus change)
- [ ] Generalize fb_console to render into arbitrary surfaces
- [ ] Shell-in-surface graphical mode
- [ ] Integration test: 2 surfaces, color-check FB
- [ ] Document `docs/display-api.md`
- [ ] Performance: 30 FPS sustained

## Success Criteria

- `run-gui.ps1` opens QEMU window with compositor visible
- Two surfaces both render correctly with correct Z-order
- Mouse click changes focus to the clicked surface
- 30 FPS sustained on damaged full-frame redraw
- Compositor cell `#![forbid(unsafe_code)]`

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| VirtIO GPU command sequencing quirks (host hangs on missing FLUSH) | High | High | Wrap commands in a tested state machine; log every command in debug builds |
| 1024×768 ×4 = 3 MB framebuffer; copy cost in software blend dominates | Cert | Med | Damage tracking reduces avg case; for v1.0 we accept worst-case at 30 FPS |
| Z-order changes mid-render race | Med | Low | Snapshot z-order at start of render frame |
| Pointer hit-test ignores transparency | Low | Low | v1.0: rectangular hit-test only; document |
| Resolution from VirtIO GPU varies (host window resize) | Med | Med | Pin to 1024×768 in QEMU args; revisit dynamic resize post-v1.0 |

## Security Considerations

- A cell can only damage its own Surface (capability-checked)
- Pixel data from one cell never visible to another (compositor only reads, never exposes cells' pixel buffers)
- Cursor + focus tracking handled centrally; no cell can spoof focus

## Rollback

Revert removes compositor; text shell on raw FB still works via existing fb_console direct path. Phase 17's GUI utilities defer.

## Next Steps

GUI utilities (clock, file manager) layer on top in v1.x. Phase 22 benchmarks frame time. Hardware acceleration is post-v1.0.
