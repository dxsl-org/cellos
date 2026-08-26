# Phase P01 — ViUI Core Engine

**Step**: 1 (Core)  
**Priority**: P0 — blocking tất cả phases sau  
**Status**: 📋 Planned  
**Effort est.**: 7-9 ngày  
**Algorithm refs**: iced (`Length`, `Limits`, `LayoutNode`) · egui (`Id` hash, `Memory`) · OrbTK (`measure/arrange` 2-pass, `State` lifecycle, BottomUp routing, dirty subtree) · embedded-graphics (P02)

---

## Context Links

- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) — Full ViUI spec
- [libs/ostd/src/display.rs](../../libs/ostd/src/display.rs) — ViSurface (dùng để render)
- [libs/api/src/display.rs](../../libs/api/src/display.rs) — DamageNotify protocol

---

## Overview

Xây dựng core engine của ViUI: `ViWidget` trait, `LayoutEngine`, `EventDispatch`, và `ViApp` Elm trait. Đây là foundation — không có implementation widget nào trong phase này, chỉ contracts và plumbing.

---

## Requirements

### Functional
- `ViWidget` trait: `layout()`, `paint()`, `event()`, `children()`
- `Length` enum: `Fill | Shrink | Fixed(f32) | FillPortion(u16)` — iced ref — responsive layout
- `LayoutEngine`: Constraints-based, Column/Row/Stack/Padding/SizedBox
- `WidgetId`: hash-based stable ID — egui ref — retained mode requires stable IDs across frames
- `WidgetStateStore`: per-widget hover/focus/drag state — egui `Memory` ref
- `EventDispatch`: route `Event` xuống widget tree theo hit-testing + focus
- `FocusManager`: Tab order, keyboard focus transfer
- `ViApp` trait: `view() -> Element<Msg>`, `update(Msg)` — iced-compatible shape
- `Element<Msg>`: type-erased widget container

### Non-functional
- `#![no_std]` + `alloc` only
- `#![forbid(unsafe_code)]` — Law 4
- Không có `mod.rs` — Law 5
- Public types: `Vi` prefix — Law 6
- Zero allocation trong `layout()` và `paint()` hot path

---

## Architecture

### ViWidget trait
```rust
pub trait ViWidget: 'static {
    fn layout(&self, cx: &LayoutCx, constraints: Constraints) -> Size;
    fn paint(&self, cx: &mut PaintCx);
    fn event(&mut self, cx: &mut EventCx, e: &Event) -> EventStatus;
    fn children(&self) -> &[Box<dyn ViWidget>] { &[] }
}
```

### Two-pass Layout — OrbTK ref: `Layout::measure` + `Layout::arrange`

OrbTK confirms the correct algorithm for a retained-mode UI layout engine:

```
Pass 1 — measure (bottom-up):
  Widget asks: "given available Constraints, how much space do I need?"
  Children are measured first; parent accumulates their sizes.
  Returns: desired_size (the widget's preferred size within constraints)

Pass 2 — arrange (top-down):
  Parent distributes actual space to children based on their desired_size.
  Children that declared Length::Fill get remaining space proportionally.
  Writes final bounds (x, y, w, h) into LayoutNode.
```

Two-pass is required because `Length::Fill` cannot be resolved until the parent knows how much space is left after all `Shrink/Fixed` children are measured. Single-pass layout cannot handle this correctly.

```rust
// libs/viui/src/layout.rs (new)
pub trait ViLayout {
    // Pass 1: measure children, return this widget's desired size
    fn measure(&self, widget: &dyn ViWidget, constraints: Constraints,
               state: &WidgetStateStore) -> Size;
    // Pass 2: write computed bounds into LayoutNode tree
    fn arrange(&self, widget: &dyn ViWidget, bounds: Rect,
               state: &WidgetStateStore) -> Vec<LayoutNode>;
}

// Built-in layouts (each widget picks one at construction)
pub struct StackLayout { pub orientation: Orientation, pub spacing: f32 }
pub struct FixedLayout;      // SizedBox — ignores children
pub struct PaddingLayout { pub padding: Padding }
pub struct AbsoluteLayout;   // explicit x/y children
```

`LayoutNode` stores the result of arrange:
```rust
pub struct LayoutNode {
    pub bounds:   Rect,            // final screen position + size
    pub children: Vec<LayoutNode>, // arranged child nodes
    pub dirty:    bool,            // OrbTK ref: dirty_widgets list
}
```

