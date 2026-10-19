# Cellos Display / Compositor API

The Compositor Cell (`cells/services/compositor/`) manages a z-ordered set of
cell surfaces, blends them in software, and flushes dirty regions to the VirtIO
GPU via the `GpuFlush` kernel syscall.

---

## Architecture

```
Consumer cell (shell / app)
        │  IPC: CreateSurface, WritePixels, DamageSurface, …
        ▼
Compositor Cell (cells/services/compositor/)
        ├─ surface_table: CapId → SurfaceState { pixels, pos, damage }
        ├─ z_order: paint order list (bottom → top)
        ├─ render loop (≈ 30 FPS, damage-driven):
        │    blit dirty surfaces into screen FB
        │    call sys_gpu_flush(dirty_rect)
        │                    │
        ▼                    ▼
Kernel GpuFlush syscall (id 300)
        │  copies pixels + calls gpu.flush()
        ▼
VirtIO GPU kernel driver (virtio_gpu.rs)
        │
        ▼
QEMU display (SDL window / VNC)
```

---

## Pixel Format

All surfaces and the screen framebuffer use **BGRA8888** (4 bytes per pixel):
- byte 0: Blue
- byte 1: Green
- byte 2: Red
- byte 3: Alpha (currently unused by compositor — treated as opaque)

Row major, left-to-right, top-to-bottom.

---

## Compositor IPC Protocol

All messages are sent to the compositor's IPC endpoint
(`api::display::COMPOSITOR_ENDPOINT = 5`).

### Request envelope

```
byte[0]   = opcode
byte[1..] = opcode-specific payload
```

### Opcodes

| Opcode | Name | Payload | Reply |
|--------|------|---------|-------|
| `0x01` | `CREATE_SURFACE` | `w: u32 LE, h: u32 LE` | `CapId (u64 LE)`, 0 = error |
| `0x02` | `WRITE_PIXELS` | `cap: u64, x: i32, y: i32, w: u32, h: u32, pixel_data` | — |
| `0x03` | `DAMAGE_SURFACE` | `cap: u64, x: i32, y: i32, w: u32, h: u32` | — |
| `0x04` | `MOVE_SURFACE` | `cap: u64, x: i32, y: i32` | — |
| `0x05` | `RAISE_SURFACE` | `cap: u64` | — |
| `0x06` | `DESTROY_SURFACE` | `cap: u64` | `0x00` ok |
| `0x0A` | `SET_TITLE` | bounded UTF-8 title, `cap: u32` | — |
| `0x0B` | `CONFIGURE_ACK` | `cap: u32, serial: u32` | — |
| `0x0C` | `CLOSE_RESPONSE` | `cap: u32, serial: u32, accept: u8` | — |
| `0x0D` | `MINIMIZE` | `cap: u32` | — |
| `0x0E` | `MAXIMIZE` | `cap: u32` | — |
| `0x0F` | `RESTORE` | `cap: u32` | — |
| `0x10` | `GET_SCREEN_SIZE` | — | `w: u32, h: u32` |
| `0x11` | `DETACH_REPLACED_GRANT` | `cap: u32, old_reg_id: u64` | success/failure |
| `0x20` | `CREATE_VIEW` | `CreateViewRequest` (64 bytes) | `ViewOpResponse` (20 bytes) |
| `0x21` | `DESTROY_VIEW` | `[frame_id, frame_gen, view_id, view_inc]` (16 bytes) | `ViewOpResponse` (20 bytes) |
| `0x22` | `SELECT_VIEW` | `[frame_id, frame_gen, view_id, view_inc]` (16 bytes) | `ViewOpResponse` (20 bytes) |
| `0x23` | `GROUP_VIEW` | `[tgt_fid, tgt_gen, src_fid, src_gen, vid, vinc]` (24 bytes) | `ViewOpResponse` (20 bytes) |
| `0x24` | `DETACH_VIEW` | `[src_fid, src_gen, vid, vinc, x: i32, y: i32]` (24 bytes) | `ViewOpResponse` (20 bytes) |
| `0x25` | `STAGE_GROUP_CONFIGURE` | atomic frame resize proposal | — |
| `0x26` | `COMMIT_GROUP_CONFIGURE` | commit staged group configuration | — |
| `0x27` | `SNAP_FRAME` | `[frame_id: u32, frame_gen: u32, zone: u8, _pad: [u8; 3]]` (12 bytes) | `ViewOpResponse` (20 bytes) |
| `0x28` | `TOGGLE_FULLSCREEN` | `[frame_id: u32, frame_gen: u32, enable: u8, _pad: [u8; 3]]` (12 bytes) | `ViewOpResponse` (20 bytes) |
| `0xFE` | `DUMP_FB` | — | raw BGRA pixels (debug only) |

