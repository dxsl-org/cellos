# Phase 01 — Font Pipeline Activation

**Status:** Planned  
**Priority:** Critical — blocker cho UI quality  
**Estimate:** 1-2 ngày

---

## Context Links

- Infrastructure đã có: [`libs/ostd/src/font_atlas.rs`](../../../libs/ostd/src/font_atlas.rs) — fontdue GlyphAtlas
- [`libs/viui/src/canvas.rs`](../../../libs/viui/src/canvas.rs):379 — `draw_text_scaled()` đã implement đúng
- [`libs/viui/src/app_runner.rs`](../../../libs/viui/src/app_runner.rs) — `tick()` cần nhận FontContext

---

## Overview

`draw_text_scaled()` + `GlyphAtlas` đã implemented nhưng chưa có:
1. Font TTF data để load
2. `FontContext` struct để pass atlas qua widget tree
3. Widgets (Label, Button, TextEdit) vẫn dùng `draw_text()` (8×8 bitmap)

Phase này wire tất cả lại để scalable font trở thành default.

---

## Requirements

**Functional:**
- Label/Button/TextEdit render text với fontdue GlyphAtlas ở size tuỳ chọn
- Default font size: 16px cho UI thường, 24px cho kiosk/robot
- Fallback về 8×8 bitmap khi GlyphAtlas chưa được set (no_std graceful degradation)
- Font data embed vào binary tại compile time (no filesystem dependency)

**Non-functional:**
- Glyph cache: BTreeMap trong GlyphAtlas (đã có), không cache thêm tầng nữa
- Không tăng stack depth: GlyphAtlas qua `&mut` borrow, không clone
- Embedded font TTF phải < 200KB (robot/embedded disk constraint)

---

## Architecture

### FontContext

```rust
// libs/viui/src/font_context.rs (NEW, ~50 lines)
pub struct FontContext {
    pub atlas:    GlyphAtlas,
    pub size_px:  f32,       // default text size
}

impl FontContext {
    /// Load the bundled default font (embedded at compile time).
    pub fn default_font() -> Option<Self> {
        GlyphAtlas::new(include_bytes!("../assets/Inter-Regular.ttf"))
            .map(|atlas| Self { atlas, size_px: 16.0 })
    }
}
```

### PaintCx update

`PaintCx` (trong `libs/viui/src/widget.rs`) hiện không có font context. Cần thêm:

```rust
pub struct PaintCx<'a> {
    pub canvas: &'a mut dyn ViCanvas,
    pub font:   Option<&'a mut FontContext>,   // NEW — None = bitmap fallback
    // ...existing fields
}
```

Tương tự cho `ViNode::paint(&mut self, canvas: &mut dyn ViCanvas, font: &mut FontContext)` — hoặc dùng một `PaintCtx` wrapper.

**Design decision:** Thêm `font_ctx: Option<&mut FontContext>` vào signature của `ViNode::paint()` là breaking change với v1 ViWidget. Thay vào đó, dùng một `RenderCtx` struct chứa cả canvas + font:

```rust
// libs/viui/src/render_ctx.rs (NEW, ~30 lines)
pub struct RenderCtx<'a> {
    pub canvas: &'a mut dyn ViCanvas,
    pub font:   &'a mut FontContext,
}
```

Update `ViNode::paint()` signature:
```rust
// Before: fn paint(&self, canvas: &mut dyn ViCanvas);
// After:  fn paint(&self, cx: &mut RenderCtx<'_>);
```

Update `ViRenderer::render()` để nhận `RenderCtx`.

### ViApp integration

```rust
pub struct ViApp {
    // ...existing
    font_ctx: FontContext,  // NEW — held for lifetime of app
}

impl ViApp {
    pub fn new(root: Box<dyn ViNode>, renderer: Box<dyn ViRenderer>) -> Self {
        let font_ctx = FontContext::default_font()
            .unwrap_or_else(|| FontContext::bitmap_fallback());
        // ...
    }
    
    pub fn with_font(mut self, font_bytes: &[static u8], size_px: f32) -> Self {
        // Allow caller to override the bundled font
        // ...
    }
}
```

### Widget updates

Label, Button, TextEdit: thay `canvas.draw_text()` → `cx.font.atlas.rasterize()` → `canvas.draw_text_scaled()`.