**Dirty subtree flag** (OrbTK `dirty_widgets: Vec<Entity>`): when a widget's state changes, mark `dirty = true` on its node and all ancestors up to root. Layout pass only re-measures dirty subtrees, not the entire tree. This is critical for performance when only a button label changes in a large form.

### `Length` — iced ref: `iced_core::Length`
```rust
// Cách widget khai báo nó muốn bao nhiêu space — identical shape với iced
pub enum Length {
    Fill,               // chiếm toàn bộ space còn lại trong axis
    Shrink,             // chỉ lấy đúng kích thước nội dung
    Fixed(f32),         // pixels cứng
    FillPortion(u16),   // chia tỉ lệ: FillPortion(2) lấy 2x so với FillPortion(1)
}
```
`Length` là input từ widget/user. `Constraints` là output từ LayoutEngine sau khi resolve:
```rust
// iced ref: iced_core::layout::Limits
pub struct Constraints { pub min: Size, pub max: Size }
pub struct Size { pub w: f32, pub h: f32 }
pub struct Point { pub x: f32, pub y: f32 }
pub struct Rect { pub origin: Point, pub size: Size }
```

### Event routing — OrbTK ref: BottomUp + Direct strategies

OrbTK confirms two routing modes are sufficient for all UI cases:

```
BottomUp (default — pointer/touch/mouse):
  1. Hit-test: walk tree top-down, collect all nodes whose bounds contain the pointer pos
  2. Route event to the deepest match (leaf) first
  3. Bubble toward root; stop if any handler returns EventStatus::Consumed
  Use for: MousePress, MouseRelease, MouseMove, Scroll

Direct (keyboard/focus events):
  1. Route directly to the focused widget (FocusManager holds the current WidgetId)
  2. No hit-test, no bubbling
  Use for: KeyPress, KeyRelease, Char
  ViCell mapping: focus-targeted key events = sys_send(focused_cell_tid, ...)

GlobalRelease (OrbTK: GlobalMouseUpEvent):
  1. Fire to ALL interactive widgets regardless of pointer position
  Use for: MouseRelease when pointer may have moved outside widget during drag
```

```rust
pub enum Event {
    // BottomUp
    MouseMove { pos: Point },
    MousePress { pos: Point, button: MouseButton },
    MouseRelease { pos: Point, button: MouseButton },  // also sent as Global
    Scroll { delta: f32 },
    // Direct (to focused widget)
    KeyPress { key: KeyCode, modifiers: Modifiers },
    KeyRelease { key: KeyCode },
    Char(char),
    // Lifecycle
    Focus, Blur,
}
pub enum EventStatus { Consumed, Ignored }
```

`EventCx` carries routing metadata:
```rust
pub struct EventCx<'a> {
    pub state:       &'a mut WidgetStateStore,
    pub focus:       &'a mut FocusManager,
    pub messages:    &'a mut alloc::vec::Vec<ErasedMessage>, // pending Elm messages
    pub widget_id:   WidgetId,    // current widget being dispatched to
    pub widget_rect: Rect,        // current widget's screen bounds (for hit test)
}
```

OrbTK insight: **behavior widgets** (`MouseBehaviorState`) send an internal `Action` message to themselves on event, processed in `update()`. In ViUI's Elm model this is natural — button's `event()` pushes `Message::Pressed` to `cx.messages` on click; the Elm runner delivers it to `ViApp::update()`.

### Event types (full definition above)

### `ViWidget` lifecycle — OrbTK ref: `State::init/update/cleanup/update_post_layout`

OrbTK's `State` trait reveals a critical lifecycle method missing from the basic `ViWidget`:

```rust
pub trait ViWidget: 'static {
    fn layout(&self, cx: &LayoutCx, constraints: Constraints) -> Size; // measure pass
    fn arrange(&self, cx: &mut ArrangeCx, bounds: Rect);               // arrange pass (new)
    fn paint(&self, cx: &mut PaintCx);
    fn event(&mut self, cx: &mut EventCx, e: &Event) -> EventStatus;

    // OrbTK ref: State::update_post_layout()
    // Called AFTER arrange(), BEFORE paint(). Widget knows its final bounds.
    // Use case: ScrollArea computes scroll clamp after knowing its final height;
    //           Label decides truncation after knowing its final width.
    fn post_layout(&mut self, _bounds: Rect) {}   // default no-op

    // OrbTK ref: State::init() / State::cleanup()
    // RAII — Law 8. init() called when widget inserted into tree.
    // cleanup() called when removed. Map to Drop-based cleanup where possible.
    fn on_mount(&mut self) {}
    fn on_unmount(&mut self) {}

    fn children(&self) -> &[Box<dyn ViWidget>] { &[] }
}
```

