# Phase P06 — Theming System

**Step**: 2 (Widgets)  
**Priority**: P2  
**Status**: 📋 Planned  
**Effort est.**: 2-3 ngày  
**Depends on**: P04

---

## Context Links

- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) §10 Phân tầng theo profile

---

## Overview

`ViTheme` trait + 3 built-in themes: `DarkTheme`, `LightTheme`, `KioskTheme`. Widgets đọc colors/spacing từ theme thay vì hardcode. Theme được truyền qua `PaintCx`.

---

## Architecture

```rust
// libs/viui/src/theme.rs
pub trait ViTheme: 'static {
    fn bg(&self) -> Color;
    fn fg(&self) -> Color;
    fn accent(&self) -> Color;
    fn surface(&self) -> Color;           // card/panel background
    fn border(&self) -> Color;
    fn text_primary(&self) -> Color;
    fn text_secondary(&self) -> Color;

    fn button_normal(&self) -> Color;
    fn button_hovered(&self) -> Color;
    fn button_pressed(&self) -> Color;

    fn font_size_body(&self) -> u16;      // pixels
    fn font_size_heading(&self) -> u16;
    fn padding_sm(&self) -> f32;          // 4px
    fn padding_md(&self) -> f32;          // 8px
    fn padding_lg(&self) -> f32;          // 16px
}

pub struct DarkTheme;
pub struct LightTheme;
pub struct KioskTheme;   // high contrast, large touch targets

impl ViTheme for DarkTheme { ... }
impl ViTheme for LightTheme { ... }
impl ViTheme for KioskTheme { ... }
```

`PaintCx` thêm `theme: &dyn ViTheme` field. Widgets gọi `cx.theme.button_normal()` thay vì hardcode `Color::bgra(...)`.

---

## Implementation Steps

1. `ViTheme` trait
2. `DarkTheme` impl (OLED-friendly dark palette)
3. `LightTheme` impl
4. `KioskTheme` impl (high contrast, 24px font minimum)
5. Update `PaintCx` thêm `theme` field
6. Update tất cả widgets trong P04 dùng `cx.theme.*` colors
7. `cargo check` clean

---

## Todo

- [ ] ViTheme trait
- [ ] DarkTheme
- [ ] LightTheme  
- [ ] KioskTheme
- [ ] PaintCx.theme field
- [ ] Update Label/Button/TextEdit/Checkbox/ScrollArea dùng theme
- [ ] cargo check clean

---

## Success Criteria

- Swap theme tại runtime (change `PaintCx.theme` ref) → toàn bộ UI re-renders với palette mới
- KioskTheme: tất cả touch targets ≥ 44×44px, font ≥ 20px

---

## Next Steps

→ P07: Elm facade (iced API)  
→ P08: Multi-window + window chrome (Step 3)
