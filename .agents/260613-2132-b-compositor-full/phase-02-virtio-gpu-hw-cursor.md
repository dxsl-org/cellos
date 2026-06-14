# Phase 02 — VirtIO GPU Hardware Cursor

## Context Links
- Plan: [plan.md](plan.md)  ·  Depends on: [Phase 01](phase-01-software-cursor-sprite.md)
- Driver: `kernel/src/task/drivers/virtio_gpu.rs`
- GpuFlush handler: `kernel/src/task/syscall.rs:1654`
- Syscall ids: `libs/api/src/syscall.rs:70` (`GpuFlush = 300`)
- ostd wrapper: `libs/ostd/src/syscall.rs:688`

## Overview
- **Priority:** P2.
- **Status:** ✅ complete.
- **Description:** Offload the cursor to the GPU scanout via VirtIO GPU
  `UPDATE_CURSOR`/`MOVE_CURSOR`, so a mouse move does not force a full-frame
  software re-render. Software cursor (Phase 01) remains the fallback when the
  GPU cursor is unavailable.

## Key Insights (verified)
- **virtio-drivers 0.7.5 already implements the hardware cursor**:
  - `VirtIOGpu::setup_cursor(image: &[u8], pos_x, pos_y, hot_x, hot_y)`
    (`gpu.rs:137`) — sprite MUST be exactly `CURSOR_RECT` = **64×64×4 bytes**
    (`gpu.rs:145-148` returns `InvalidParam` otherwise). Allocates a DMA page,
    does RESOURCE_CREATE_2D + ATTACH_BACKING + TRANSFER_TO_HOST_2D + UPDATE_CURSOR.
  - `VirtIOGpu::move_cursor(pos_x, pos_y)` (`gpu.rs:170`) — MOVE_CURSOR only,
    no shape upload. Cheap.
- So the kernel work is wiring two new syscall ops to `GPU_CONTEXT.gpu`, NOT
  implementing the VirtIO cursor protocol from scratch.
- `GpuContext.gpu` is `pub` (`virtio_gpu.rs:10`), reachable under the spinlock.

## Requirements
### Functional
1. Kernel: a way to (a) set the cursor sprite once (64×64), (b) move it per
   MouseMove. Two options — **decision required**:
   - **Option A (preferred, minimal Law-1 surface):** extend `GpuFlush` op-style:
     keep id 300 but encode a sub-op. ✗ `GpuFlush` has no spare op field (it is
     `{data_ptr,data_len,xy,wh}`); overloading is fragile.
   - **Option B:** add a new syscall `GpuCursor = 301` with `{op, data_ptr,
     xy, hot}` where `op=0` = set sprite (data_ptr→64×64 BGRA), `op=1` = move
     (xy only). **This is a `libs/api/` change → Law 1: 2× user confirmation.**
   Plan adopts **Option B** (clean, one syscall, op-multiplexed).
2. Compositor: on `MouseMove`, call `sys_gpu_cursor(move, x, y)` instead of
   forcing a software repaint — *when the HW cursor is active*. Fall back to the
   Phase 01 software path if `setup_cursor` failed at init.
3. Compositor uploads the 64×64 sprite once at startup (op=set). If that syscall
   returns an error (GPU has no cursor / not initialised), set a flag and use the
   Phase 01 software cursor for the session.

### Non-functional
- Kernel cursor code lives in a new `virtio_gpu/cursor.rs` submodule (Law 5:
  split `virtio_gpu.rs` → `virtio_gpu/` dir; `cursor.rs` + later `blit.rs` for
  Phase 03 to keep file ownership disjoint). `// SAFETY:` on any unsafe.
- Compositor stays unsafe-free (Law 4).

## Architecture
### Syscall (Law 1 — confirm before editing libs/api)
```
GpuCursor = 301
  a0 op    : 0 = set sprite, 1 = move
  a1 ptr   : (op=0) ptr to 64*64*4 BGRA sprite; (op=1) unused
  a2 xy    : (x<<16)|y  pos
  a3 hot   : (op=0) (hot_x<<16)|hot_y; (op=1) unused
```
Add to `ViSyscall` enum (`libs/api/src/syscall.rs`), `from_u16` (:390), cost
weight (:295). ostd wrapper `sys_gpu_cursor(op,ptr,x,y,hot_x,hot_y)`
(`libs/ostd/src/syscall.rs`). Kernel dispatch arm in `syscall.rs` next to
`GpuFlush` (:1654), delegating to `virtio_gpu::cursor::{set_sprite,move_to}`.

### Kernel handler
```
set_sprite(image: &[u8], x,y, hot_x,hot_y):
   GPU_CONTEXT.lock().as_mut() → ctx.gpu.setup_cursor(image, x,y, hot_x,hot_y)
move_to(x,y):
   GPU_CONTEXT.lock().as_mut() → ctx.gpu.move_cursor(x,y)
```
Validate `data_len == 64*64*4` for op=0 (return `BufferTooSmall` otherwise).