`post_layout` is not needed by most widgets — the default no-op is fine. It's essential for:
- **ScrollArea**: clamping `scroll_y` after knowing actual visible height
- **TextEdit**: computing cursor pixel position after knowing actual text box width
- **any widget that positions a child relative to its own computed size**

### `WidgetId` — egui ref: `egui::Id`
```rust
// Hash-based stable ID — KHÔNG auto-increment
// egui dùng hash(location_in_source + user_suffix) để ID ổn định
// qua nhiều frames dù widget tree thay đổi cấu trúc
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WidgetId(u64);

impl WidgetId {
    // Primary constructor: hash từ stable string (module path + user label)
    pub fn new(salt: &str) -> Self {
        Self(hash_str(salt))
    }
    // Chain: tạo child ID từ parent (cho widgets trong list)
    pub fn with(self, index: usize) -> Self {
        Self(self.0.wrapping_add(hash_usize(index)))
    }
}
// hash_str: FNV-1a 64-bit (no_std, no alloc)
```

**Tại sao không auto-increment**: nếu widget tree insert một node ở đầu, toàn bộ IDs sau đó drift → state store bị orphan hoặc sai widget. Hash-based IDs stable kể cả khi tree reorder.

### `WidgetStateStore` — egui ref: `egui::Memory`
```rust
// Per-widget transient state (hover, pressed, drag offset, scroll pos)
// egui gọi là Memory { data: HashMap<Id, Box<dyn Any>> }
// ViUI dùng BTreeMap vì no_std (không có HashMap)
pub struct WidgetStateStore {
    inner: BTreeMap<WidgetId, WidgetState>,
}

/// Bit flags for interactive widget visual state.
///
/// Single u8 — state is combinatorial (widget can be hovered+focused simultaneously).
/// P04 OrbTK WidgetFlags pattern: Button reads flags instead of storing state in struct.
pub struct WidgetFlags(pub u8);
impl WidgetFlags {
    pub const HOVERED: u8 = 0b001;
    pub const PRESSED: u8 = 0b010;
    pub const FOCUSED: u8 = 0b100;
    pub fn has(&self, f: u8) -> bool { self.0 & f != 0 }
    pub fn set(&mut self, f: u8) { self.0 |= f; }
    pub fn clear(&mut self, f: u8) { self.0 &= !f; }
}

pub struct WidgetState {
    pub flags:    WidgetFlags,    // hovered/pressed/focused bitfield
    pub drag_pos: Option<Point>,
    pub scroll_y: f32,            // ScrollArea scroll offset
    pub custom:   [u8; 32],       // widget-specific state (e.g. TextEdit cursor byte offset)
}

impl WidgetStateStore {
    pub fn get(&self, id: WidgetId) -> &WidgetState;
    pub fn get_mut(&mut self, id: WidgetId) -> &mut WidgetState;
    // Gọi cuối frame để xoá state của widgets không còn trong tree
    pub fn gc(&mut self, live_ids: &[WidgetId]);
}
```

`WidgetStateStore` sống trong `WidgetTree`, được truyền vào `EventCx` để widgets đọc/ghi state của mình.

### `ViApp` trait (Elm)
```rust
pub trait ViApp: 'static + Sized {
    type Message: 'static;
    fn view(&self) -> Element<Self::Message>;
    fn update(&mut self, msg: Self::Message);
    fn title(&self) -> &str { "ViCell App" }
}
```

### `PaintCx` — full definition