```rust
// Pattern cho Label::paint():
fn paint(&self, cx: &mut RenderCtx<'_>) {
    let text = self.text.get();
    cx.canvas.draw_text_scaled(
        pos, &text,
        cx.font.size_px,
        self.color,
        &mut cx.font.atlas,
    );
}
```

---

## Font Asset

**Chọn font:** [Inter](https://rsms.me/inter/) Regular — MIT license, Latin + Vietnamese coverage, tốt cho UI.

Nếu Inter quá lớn, dùng **Noto Sans Regular subset** (Latin only) hoặc **JetBrains Mono** cho kiosk.

Thêm font vào `libs/viui/assets/Inter-Regular.ttf` (hoặc `libs/ostd/assets/`).

Cargo.toml `viui` crate: không cần gì thêm vì `include_bytes!` compile-time embed.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/font_context.rs` | CREATE — FontContext struct |
| `libs/viui/src/render_ctx.rs` | CREATE — RenderCtx{canvas, font} |
| `libs/viui/src/lib.rs` | MODIFY — add mod declarations |
| `libs/viui/src/app_runner.rs` | MODIFY — hold FontContext, pass to render |
| `libs/viui/src/renderer.rs` | MODIFY — render() nhận RenderCtx |
| `libs/viui/src/node.rs` | MODIFY — paint() nhận RenderCtx |
| `libs/viui/src/node_widgets/*.rs` | MODIFY — all 4 node widgets |
| `libs/viui/src/widgets/*.rs` | MODIFY — Label, Button, TextEdit |
| `libs/viui/assets/Inter-Regular.ttf` | ADD — embedded font |
| `libs/viui/Cargo.toml` | VERIFY — fontdue available via ostd dep |

---

## Implementation Steps

1. Download Inter-Regular.ttf (hoặc chọn font compact) → `libs/viui/assets/`
2. Tạo `libs/viui/src/font_context.rs` với `FontContext` + `FontContext::default_font()`
3. Tạo `libs/viui/src/render_ctx.rs` với `RenderCtx<'a>`
4. Update `ViNode::paint()` trait: `fn paint(&self, cx: &mut RenderCtx<'_>)`
5. Update `ViRenderer::render()`: pass `RenderCtx` vào closure
6. Update `ViApp`: hold `FontContext`, construct `RenderCtx` per frame
7. Update tất cả `ViNode` impl: `label.rs`, `button.rs`, `column.rs`, `row.rs`
8. Update tất cả `ViWidget` impl (v1): `widgets/label.rs`, `button.rs`, `text_edit.rs`
9. `cargo check` — fix tất cả compile errors
10. Chạy `viui-demo` trên ViOS, verify text hiển thị scalable ở 16px và 24px

---

## Todo

- [ ] Chọn + download font TTF (Inter-Regular hoặc compact alternative)
- [ ] Tạo `font_context.rs` và `render_ctx.rs`
- [ ] Refactor `ViNode::paint()` signature → RenderCtx
- [ ] Refactor `ViRenderer::render()` → nhận RenderCtx
- [ ] Update ViApp::tick() để construct + pass RenderCtx
- [ ] Update tất cả node_widgets (label, button, column, row)
- [ ] Update tất cả v1 widgets (label, button, text_edit)
- [ ] cargo check pass, viui-demo boots với scalable font

---

## Success Criteria

- `cargo check` passes, zero compile errors
- `viui-demo` hiển thị "Count: 0" bằng scalable font (không phải 8×8 blocky)
- GlyphAtlas cache: glyph 'A' ở 16px chỉ rasterize 1 lần, subsequent paint = cache hit
- `FontContext::bitmap_fallback()` hoạt động khi không có TTF (graceful no_std fallback)

---

## Risk

**Breaking change trong ViNode::paint() signature** — ảnh hưởng tất cả widget implementations.
Giảm thiểu: thực hiện toàn bộ refactor trong 1 commit, `cargo check` trước khi push.

**Font size**: Inter-Regular.ttf ~280KB. Embedded vào binary sẽ tăng binary size.
Mitigate: dùng subsetting tool (pyftsubset) để chỉ giữ ASCII + Vietnamese glyphs → ~60KB.
