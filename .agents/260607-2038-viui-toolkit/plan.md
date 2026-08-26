# ViUI Toolkit — Implementation Plan

**Plan ID**: 260607-2038-viui-toolkit  
**Stage**: G2  
**Priority**: P1 — blocking desktop GUI experience  
**Created**: 2026-06-07  
**Spec**: [docs/specs/14-viui.md](../../docs/specs/14-viui.md)

---

## Mục tiêu

Xây dựng ViUI — UI toolkit `no_std`-native cho ViCell với:
- **Elm-only API** (iced-compatible) — immediate mode đã bị drop (2026-06-07)
- Direct pixel rendering (không tessellation)
- MIT license
- 3 bước triển khai: Core → Basic Widgets → Full Windows

---

## Phân bổ theo 3 bước

| Bước | Phases | Mô tả | Timeline est. | Stage |
|------|--------|-------|---------------|-------|
| **Step 1: Core** | P01, P02, P03 | Engine (Length/WidgetId/StateStore) + Canvas + GlyphAtlas/Galley | ~3 tuần | G2 start |
| **Step 2: Widgets** | P04, P05, P06 | Basic widgets + Theming + Elm facade (iced API) | ~3 tuần | G2 |
| **Step 3: Windows** | P07 | Multi-window + Window chrome + resize | ~3 tuần | G2 |

> **Quyết định 2026-06-07**: Dual-facade dropped. Chỉ Elm (iced-compatible). P05 (immediate egui facade) đã loại. Immediate mode có thể add sau khi Elm core stable nếu cần.

---

## Phase Table

| Phase | File | Nội dung | Algorithm refs | Status | Bước |
|-------|------|----------|----------------|--------|------|
| P01 | [phase-01-core-engine.md](phase-01-core-engine.md) | ViWidget, Length, LayoutNode, WidgetId (hash), WidgetStateStore, ViApp | iced (Length/Limits/Node) · egui (Id hash/Memory) | ✅ Done | Step 1 |
| P02 | [phase-02-vicanvas.md](phase-02-vicanvas.md) | ViCanvas, FramebufferCanvas, Bresenham, bitmap font render | embedded-graphics (DrawTarget/scanline/Bresenham) | ✅ Done | Step 1 |
| P03 | [phase-03-glyph-atlas.md](phase-03-glyph-atlas.md) | GlyphAtlas, LayoutedText (Galley), fontdue, prewarm ASCII | egui (TextureAtlas/Galley) · fontdue | ✅ Done — `default-features=false,features=["hashbrown"]` fix | Step 1 |
| P04 | [phase-04-widget-set.md](phase-04-widget-set.md) | Label, Button, TextEdit, Checkbox, ScrollArea, Image, Column, Row, Space | iced widget impls · OrbTK WidgetFlags | ✅ Done | Step 2 |
| ~~P05~~ | ~~phase-05-immediate-facade.md~~ | ~~egui Ui facade~~ | ~~egui~~ | ❌ **dropped** — dual-facade removed | — |
| P05 | [phase-06-theming.md](phase-06-theming.md) | ViTheme trait, dark/light/kiosk defaults | iced Theme trait | ✅ Done | Step 2 |
| P06 | [phase-07-elm-facade.md](phase-07-elm-facade.md) | iced API: text/button/column/row macros, Element<Msg>, run_app | iced (free fns/macros/Subscription) | ✅ Done | Step 2→3 |
| P07 | [phase-08-window-chrome.md](phase-08-window-chrome.md) | WindowChrome, WindowManager, decode_input_event, translate_input | — | ✅ Done | Step 3 |

---

## Key Dependencies

- `libs/ostd/src/display.rs` — ViSurface (đã impl)
- `libs/api/src/display.rs` — DamageNotify, AttachGrant (đã impl)
- `libs/ostd/src/font.rs` — Bitmap 8×8 (đã impl, giữ nguyên)
- `embedded-graphics` crate — **chưa có trong workspace** → add to `Cargo.toml` workspace deps + `libs/viui/Cargo.toml`
- `fontdue` crate — **chưa có trong workspace** → add to workspace deps; dùng `default-features = false` cho no_std

## Phase Parallelism

```
P01 ──> P02 ─┐
         P03 ─┤──> P04 ──> P05 ──> P06 ──> P07
              └──────┘
```

P02 và P03 có thể chạy song song sau khi P01 hoàn thành (P02 dùng types từ P01, P03 cũng chỉ cần types P01). Sau đó P04 chờ cả P02 + P03.

## Law Compliance

- **Law 1**: `libs/api/` không cần thay đổi — ViUI chỉ dùng existing display API
- **Law 4**: `libs/viui/` là library crate → `#![forbid(unsafe_code)]`
- **Law 5**: không có `mod.rs` — dùng `widgets.rs` parallel to `widgets/`
- **Law 6**: public types dùng `Vi` prefix: `ViApp`, `ViCanvas`, `ViWidget`, `ViTheme`

---

## Crate Structure

```
libs/viui/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── widget.rs          # ViWidget trait, WidgetId, WidgetFlags, PaintCx, EventCx
    ├── layout.rs          # Length, Constraints, Size, Rect, LayoutNode, ViLayout
    ├── state_store.rs     # WidgetState, WidgetStateStore, FocusManager
    ├── canvas.rs          # ViCanvas trait, FramebufferCanvas
    ├── event.rs           # Event enum, EventStatus
    ├── response.rs        # Response (clicked/hovered/changed)
    ├── elm.rs             # ViApp trait, Element<Msg>, run_app
    ├── theme.rs           # ViTheme trait, dark/light/kiosk defaults
    ├── prelude.rs         # re-exports
    └── widgets/
        ├── label.rs
        ├── button.rs
        ├── text_edit.rs
        ├── checkbox.rs
        ├── scroll_area.rs
        └── image.rs
```