```rust
// libs/viui/src/widget.rs
// PaintCx is the rendering context passed to ViWidget::paint().
// It carries the canvas + font resources — widgets never own these.
pub struct PaintCx<'a> {
    pub canvas:  &'a mut dyn ViCanvas,
    pub origin:  Point,                            // widget's top-left in screen coords
    pub theme:   &'a dyn ViTheme,                  // from P05 — default DarkTheme until then
    // Text rendering: both fields or neither. PaintCx owns the font + atlas for the frame.
    // Widgets call cx.layout_text() / cx.paint_text() — never reach into atlas directly.
    pub font:    Option<&'a fontdue::Font>,         // None = bitmap-only mode
    pub atlas:   Option<&'a mut GlyphAtlas>,        // None = bitmap-only mode
}
```

**Tại sao font/atlas ở PaintCx, không phải FramebufferCanvas**: một App Cell có thể dùng nhiều canvas (main surface + off-screen buffer). Font là singleton, không phải per-canvas. PaintCx là entry point của một paint pass — nó tổng hợp canvas + font + theme cho một frame, rồi hủy sau khi frame xong.

P03 `draw_text` atlas path: widgets gọi `cx.paint_text(lt, pos, color)` sau khi measure bằng `cx.layout_text(text, size_px)` — không gọi canvas.draw_text với atlas trực tiếp.

### WidgetTree — Elm rebuild strategy

```rust
// libs/viui/src/widget.rs
pub struct WidgetTree {
    // iced strategy: view() is called each time state changes → new Element tree.
    // WidgetTree rebuilds from the new Element on each update cycle.
    // WidgetStateStore (egui Memory) persists ACROSS rebuilds by WidgetId hash.
    root:    Box<dyn ViWidget>,
    // LayoutNode is a pure tree (no flat map) — avoids dual-representation bug.
    // iced ref: iced_core::layout::Node (tree, not map)
    layout:  LayoutNode,      // root layout node; children are recursive
    state:   WidgetStateStore, // persists across view() rebuilds
    dirty:   Option<Rect>,
    focus:   FocusManager,
}

// Single layout tree — no BTreeMap double-representation.
pub struct LayoutNode {
    pub bounds:    Rect,
    pub children:  alloc::vec::Vec<LayoutNode>,
}

impl WidgetTree {
    pub fn rebuild(root: Box<dyn ViWidget>) -> Self;    // called after app.update()
    pub fn dispatch_event(&mut self, e: &Event);
    pub fn layout(&mut self, available: Size);           // 2-pass measure+arrange
    pub fn paint(&self, cx: &mut PaintCx);
    pub fn take_dirty(&mut self) -> Option<Rect>;
}
```

**Rebuild vs retain**: `WidgetTree::rebuild()` called after each `app.update(msg)` — cheap because it just sets a new root pointer. `layout()` re-runs on next frame only if `dirty` flag is set. `WidgetStateStore` (keyed by FNV hash) survives rebuild because WidgetIds are hash-stable.

---

## Related Code Files

**Create**:
- `libs/viui/Cargo.toml`
- `libs/viui/src/lib.rs`
- `libs/viui/src/widget.rs`       — ViWidget trait, WidgetId (hash-based)
- `libs/viui/src/layout.rs`       — Length, Constraints, Size, Point, Rect, LayoutCx, LayoutNode
- `libs/viui/src/state_store.rs`  — WidgetStateStore, WidgetState
- `libs/viui/src/event.rs`        — Event enum, EventStatus, EventCx, FocusManager
- `libs/viui/src/response.rs`     — Response
- `libs/viui/src/elm.rs`          — ViApp trait, Element<Msg>
- `libs/viui/src/prelude.rs`

**Modify**:
- `Cargo.toml` (workspace) — add `libs/viui` member

---

## Implementation Steps

1. `libs/viui/Cargo.toml` — `no_std`, `alloc`, embedded-graphics + fontdue deps
2. `widget.rs` — `ViWidget` trait với `measure/arrange/paint/event/post_layout/on_mount/on_unmount`; `WidgetId(u64)` với `new(salt)` + `with(index)` (FNV-1a)
3. `layout.rs` — `Length`; `Constraints`; `Size/Point/Rect`; `ViLayout` trait (measure+arrange 2-pass); `LayoutNode { bounds, children, dirty }`; `StackLayout/FixedLayout/PaddingLayout`
4. `state_store.rs` — `WidgetState` struct; `WidgetStateStore` với BTreeMap; `gc(live_ids)`
5. `event.rs` — `Event` enum; `EventStatus`; `EventCx` (carries `&mut WidgetStateStore`); `FocusManager`
6. `response.rs` — `Response` (clicked/hovered/changed + rect + id)
7. `elm.rs` — `ViApp` trait; `Element<Msg>` (Box<dyn ErasedWidget<Msg>>)
8. `lib.rs` + `prelude.rs`
9. `WidgetTree` trong `widget.rs`: `build`, `dispatch_event`, `layout`, `paint`, `take_dirty`
10. Add `viui` to workspace `Cargo.toml`
11. `cargo check -p viui` clean