### Notes

- **WRITE_PIXELS** does not trigger a render; call **DAMAGE_SURFACE** after writing
  to schedule the region for the next frame.
- **DAMAGE_SURFACE** coordinates are surface-local (0,0 = top-left of the surface).
- Surfaces are automatically given an initial position of (0, 0).  Use **MOVE_SURFACE**
  to reposition before the first damage.
- Destroying a surface does not flush; the compositor composites the hole on the
  next render tick.

---

## Surface Capability Lifecycle

```
CREATE_SURFACE(w, h) → CapId N
       │
       ├─ WRITE_PIXELS(N, x, y, pw, ph, data)
       ├─ DAMAGE_SURFACE(N, x, y, pw, ph)   ← triggers redraw
       ├─ MOVE_SURFACE(N, new_x, new_y)
       ├─ RAISE_SURFACE(N)
       │
       └─ DESTROY_SURFACE(N)                 ← CapId freed
```

### Window-policy lifecycle

Interactive `ViSurface`s use `set_title`, `minimize`, `maximize`, `restore`,
and `respond_close` for owner-authorized lifecycle requests. Owners consume
trusted `SurfaceEvent::{Configure,CloseRequest,StateChanged}` frames through
`poll_surface_events` without consuming normal forwarded input frames.
A `WindowConfigure` is a proposal, not a live resize. Call
`ViSurface::apply_configure(configure)` to allocate/attach the replacement
Grant and acknowledge its matching serial; only that successful transaction
swaps the client's local pixel buffer and allows the compositor to commit the
new content geometry. It then detaches the identified old Grant, never the new
active Grant. Frame, titlebar, and controls remain compositor-owned outside the
client coordinate system. Background surfaces remain visible but cannot
participate in this policy.

---

## Native Views, Frames & Session Lifecycle (ADR-0027 Phase 03)

Under ADR-0027, window frames and tab views are decoupled:
- **Frame (`Frame`)**: Compositor-owned window container managing position `(x, y)`, outer dimensions `(w, h)`, minimum constraints `(min_w, min_h)`, tab list (`Vec<ViewRef>`), and selected active tab.
- **View (`View`)**: An application-owned session associated with an authentic client process (`owner_tid`), backing surface (`surface_cap`), role (`SurfaceRole`), and generational incarnation.

### Multi-View and Mixed-Owner Support
- Applications can allocate multiple tab views within the same frame (`req.frame_id != 0`) or spawn new frames (`req.frame_id == 0`).
- Views owned by distinct processes (`owner_tid`) can share a single frame without privilege escalation, capability leakage, or impersonation.

### Generational Validation
All view operations enforce generational qualifiers:
- `FrameRef`: `frame_id: u32`, `generation: u32` (8 bytes)
- `ViewRef`: `view_id: u32`, `incarnation: u32` (8 bytes)
- Any state modification (view attach, destroy, select, group, detach, or owner crash) monotonically increments frame generation (`wrapping_add(1)`).
- Operations presenting stale generations fail with `ViewOpResult::StaleGeneration`, preventing race conditions and TID-reuse vulnerabilities.

### Wire Structures & Protocol
- **`CreateViewRequest`** (64 bytes, naturally 8-byte aligned, `CREATE_VIEW_REQUEST_WIRE_SIZE`):
  `opcode` (1B), `role` (1B), `_pad` (2B), `frame_id` (4B), `frame_generation` (4B), `w` (4B), `h` (4B), `min_w` (4B), `min_h` (4B), `recipient_incarnation` (4B), `file_grant_id` (8B), `request_generation` (4B), `_reserved` (4B), `launch_token` (16B).
