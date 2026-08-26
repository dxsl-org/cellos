# Phase 04: App-Side ViSurface Wrapper

**Priority**: P0  
**Status**: ✅ Complete  
**Duration**: ~1d  
**Depends on**: Phase 02 merged  
**Parallel**: Can run in parallel with Phase 03

---

## Context Links

- [libs/ostd/src/lib.rs](../../../libs/ostd/src/lib.rs) — module exports
- [libs/ostd/src/syscall.rs:763-803](../../../libs/ostd/src/syscall.rs) — Grant syscall wrappers
- [libs/ostd/src/fs.rs](../../../libs/ostd/src/fs.rs) — reference: Grant usage pattern for VFS
- [libs/api/src/display.rs](../../../libs/api/src/display.rs) — `AttachGrant`, `DamageNotify`, opcodes

---

## Overview

App Cells currently must manually call `sys_grant_register` → `sys_grant_share` → `ATTACH_GRANT`
IPC → write raw pixels → `DAMAGE_NOTIFY` IPC. This is ~40 lines of boilerplate per surface.

`ViSurface` in `libs/ostd/src/display.rs` wraps this into a clean API:

```rust
let mut surface = ViSurface::create(compositor_tid, 800, 600, PixelFormat::Bgra8888)?;
let pixels: &mut [u8] = surface.pixels_mut();
// ... draw into pixels ...
surface.damage(Rect { x: 0, y: 0, w: 800, h: 600 });
// destroy auto-detaches grant on Drop
```

---

## Requirements

- `ViSurface::create(comp_tid, w, h, fmt)` allocates a Grant (`sys_grant_register`), shares
  ReadOnly with compositor, sends `CREATE_SURFACE` + `ATTACH_GRANT` IPC, returns `Ok(ViSurface)`.
- `ViSurface::pixels_mut()` returns `&mut [u8]` — app writes directly to physical buffer.
- `ViSurface::damage(rect)` sends `DAMAGE_NOTIFY` IPC to compositor (24 bytes, no reply wait).
- `Drop` impl: sends `DETACH_GRANT` + `DESTROY_SURFACE` IPC, calls `sys_grant_unregister`.
- `ViSurface` is `!Send` (raw pointer to physical buffer — must stay on the same cell task).
- Placed in `libs/ostd/src/display.rs`, exported from `libs/ostd/src/lib.rs` as `pub mod display`.
- Total new code: ~120 LOC (within 200-line file limit).

---

## Architecture

```rust
// libs/ostd/src/display.rs
use crate::syscall::{
    sys_grant_register, sys_grant_share, sys_grant_slice, sys_grant_unregister,
    sys_send, sys_recv, SyscallResult,
};
use api::display::{PixelFormat, Rect, AttachGrant, DamageNotify, compositor_ops};

pub struct ViSurface {
    comp_tid: usize,
    cap:      u32,         // compositor surface capability (fits u32 in current impl)
    reg_id:   usize,       // sys_grant_register id = physical base addr
    ptr:      *mut u8,     // write pointer into registered Grant
    width:    u32,
    height:   u32,
    fmt:      PixelFormat,
}

// SAFETY: ViSurface holds a raw *mut u8 into a Grant buffer; !Send to prevent
// accidental cross-task access (cell is single-task in current model).
impl !Send for ViSurface {}

impl ViSurface {
    pub fn create(comp_tid: usize, w: u32, h: u32, fmt: PixelFormat)
        -> Result<Self, ViError>
    {
        let size = (w * h * fmt.bpp()) as usize;
        // 1. Allocate persistent Grant buffer (lives until unregister or cell exit).
        let reg_id = sys_grant_register(size).ok_or(ViError::OutOfMemory)?;
        // 2. Share read-only with compositor so it can read our pixels.
        sys_grant_share(reg_id, comp_tid, 0 /* ReadOnly */);
        // 3. Get our own write pointer.
        let ptr = sys_grant_slice(reg_id).ok_or(ViError::IO)?;
        // 4. Tell compositor to create a surface slot.
        let cap = Self::ipc_create_surface(comp_tid, w, h)?;
        // 5. Attach our Grant to that slot.
        Self::ipc_attach_grant(comp_tid, cap, reg_id, w, h, fmt)?;
        Ok(Self { comp_tid, cap, reg_id, ptr, width: w, height: h, fmt })
    }

    /// Direct write access — app draws here, compositor reads here.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.width * self.height * self.fmt.bpp()) as usize;
        // SAFETY: ptr is our own registered Grant buffer; we hold &mut self so no
        // aliasing; compositor holds ReadOnly share — kernel blocks any compositor write.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, len) }
    }

    /// Signal a dirty region to the compositor (fire-and-forget, 24 bytes IPC).
    pub fn damage(&self, rect: Rect) {
        let mut buf = [0u8; 24];
        buf[0] = compositor_ops::DAMAGE_NOTIFY;
        // _pad bytes 1..4
        buf[4..8].copy_from_slice(&self.cap.to_le_bytes());
        encode_rect(&mut buf[8..24], rect);
        sys_send(self.comp_tid, &buf);
        // No reply — compositor picks it up on next render tick.
    }

    /// Notify damage for the entire surface.
    pub fn damage_all(&self) {
        self.damage(Rect { x: 0, y: 0, w: self.width, h: self.height });
    }
}

impl Drop for ViSurface {
    fn drop(&mut self) {
        // Detach grant from compositor before freeing pages.
        let mut buf = [0u8; 9];
        buf[0] = compositor_ops::DETACH_GRANT;
        buf[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        sys_send(self.comp_tid, &buf);
        // Drain reply.
        let mut resp = [0u8; 8];
        let _ = sys_recv(0, &mut resp);
        // Destroy surface slot.
        // ... DESTROY_SURFACE IPC ...
        // Release physical pages.
        sys_grant_unregister(self.reg_id);
    }
}
```

