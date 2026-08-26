# Phase 06 — Theme System v2

**Status:** Planned  
**Stage:** G1  
**Priority:** Low  
**Estimate:** 1 ngày  
**Depends on:** Phase 05 (all widgets must exist)

---

## Context Links

- [`libs/viui/src/theme.rs`](../../../libs/viui/src/theme.rs) — DarkTheme, LightTheme, KioskTheme đã có
- [`libs/viui/src/render_ctx.rs`](../../../libs/viui/src/render_ctx.rs) — RenderCtx carries `&dyn ViTheme`
- [`libs/viui/src/app_runner.rs`](../../../libs/viui/src/app_runner.rs) — with_theme() builder

---

## Overview

Theme cơ bản đã có (DarkTheme, LightTheme, KioskTheme). Phase này:

1. **RobotTheme** — industrial dark, high contrast, phù hợp robot/embedded
2. **Dynamic theme switching** — `Signal<Arc<dyn ViTheme>>` cho phép switch theme lúc runtime
3. **Font size activation** — KioskTheme/RobotTheme mới nên khai báo đúng `font_size_body` (16px, 20px)
4. **Color token completeness** — thêm vào ViTheme trait: `progress_fill`, `slider_track`, `list_selected_bg`

---

## RobotTheme

Industrial dark với high contrast, chú trọng readability dưới ánh sáng mạnh:

```rust
pub struct RobotTheme;

impl ViTheme for RobotTheme {
    fn bg(&self)      -> Color { Color::rgb(10, 12, 15) }
    fn surface(&self) -> Color { Color::rgb(22, 26, 30) }
    fn border(&self)  -> Color { Color::rgb(80, 90, 100) }

    fn text_primary(&self)   -> Color { Color::rgb(220, 225, 230) }
    fn text_secondary(&self) -> Color { Color::rgb(140, 150, 160) }

    fn accent(&self)          -> Color { Color::rgb(0, 160, 255) }   // industrial blue
    fn button_normal(&self)   -> Color { Color::rgb(35, 40, 48) }
    fn button_hovered(&self)  -> Color { Color::rgb(50, 60, 72) }
    fn button_pressed(&self)  -> Color { Color::rgb(0, 120, 200) }
    fn input_bg(&self)        -> Color { Color::rgb(15, 18, 22) }
    fn input_focused_bg(&self) -> Color { Color::rgb(10, 15, 40) }
    fn input_focused_border(&self) -> Color { Color::rgb(0, 160, 255) }

    fn padding_sm(&self) -> f32 { 6.0 }
    fn padding_md(&self) -> f32 { 12.0 }
    fn padding_lg(&self) -> f32 { 20.0 }

    fn font_size_body(&self)    -> u16 { 16 }
    fn font_size_heading(&self) -> u16 { 20 }
}
```

---

## ViTheme trait extensions

Thêm token mới với default impl để không break existing themes:

```rust
pub trait ViTheme: 'static {
    // --- existing tokens ---

    // --- new tokens (default impls = derive from existing) ---

    /// Fill color for ProgressBar fill region.
    fn progress_fill(&self) -> Color { self.accent() }

    /// Track color for Slider background.
    fn slider_track(&self) -> Color { self.border() }

    /// Background highlight for selected ListView item.
    fn list_selected_bg(&self) -> Color {
        let a = self.accent();
        Color::rgba(a.r, a.g, a.b, 60) // semi-transparent accent
    }

    /// Foreground of selected list item text.
    fn list_selected_fg(&self) -> Color { self.text_primary() }

    /// Checkbox check color.
    fn check_color(&self) -> Color { self.accent() }

    /// Divider line color.
    fn divider(&self) -> Color { self.border() }
}
```

KioskTheme override `progress_fill` → bright yellow, `list_selected_bg` → high contrast.

---

## Dynamic Theme Switching

Hiện tại `ViApp::with_theme(Box<dyn ViTheme>)` là one-time setup. Cần dynamic switching:

```rust
// app_runner.rs

pub struct ViApp {
    // current:
    theme: Box<dyn ViTheme>,

    // change to:
    theme: Arc<dyn ViTheme + Send + Sync>,
}

impl ViApp {
    /// Switch theme at runtime. Next frame will use new theme.
    pub fn set_theme(&mut self, theme: impl ViTheme + Send + Sync + 'static) {
        self.theme = Arc::new(theme);
    }
}
```

`RenderCtx` đã nhận `&dyn ViTheme` per-frame → no cache to invalidate.

**Use case**: kiosk app switch giữa day/night mode, robot switch danger/normal mode.

---

## Font size fix in KioskTheme

KioskTheme hiện tại trả về `font_size_body() → 0` (bitmap fallback). Fix:

```rust
impl ViTheme for KioskTheme {
    // add:
    fn font_size_body(&self)    -> u16 { 18 }
    fn font_size_heading(&self) -> u16 { 22 }
}
```

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/theme.rs` | MODIFY — RobotTheme, ViTheme new tokens, KioskTheme font sizes |
| `libs/viui/src/app_runner.rs` | MODIFY — Arc<dyn ViTheme>, set_theme() method |
| `libs/viui/src/render_ctx.rs` | MODIFY — update RenderCtx if theme type changes |
| `libs/viui/src/node_widgets/progress_bar.rs` | MODIFY — use `cx.theme.progress_fill()` |
| `libs/viui/src/node_widgets/slider.rs` | MODIFY — use `cx.theme.slider_track()` |
| `libs/viui/src/node_widgets/list_view.rs` | MODIFY — use `cx.theme.list_selected_bg/fg()` |

---

## Todo

- [ ] ViTheme: add new token methods với default impls
- [ ] RobotTheme: implement ViTheme
- [ ] KioskTheme: fix font sizes + override new tokens
- [ ] app_runner.rs: Arc<dyn ViTheme>, set_theme()
- [ ] progress_bar.rs: use progress_fill token
- [ ] slider.rs: use slider_track token
- [ ] list_view.rs: use list_selected_bg/fg tokens
- [ ] cargo check

---

## Success Criteria

- `RobotTheme` compiles, all ViTheme tokens implemented
- `app.set_theme(RobotTheme)` switches theme next frame without restart
- ProgressBar fill uses `cx.theme.progress_fill()` (not hardcoded color)
- Slider track uses `cx.theme.slider_track()`
- ListView selected item uses `cx.theme.list_selected_bg()`
- KioskTheme font size = 18px body, 22px heading