- **`ViewOpResponse`** (20 bytes, `VIEW_OP_RESPONSE_WIRE_SIZE`):
  `opcode` (1B), `result: ViewOpResult` (1B), `_pad` (2B), `frame_id` (4B), `frame_generation` (4B), `view_id` (4B), `view_incarnation` (4B).
- **`ViewOpResult`**:
  `Success` (0), `StaleGeneration` (1), `InvalidToken` (2), `AccessDenied` (3), `CapacityExhausted` (4), `FrameNotFound` (5), `ViewNotFound` (6), `Timeout` (7), `Refused` (8).

### DoS & Capacity Limits
`FrameRegistry` enforces hard limits preflighted before state publication:
- `MAX_VIEWS_PER_FRAME = 32`
- `MAX_FRAMES = 64`
Excess allocations return `ViewOpResult::CapacityExhausted`.

### Input Focus Handoff & Damage Tracking
- `DESTROY_VIEW` and `SELECT_VIEW` route input focus directly to the authentic surface owner (`table.get(cap).owner`), preventing focus-stealing attacks by malicious request senders.
- Tab transitions union full window decoration bounds into `pending_dirty` to guarantee clean, flicker-free repainting.

### Owner Exit Reclamation
- When an application process terminates or crashes, `cleanup_owner(dead_tid)` in `cells/services/compositor/src/message_handler.rs` sweeps and reclaims only that process's views and surfaces.
- Surviving tabs owned by other processes within shared frames remain fully intact, with input focus automatically transferred to the surviving active view.

---

## Six-Zone Snap & F11 Fullscreen Geometry (ADR-0027 Phase 05)

Under ADR-0027 Phase 05, the compositor manages six-zone window placement and content-only fullscreen mode:

### Placement Modes & Zones
- **`SnapZone`**: Identifies one of six screen zones:
  - `LeftHalf` (1): Left half of the usable work area ($0 \le X < \lfloor W/2 \rfloor$, full height).
  - `RightHalf` (2): Right half of the usable work area ($\lfloor W/2 \rfloor \le X < W$, full height).
  - `TopLeftQuarter` (3): Top-left quarter ($0 \le X < \lfloor W/2 \rfloor$, $0 \le Y < \lfloor H/2 \rfloor$).
  - `BottomLeftQuarter` (4): Bottom-left quarter ($0 \le X < \lfloor W/2 \rfloor$, $\lfloor H/2 \rfloor \le Y < H$).
  - `TopRightQuarter` (5): Top-right quarter ($\lfloor W/2 \rfloor \le X < W$, $0 \le Y < \lfloor H/2 \rfloor$).
  - `BottomRightQuarter` (6): Bottom-right quarter ($\lfloor W/2 \rfloor \le X < W$, $\lfloor H/2 \rfloor \le Y < H$).
- **`FramePlacement`**: `Floating` (default), `Snapped(SnapZone)`, `Maximized`, `Fullscreen`.

### Usable Work Area & Integer Grid Math
- **Work Area Exclusion**: Snapped windows and maximized windows exclude the fixed 36 DIP bottom taskbar:
  $$\text{work\_area} = (0, 0, \text{screen\_w}, \text{screen\_h} - 36)$$
- **Integer Arithmetic**: `calculate_snap_rect` uses shared midpoint boundaries:
  $$\text{half\_w} = \lfloor W / 2 \rfloor, \quad \text{other\_half\_w} = W - \text{half\_w}$$
  $$\text{half\_h} = \lfloor H / 2 \rfloor, \quad \text{other\_half\_h} = H - \text{half\_h}$$
  This guarantees zero pixel gaps, overlapping boundaries, or unpainted seams at odd resolutions and arbitrary UI scaling factors.

### Minimum-Size Preflighting
- Before applying snap geometry, `FrameRegistry::snap_frame` checks every view in the target frame.
- If any member view's `min_w > target_w` or `min_h > target_h`, the snap request is rejected with `ViewOpResult::Refused`.
- The window frame remains untouched in its prior state with zero geometry or focus disruption.