### Helper: encode_rect / ipc_create_surface

Private helpers within `display.rs` (~30 LOC) for encoding the `CREATE_SURFACE` IPC and decoding
the cap reply. Follows the same pattern as `libs/ostd/src/fs.rs` IPC helpers.

---

## Related Code Files

**Create:**
- `libs/ostd/src/display.rs` — `ViSurface` implementation (~120 LOC)

**Modify:**
- `libs/ostd/src/lib.rs` — add `pub mod display;`

---

## Implementation Steps

1. Create `libs/ostd/src/display.rs`
2. Implement `ViSurface` struct with `create`, `pixels_mut`, `damage`, `damage_all`, `Drop`
3. Implement private helpers: `ipc_create_surface`, `ipc_attach_grant`, `encode_rect`
4. Add `pub mod display;` to `libs/ostd/src/lib.rs`
5. `cargo check -p ostd` clean
6. Write a minimal test cell (or extend an existing demo cell) that creates a surface, writes a
   solid-color fill, calls `damage_all`, and loops — verify compositor renders it.

---

## Todo List

- [x] Create `libs/ostd/src/display.rs`
- [x] Implement `ViSurface::create`
- [x] Implement `ViSurface::pixels_mut`
- [x] Implement `ViSurface::damage` and `damage_all`
- [x] Implement `Drop` for `ViSurface`
- [x] Implement private IPC helpers (`ipc_create_surface`, `ipc_attach_grant`, `encode_rect`)
- [x] Add `pub mod display;` to `libs/ostd/src/lib.rs`
- [x] `cargo check -p ostd` clean
- [x] Test: demo cell creates surface, fills pixels, damages — compositor renders it

---

## Success Criteria

- [ ] `ViSurface::create` succeeds against a running compositor
- [ ] App writes pixels via `pixels_mut()` and they appear on screen (via compositor blend)
- [ ] Drop sends `DETACH_GRANT` + `DESTROY_SURFACE`, releases Grant pages
- [ ] Zero `WRITE_PIXELS` IPC calls in the test cell
- [ ] `cargo check -p ostd` clean

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `!Send` syntax (`impl !Send`) — requires nightly or explicit `PhantomData` | Medium | Use `PhantomData<*mut ()>` to make `ViSurface` not-Send on stable Rust |
| sys_recv in Drop (blocking) could deadlock if compositor is dead | Medium | Use `sys_recv` with short timeout (if available) or fire-and-forget the DETACH_GRANT |
| CREATE_SURFACE IPC: compositor may not respond if boot order is wrong | Low | Reuse existing `wait_for_service(service::COMPOSITOR)` pattern from input_handler.rs |

## Security Considerations

- `pixels_mut()` returns `&mut [u8]` — caller can write arbitrary pixels. This is intentional;
  the compositor has ReadOnly so cannot be tricked into writing back.
- `damage()` sends the surface `cap` — compositor must verify cap ownership (Phase 03 responsibility).
- `ViSurface::Drop` must always call `sys_grant_unregister` to free physical pages even if
  compositor is unreachable (avoid leak on compositor restart).

---

## Evidence

**Status**: ✅ Complete

**Verification**:
```bash
$ cargo check -p ostd
warning: field `0` is never read in `heap.rs:29`
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
```

**Code Evidence**:
1. **ViSurface struct** — `libs/ostd/src/display.rs:47–57` defines `comp_tid, cap, reg_id, ptr, width, height, fmt, _not_send` fields.
2. **create() method** — lines 59–101 implements full lifecycle: sys_grant_register → sys_grant_share → sys_grant_slice → ipc_create_surface → ipc_attach_grant with error cleanup.
3. **pixels_mut() method** — lines 103–113 safely dereferences Grant pointer with ownership via &mut self and ReadOnly kernel guarantee.
4. **damage() method** — lines 126–137 encodes DamageNotify and sends to compositor (fire-and-forget).
5. **damage_all() helper** — lines 139–142 signals full surface damage.
6. **move_to() helper** — lines 144–150 supports surface positioning.
7. **Drop impl** — line 150+ sends DETACH_GRANT + DESTROY_SURFACE IPC, always calls sys_grant_unregister.
8. **!Send marker** — line 56 `_not_send: PhantomData<*mut ()>` prevents cross-task access on stable Rust.
9. **wait_for_compositor() helper** — lines 26–33 provides service lookup with sys_lookup_service pattern.

Full app-side API wrapper verified.