### Compositor
- At startup, after `connect_to_input`, build a 64×64 BGRA sprite (reuse the
  Phase 01 16×16 art, top-left, rest transparent) and call
  `sys_gpu_cursor(set, …)`. Store `hw_cursor: bool` in `InputState`.
- `update_cursor`: if `hw_cursor`, call `sys_gpu_cursor(move, x, y)` and do NOT
  set `pending_dirty` for the cursor. Else fall through to the Phase 01 software
  path (set `pending_dirty`).

## Related Code Files
- **Create:** `kernel/src/task/drivers/virtio_gpu/cursor.rs` — `set_sprite`,
  `move_to`. (Requires moving `virtio_gpu.rs` → `virtio_gpu/native.rs` or keeping
  `virtio_gpu.rs` beside a new `virtio_gpu/` dir per Law 5.)
- **Modify (Law 1, 2× confirm):** `libs/api/src/syscall.rs` — `GpuCursor = 301`.
- **Modify:** `libs/ostd/src/syscall.rs` — `sys_gpu_cursor` wrapper.
- **Modify:** `kernel/src/task/syscall.rs` — `Syscall::GpuCursor` variant + dispatch.
- **Modify:** `cells/services/compositor/src/input_handler.rs` — HW move path + flag.
- **Modify:** `cells/services/compositor/src/main.rs` — upload sprite at startup.
  NOTE: these compositor files are owned by Phase 01 → run 02 strictly AFTER 01.

## Implementation Steps
1. **Get 2× user confirmation** for the `libs/api/` change (Law 1) before editing.
2. Add `GpuCursor = 301` to `ViSyscall` + `from_u16` + cost weight.
3. ostd `sys_gpu_cursor(op, ptr, x, y, hot_x, hot_y)`.
4. Split `virtio_gpu.rs` per Law 5; add `cursor.rs` with `set_sprite`/`move_to`
   wrapping `ctx.gpu.setup_cursor`/`move_cursor`. Validate 64×64 len.
5. Kernel `Syscall::GpuCursor` dispatch arm (validate buf for op=0).
6. Compositor: upload 64×64 sprite at startup; set `hw_cursor` from the result;
   route MouseMove to `sys_gpu_cursor(move,…)` when `hw_cursor`.
7. `cargo check` workspace; build kernel with `RUSTFLAGS=-C relocation-model=pic`
   (per run.ps1) — verify it still boots to `ViCell >`.

## Todo List
- [ ] 2× confirm Law 1 (libs/api)
- [ ] GpuCursor = 301 enum + from_u16 + cost
- [ ] sys_gpu_cursor ostd wrapper
- [ ] virtio_gpu split + cursor.rs (set_sprite/move_to, len validate)
- [ ] kernel GpuCursor dispatch
- [ ] compositor: upload sprite at startup + hw_cursor flag
- [ ] compositor: MouseMove → move_cursor when hw_cursor
- [ ] boots to shell with GPU; cursor moves without full repaint

## Success Criteria
- **Observable:** with the HW cursor active, moving the mouse updates the cursor
  position on screen WITHOUT a `render_frame` full-frame flush (the software
  `[compositor] cursor at` repaint path is not taken).
- Kernel logs `[gpu] cursor sprite uploaded` once and boots to shell.
- Fallback verified: if `setup_cursor` errors, the Phase 01 software cursor draws.

## Risk Assessment
- **Sprite size mismatch (High→mitigated):** `setup_cursor` hard-requires exactly
  64×64×4 — wrong size → `InvalidParam`. Mitigation: build the sprite at exactly
  64×64 and assert `data_len` in the kernel handler.
- **Blocking control-queue request on the hot path (Med):** `move_cursor` issues
  a synchronous `add_notify_wait_pop`. Per-move it is one small request — fine for
  G2. If it stalls, fall back to software cursor. Do NOT call `setup_cursor`
  (DMA alloc) per move — only `move_cursor`.
- **Law 1 (Process):** adding syscall id 301 needs explicit 2× user confirmation
  — STOP and ask before editing `libs/api/`.
- **Spinlock contention with GpuFlush (Low):** cursor move and frame flush both
  take `GPU_CONTEXT.lock()`; both are short. Keep lock scope minimal.

## Security Considerations
- op=0 sprite pointer is a user buffer in the SAS: validate `data_len == 64*64*4`
  and read exactly that many bytes (`// SAFETY:` comment), mirroring the existing
  `GpuFlush` bounds discipline (`syscall.rs:1660-1683`).
- No capability gate today on `GpuFlush`; `GpuCursor` follows the same trust model
  (compositor is a trusted system cell). Note for future cap-gating.

## Next Steps
- Phase 04 may add a HW-cursor assertion (`[gpu] cursor sprite uploaded`).
- Phase 03 (DMA blit) shares `virtio_gpu/` — the Law-5 split here keeps files
  disjoint between 02 (`cursor.rs`) and 03 (`blit.rs`).