---

## Todo

- [ ] Cargo.toml (no_std + alloc + embedded-graphics + fontdue)
- [ ] `WidgetId(u64)` — hash-based (FNV-1a), `new(salt)` + `with(index)` chain
- [ ] `Length` enum (Fill / Shrink / Fixed / FillPortion) — iced ref
- [ ] `Constraints` / `Size` / `Point` / `Rect` primitives
- [ ] `LayoutCx` + `LayoutNode { bounds: Rect, children: Vec<LayoutNode> }` — iced ref
- [ ] `WidgetFlags(u8)` bitfield (HOVERED/PRESSED/FOCUSED constants + has/set/clear) — P04 OrbTK ref
- [ ] `WidgetState` struct (flags: WidgetFlags / drag_pos / scroll_y / custom[32])
- [ ] `WidgetStateStore` (BTreeMap + gc) — egui Memory ref
- [ ] `ViWidget` trait: measure/arrange/paint/event/post_layout/on_mount/on_unmount — OrbTK lifecycle ref
- [ ] `ViLayout` trait: 2-pass measure+arrange — OrbTK ref
- [ ] `StackLayout` + `FixedLayout` + `PaddingLayout` implementations
- [ ] `LayoutNode.dirty` flag + dirty subtree propagation
- [ ] `Event` enum với BottomUp/Direct routing modes — OrbTK routing ref
- [ ] `EventCx` (carries &mut WidgetStateStore + &mut FocusManager + messages vec)
- [ ] `FocusManager` (tab order SmallVec<[WidgetId; 16]>)
- [ ] `PaintCx` wrapper
- [ ] `Response` struct (clicked/hovered/changed/rect/id)
- [ ] `ViApp` trait
- [ ] `Element<Msg>` type erasure
- [ ] `PaintCx` (canvas + origin + theme + font + atlas — all owned by caller, not canvas)
- [ ] `WidgetTree` (rebuild/dispatch/layout/paint/take_dirty) — pure LayoutNode tree, no flat BTreeMap
- [ ] lib.rs + prelude.rs
- [ ] cargo check clean

---

## Success Criteria

- `cargo check -p viui` passes với 0 errors, 0 warnings
- `ViWidget` có thể implement bởi một struct đơn giản (test fixture)
- `WidgetTree::layout()` không allocate trên hot path (verified bởi no heap usage in hot path)
- `ViApp` trait compile với một dummy impl

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `dyn ViWidget` sizing issues với `alloc` | Medium | Dùng `Box<dyn ViWidget>` thống nhất |
| FocusManager ring buffer cần alloc | Low | `SmallVec<[WidgetId; 16]>` hoặc `[WidgetId; 16]` fixed |
| Layout cache invalidation logic phức tạp | Medium | Phase 1 không cần full cache — LayoutNode tree chỉ rebuild khi view() output thay đổi |
| **FNV-1a hash collision** trên WidgetId | Low | Collision chỉ gây wrong state hiển thị; dùng salt đủ unique |
| **WidgetStateStore gc()** call timing | Medium | gc() gọi cuối mỗi `view()` pass; thiếu gc → memory leak |
| `Length::FillPortion` resolve logic | Medium | Cần 2-pass layout (OrbTK confirms this) — Pass 1 measure Shrink/Fixed, Pass 2 distribute Fill |
| **dirty subtree** propagation cost | Low | Mark ancestors up to root on state change — O(depth) = O(log N) for balanced tree |
| **`post_layout()` call ordering** | Medium | Must be called AFTER arrange writes final bounds, BEFORE paint. Wrong order → ScrollArea computes with wrong height |
| **on_unmount() not called on app exit** | Low | ViCell cells drop all resources on exit anyway; ensure on_unmount runs on DESTROY_SURFACE path |

---

## Next Steps

→ P02: ViCanvas + DrawTarget (cần types từ P01: `Rect`, `Color`, `PaintCx`)
