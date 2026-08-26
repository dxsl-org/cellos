# ViUI Embedded/Robot Readiness — Implementation Plan

**Created:** 2026-06-08  
**Status:** Superseded — capabilities landed through later ViUI plans (D34, 2026-08-01)  
**Priority:** High — G1 target milestone

> Historical precursor only. The source now contains the planned font/widget/animation/
> list/dashboard surface. `260616-0755-viui-completion` is the closed implementation
> record; Spec 14 owns current qualification gates.

---

## Context

ViUI hiện đạt ~55% Slint embedded feature set. Architecture (GpuRenderer<E>, Signal reactivity,
dual-dirty-flags) đã solid. Gap chính ở **content layer**: font wiring, missing widgets,
animation, touch events, DSL codegen completeness.

**Key discovery:** `GlyphAtlas` (fontdue no_std) + `draw_text_scaled()` đã implement xong trong
`ostd/font_atlas.rs` + `canvas.rs`. Chỉ cần wire vào widgets thay vì build từ đầu.

---

## Phases

| # | Phase | Status | Priority | Est |
|---|-------|--------|----------|-----|
| 01 | [Font Pipeline Activation](phase-01-font-pipeline-activation.md) | Planned | Critical | 1-2 ngày |
| 02 | [Touch Input + Robot Widgets](phase-02-touch-input-robot-widgets.md) | Planned | Critical | 3-4 ngày |
| 03 | [Animation Engine](phase-03-animation-engine.md) | Planned | High | 2-3 ngày |
| 04 | [ListView + DSL Completeness](phase-04-listview-dsl-completeness.md) | Planned | Medium | 2-3 ngày |
| 05 | [Robot Dashboard Demo](phase-05-robot-dashboard-demo.md) | Planned | Medium | 2 ngày |

**Total estimate:** 10-14 ngày

---

## Dependencies

```
Phase 01 (Font) ──────────────────────┐
                                       ├──▶ Phase 03 (Animation)
Phase 02 (Touch + Widgets) ───────────┘
    │
    └──▶ Phase 04 (ListView + DSL)
              │
              └──▶ Phase 05 (Demo)
```

Phase 01 và 02 có thể chạy song song vì chúng sửa file khác nhau (01: canvas/widgets/font, 02: event/new widgets).

---

## Affected Files (Quick Reference)

```
libs/viui/src/
├── canvas.rs              # Phase 01: wire draw_text_scaled defaults
├── event.rs               # Phase 02: add Touch* variants
├── app_runner.rs          # Phase 01+03: FontContext injection, animation tick
├── animation.rs           # Phase 03: NEW
├── widgets/
│   ├── label.rs           # Phase 01: use draw_text_scaled
│   ├── button.rs          # Phase 01: use draw_text_scaled
│   ├── text_edit.rs       # Phase 01: use draw_text_scaled
│   ├── progress_bar.rs    # Phase 02: NEW
│   ├── slider.rs          # Phase 02: NEW
│   ├── touch_area.rs      # Phase 02: NEW
│   └── list_view.rs       # Phase 04: NEW
libs/ostd/src/
│   └── (font assets embedded here or in viui)
tools/vi-compiler/src/
│   └── codegen.rs         # Phase 04: if/for codegen
cells/apps/robot-dashboard/ # Phase 05: NEW
```

---

## Success Definition

- Label/Button hiển thị scalable font (16px, 24px) không pixelated
- Touchscreen robot panel UI: ProgressBar sensor levels + Slider controls + TouchArea gestures
- Animation: value transitions mượt (200ms ease-in-out)
- robot-dashboard app compile + boot + hiển thị đầy đủ trên ViOS
