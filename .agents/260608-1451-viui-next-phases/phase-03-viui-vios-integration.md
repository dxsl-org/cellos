# Phase 03 — ViUI ↔ ViOS Integration

**Status:** Planned  
**Stage:** G1  
**Priority:** High  
**Estimate:** 2-3 ngày  
**Depends on:** input service cell, compositor cell, embedded/robot P01-P03 (DONE)  
**Can run parallel with:** P04, P05 (chạy trên files khác nhau)

---

## Context Links

- [`libs/viui/src/event.rs`](../../../libs/viui/src/event.rs) — viui::Event enum
- [`libs/viui/src/app_runner.rs`](../../../libs/viui/src/app_runner.rs) — ViApp::tick_with_dt
- [`libs/viui/src/renderer.rs`](../../../libs/viui/src/renderer.rs) — ViRenderer trait + FramebufferRenderer
- [`cells/apps/viui-demo/src/main.rs`](../../../cells/apps/viui-demo/src/main.rs) — current demo pattern
- [`cells/services/input/`](../../../cells/services/input/) — input service IPC protocol
- [`cells/services/compositor/`](../../../cells/services/compositor/) — compositor surface API
- [`docs/specs/06-graphics.md`](../../../docs/specs/06-graphics.md) — compositor surface spec

---

## Overview

ViUI hiện tại chạy với **simulated events** (viui-demo tự tạo event list). Real apps cần:

1. **Real input events**: keyboard key presses + repeats, touchscreen contacts từ input service cell
2. **Real compositor surface**: draw lên ViSurface thay vì fake framebuffer
3. **Event rate limiter**: input service có thể flood events (e.g. mouse move) → cần batch/debounce

Sau phase này, viui-demo và robot-dashboard nhận events thực sự từ input cell, hiển thị trên compositor surface đúng cách.

---

## Part A — Input Service Event Mapping

### Input service IPC protocol (đọc từ cells/services/input/)

Input service gửi messages dạng:
```
[event_type: u8] [payload bytes...]
```

Cần map sang `viui::Event`. Tạo `libs/viui/src/input_bridge.rs`:

```rust
//! Converts raw input-service IPC bytes to viui::Event.

use crate::event::{Event, KeyCode, Modifiers, MouseButton};
use crate::layout::Point;

/// Raw input-service message types (must match input service protocol).
#[repr(u8)]
enum InputMsgType {
    MouseMove    = 0x01,
    MouseDown    = 0x02,
    MouseUp      = 0x03,
    Scroll       = 0x04,
    KeyDown      = 0x10,
    KeyUp        = 0x11,
    KeyChar      = 0x12,
    TouchBegin   = 0x20,
    TouchMove    = 0x21,
    TouchEnd     = 0x22,
}

pub fn parse_input_message(buf: &[u8]) -> Option<Event> {
    if buf.is_empty() { return None; }
    match buf[0] {
        0x01 => {
            let (x, y) = read_f32_pair(&buf[1..])?;
            Some(Event::MouseMove { pos: Point::new(x, y) })
        }
        0x02 => {
            let (x, y) = read_f32_pair(&buf[1..])?;
            let button = parse_mouse_button(buf.get(9).copied()?);
            Some(Event::MousePress { pos: Point::new(x, y), button })
        }
        // ... etc.
        _ => None,
    }
}
```

### Key repeat handling

Input service gửi `KeyDown` events. ViUI cần synthetic key repeat:

```rust
// app_runner.rs — thêm KeyRepeatState
struct KeyRepeatState {
    held_key:       Option<KeyCode>,
    modifiers:      Modifiers,
    held_since_ms:  u64,
    last_repeat_ms: u64,
}

const KEY_REPEAT_DELAY_MS:    u64 = 500;
const KEY_REPEAT_INTERVAL_MS: u64 = 50;
```

Trong `tick_with_dt()`:
1. Xử lý normal events (bao gồm KeyPress/KeyRelease)
2. Sau khi xử lý events, check repeat state
3. Nếu key đang held + thời gian qua delay → inject synthetic `KeyPress` events

### Modifier tracking

Lưu `Modifiers` state trong `EventCtx` hoặc `ViApp`:

```rust
// event.rs — EventCtx hiện tại có gì?
// Nếu chưa có modifier tracking, thêm:
pub struct EventCtx<'a> {
    pub focus:       &'a mut FocusManager,
    pub state:       &'a mut WidgetStateStore,
    pub modifiers:   Modifiers,   // ← ADD
}
```

---

## Part B — Compositor Surface Binding

### Current state

`viui-demo` dùng:
```rust
let renderer = FramebufferRenderer::new(raw_framebuffer_ptr, width, height, stride);
```

Cần wrap ViSurface (compositor IPC) vào `ViRenderer` implementation.

### ViSurfaceRenderer

Tạo `libs/viui/src/surface_renderer.rs`:

```rust
//! Adapter: ViOS compositor surface → ViRenderer.
//!
//! ViSurface allocates a shared Grant buffer (compositor Grant API);
//! flush() calls compositor IPC to present the frame.

use crate::renderer::ViRenderer;
use crate::canvas::{FramebufferCanvas, Color};
use crate::layout::Rect;

pub struct ViSurfaceRenderer {
    canvas: FramebufferCanvas,
    // surface_handle: ViSurfaceHandle (opaque IPC handle)
}

impl ViSurfaceRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        // 1. Call compositor IPC: CreateSurface(width, height)
        // 2. Map returned Grant buffer as framebuffer
        // 3. Wrap in FramebufferCanvas
        todo!()
    }
}

impl ViRenderer for ViSurfaceRenderer {
    fn canvas(&mut self) -> &mut dyn crate::canvas::ViCanvas {
        &mut self.canvas
    }

    fn present(&mut self, dirty: Option<Rect>) {
        // Call compositor IPC: PresentSurface(dirty_rect)
    }
}
```