### Content-Only F11 Fullscreen & State Restoration
- **Enter Fullscreen (`enable = 1`)**:
  - Saves existing placement in `frame.saved_placement`.
  - Saves floating geometry in `frame.saved_floating_rect` if the frame was in `Floating` mode.
  - Expands window bounds to cover the entire physical screen $(0, 0, \text{screen\_w}, \text{screen\_h})$, hiding tab headers, window chrome, and the bottom taskbar.
- **Exit Fullscreen (`enable = 0`)**:
  - Restores previous placement (`saved_placement`, defaulting to `Floating`).
  - Restores floating rect if previously floating, or recomputes snapped bounds within the work area if previously snapped.
  - Advances frame generation counter and invalidates display regions for immediate redrawing.
---

## GpuFlush Kernel Syscall

Internal to the compositor; cells should not call this directly.

| Field | Description |
|-------|-------------|
| Syscall ID | 300 (`ViSyscall::GpuFlush`) |
| a0 | data_ptr — pointer to BGRA8888 pixel buffer |
| a1 | data_len — byte length (must equal w×h×4) |
| a2 | xy packed — `(x & 0xFFFF) << 16 | (y & 0xFFFF)` |
| a3 | wh packed — `(w & 0xFFFF) << 16 | (h & 0xFFFF)` |

The kernel validates `data_len >= w*h*4`, copies pixels row-by-row into the VirtIO
GPU backing buffer, then calls `gpu.flush()`.

---

## Render Loop

The compositor renders at up to **30 FPS** (≈ 33 ms interval at 10 MHz mtime).
On each tick:

1. Collect the union of all surface `damage` rects.
2. Re-blit all surfaces overlapping the dirty union (bottom → top).
3. Call `sys_gpu_flush(dirty_rect)`.
4. Clear all surface damage flags.

A tick is skipped if no surface has a non-empty damage rect.

---

## Screen Resolution

Default: **1024 × 768** (`FALLBACK_WIDTH` × `FALLBACK_HEIGHT` in `api::display`).

The kernel probes the actual resolution from VirtIO GPU `GET_DISPLAY_INFO` during
`init_driver()`.  The compositor cell queries `GET_SCREEN_SIZE` on startup to size
its framebuffer.

---

## GPU Driver Cell

`cells/drivers/gpu/src/lib.rs` provides two helper functions for cells that need
direct GPU access (rare; normally cells go through the compositor):

```rust
// Flush a BGRA8888 region from a cell-owned buffer.
pub fn flush_rect(pixels: &[u8], rect: Rect) -> ViResult<()>

// Fill a solid colour without managing a pixel buffer.
pub fn fill_rect(rect: Rect, rgba: u32) -> ViResult<()>
```

---

## Files

| File | Purpose |
|------|---------|
| `libs/api/src/display.rs` | `Rect`, `PixelFormat`, `SurfaceCap`, opcode constants |
| `libs/api/src/services/display.rs` | `CreateViewRequest`, `ViewOpResponse`, `ViewRef`, `FrameRef`, `ViewOpResult`, view opcodes |
| `cells/services/compositor/src/surface_table/frame.rs` | `FrameRegistry`, `Frame`, `View`, capacity limits, generational tracking, snap math, fullscreen restoration |
| `cells/services/compositor/src/message_handler/view.rs` | Authenticated view IPC handler with validation, focus routing, dirty tracking |
| `cells/services/compositor/src/lib.rs` | IPC receive loop + message dispatch |
| `cells/services/compositor/src/surface_table.rs` | CapId → surface state map |
| `cells/services/compositor/src/z_order.rs` | Ordered paint list |
| `cells/services/compositor/src/render.rs` | Blending + damage-driven GPU flush |
| `cells/drivers/gpu/src/lib.rs` | `flush_rect` / `fill_rect` helpers |
| `kernel/src/task/drivers/virtio_gpu.rs` | VirtIO GPU init + `GpuFlush` handler |
| `tests/integration/compositor_basic.rs` | QEMU-driven smoke tests |
