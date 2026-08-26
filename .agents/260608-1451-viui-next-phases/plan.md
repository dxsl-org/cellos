# ViUI Next Phases — G1 Completion + G2 Roadmap

**Created:** 2026-06-08  
**Status:** Superseded by `260616-0755-viui-completion` (D34, 2026-08-01)  
**Plan naming:** 260608-1451  

> Historical phase provenance only. Later plans/source completed the overlapping DSL,
> FlexBox, virtual-list, accessibility, and widget work. GPU remains separately gated;
> current normative architecture and qualification live in `docs/specs/14-viui.md`.

---

## Context

ViUI v2 architecture chốt: Reactive Signal Tree + Dual-Layer DSL. Performance baseline P08-P10c DONE. Embedded/robot readiness P01-P03 DONE.

**Còn lại trước khi G1 ship:**
- ListView widget (G1 robot UI cần log list, task queue)
- DSL if/for codegen (conditional + loop trong .vi files)
- Robot Dashboard Demo (integration validation)
- ViUI ↔ ViOS integration (real input events, real compositor surface)
- DSL widget registry (ProgressBar/Slider/ListView missing từ map_element)
- Widget library completeness (port old widgets, thêm atomic widgets)

**G2 roadmap** (plan nhưng defer execute): FlexBox layout, virtual ListView, HW GPU, reactive DSL bindings, accessibility.

---

## Phases

| # | Phase | Status | Stage | Priority | Est |
|---|-------|--------|-------|----------|-----|
| 01 | [ListView + DSL if/for Codegen](phase-01-listview-dsl-codegen.md) | **Complete** | G1 | Critical | 2-3d |
| 02 | [Robot Dashboard Demo](phase-02-robot-dashboard-demo.md) | **Complete** | G1 | High | 2d |
| 03 | [ViUI ↔ ViOS Integration](phase-03-viui-vios-integration.md) | **Complete** | G1 | High | 2-3d |
| 04 | [DSL Widget Registry + Codegen v2](phase-04-dsl-widget-registry.md) | **Complete** | G1 | Medium | 1-2d |
| 05 | [Widget Library Completeness](phase-05-widget-completeness.md) | **Complete** | G1 | Medium | 2-3d |
| 06 | [Theme System v2](phase-06-theme-v2.md) | **Complete** | G1 | Low | 1d |
| 07 | [Layout Engine v2 — FlexBox](phase-07-layout-v2.md) | **Planned** | G2 | High | 3-4d |
| 08 | [Virtual ListView + Perf](phase-08-virtual-listview.md) | **Planned** | G2 | Medium | 2d |
| 09 | [HW GPU Backend](phase-09-hw-gpu-backend.md) | **Planned** | G2 | Medium | 3-4d |
| 10 | [DSL Reactive Bindings](phase-10-dsl-reactive-bindings.md) | **Complete** | G2 | Medium | 2-3d |
| 11 | [Accessibility + Keyboard Nav](phase-11-accessibility.md) | **Planned** | G2 | Low | 2-3d |

**G1 total:** ~10-14 ngày  
**G2 total:** ~12-14 ngày

---

## Dependency graph

```
P01 (ListView + DSL if/for)
 ├──▶ P02 (Robot Dashboard)
 └──▶ P04 (DSL Widget Registry)
          └──▶ P02 (nếu Dashboard dùng .vi DSL)

P03 (ViOS Integration) — parallel với P01/P04/P05 (files khác nhau)

P05 (Widget Completeness) — parallel với P01/P03/P04

P06 (Theme v2) — sau P01+P05 (widgets phải exist)

─── G2 ───────────────────────────────────────────────────

P07 (FlexBox) — sau G1 complete
P08 (Virtual ListView) — sau P01 (cần ListView baseline)
P09 (HW GPU) — sau P07 (layout ổn định trước)
P10 (DSL Reactive) — sau P04
P11 (Accessibility) — sau P05 + P07
```

---

## Success criteria (G1)

- Robot Dashboard chạy trên ViOS QEMU với font scalable, ProgressBar animate, Slider drag, ListView scroll
- `.vi` files có `if cond { ... }` và `for x in items { ... }` compile ra valid Rust
- Input events từ input service cell map đúng sang viui::Event (keyboard + touch)
- CheckBox, TextEdit (cursor), Image, Divider render đúng với node_widget pattern
- `cargo check` full workspace không warning