### Compositor IPC protocol

Đọc `docs/specs/06-graphics.md` + compositor cell source để biết exact IPC format.

Phase này: implement đủ để viui-demo dùng compositor surface thay vì raw framebuffer.
Nếu compositor Grant redesign (`.agents/260607-1854-compositor-grant-surfaces/`) chưa merge → wrap raw framebuffer đơn giản, add TODO.

---

## Part C — Input Event Collection Helper

Tạo helper function để apps không cần tự viết IPC loop:

```rust
// libs/viui/src/input_bridge.rs (extend)

/// Collect all pending input events from input service IPC endpoint.
///
/// Returns up to `max_events` events. Call once per frame before `ViApp::tick_with_dt`.
pub fn collect_input_events(max_events: usize) -> alloc::vec::Vec<Event> {
    let mut events = alloc::vec::Vec::with_capacity(max_events);
    // sys_recv loop (non-blocking, returns Err if no message)
    while events.len() < max_events {
        match sys_recv_nonblocking(INPUT_SERVICE_ID) {
            Ok(msg) => {
                if let Some(e) = parse_input_message(&msg) {
                    events.push(e);
                }
            }
            Err(_) => break,
        }
    }
    events
}
```

Apps:
```rust
// Before:
let events: Vec<Event> = Vec::new(); // empty!

// After:
let events = viui::input_bridge::collect_input_events(64);
app.tick_with_dt(&events, dt);
```

---

## Part D — viui-demo update

Update `viui-demo/src/main.rs` để dùng:
1. `ViSurfaceRenderer` thay vì raw `FramebufferRenderer`
2. `collect_input_events()` thay vì empty event list
3. Verify keyboard focus: Tab focus, Enter activate button, ←→ move slider

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/input_bridge.rs` | CREATE |
| `libs/viui/src/surface_renderer.rs` | CREATE |
| `libs/viui/src/lib.rs` | MODIFY — expose input_bridge, surface_renderer |
| `libs/viui/src/app_runner.rs` | MODIFY — KeyRepeatState, modifier tracking |
| `libs/viui/src/event.rs` | MODIFY — add modifiers field to EventCtx if missing |
| `cells/apps/viui-demo/src/main.rs` | MODIFY — use real input + surface renderer |

---

## Implementation Steps

**Day 1 — Input bridge:**
1. Đọc input service cell source: xác định exact IPC message format
2. Tạo `input_bridge.rs`: parse_input_message + collect_input_events
3. Thêm KeyRepeatState vào app_runner.rs
4. Add modifier tracking in EventCtx if missing
5. cargo check

**Day 2 — Compositor surface:**
1. Đọc compositor cell source + `docs/specs/06-graphics.md`
2. Tạo `surface_renderer.rs` (implement ViRenderer)
3. If compositor Grant API ready: wire full Grant buffer path
4. Else: wrap raw framebuffer with IPC present call + TODO
5. Update viui-demo to use ViSurfaceRenderer
6. cargo check

**Day 3 — Integration test:**
1. Boot ViOS QEMU với input + compositor cells running
2. Test viui-demo: keyboard input (Tab/Enter), mouse click
3. Verify key repeat (hold key → repeated events)
4. Verify modifier: Shift+key sends correct Modifiers
5. Verify compositor surface flush (no tearing, correct dirty rect)

---

## Todo

- [ ] Đọc input service IPC format
- [ ] Tạo input_bridge.rs: parse_input_message
- [ ] Tạo input_bridge.rs: collect_input_events
- [ ] app_runner.rs: KeyRepeatState + inject repeat events in tick_with_dt
- [ ] event.rs: verify/add modifiers in EventCtx
- [ ] Đọc compositor source + spec
- [ ] Tạo surface_renderer.rs: ViSurfaceRenderer
- [ ] Update viui-demo: ViSurfaceRenderer + collect_input_events
- [ ] cargo check full workspace
- [ ] Boot test: keyboard input to viui-demo
- [ ] Boot test: compositor surface flush

---

## Success Criteria

- `collect_input_events(64)` nhận real mouse + key events từ input service
- Key repeat: hold Arrow key → focus moves repeatedly với 500ms delay, 50ms interval
- Shift modifier: `KeyPress { key: Char('a'), modifiers: Modifiers { shift: true } }` đúng
- viui-demo hiển thị trên compositor surface (không phải raw framebuffer)
- `cargo check` full workspace không warning

---

## Risk

**Input service IPC format**: cần đọc source carefully. Nếu format thay đổi → input_bridge.rs sẽ fail. Add `#[cfg(test)]` roundtrip test với known byte sequences để catch regressions.

**Compositor Grant API**: compositor Grant redesign plan đang deferred. Nếu chưa có Grant buffer surface → implement raw framebuffer wrapper first, document TODO clearly.

**Key repeat vs. held detection**: input service có thể đã gửi repeat events (OS-style) → không cần synthetic repeat. Kiểm tra input service source trước khi implement synthetic repeat.
