# ViUI v2 Library Completion Record

**Created:** 2026-06-16  
**Status:** Complete — closed implementation record (D34, 2026-08-01)  
**Precedes:** G2 platform milestone (App SDK L1)

> **Portfolio note:** This is the canonical closed record for shipped ViUI v2 library
> capabilities. Product qualification remains owned by Spec 14; GPU acceleration is a
> separate future gate, not an unfinished phase of this plan.

---

## Context

G1 đã xong 6/6 phases: 16 widgets, Signal<T>/Computed, AnimatedSignal, ViApp runner, FramebufferRenderer, input_bridge, DarkTheme/LightTheme, vi-compiler (17 widget types, if/for). Robot-dashboard chạy trên QEMU.

**Tại sao còn 30%?** G2 phases từ plan cũ (P07-P11) chưa implement. Và có những gap critical chưa được plan bao giờ: overlay widgets (Dialog/DropDown/Toast), navigation multi-screen, charts cho dashboards, DSL build.rs cho DX.

GPU backend và accessibility bị exclude (quá sớm, G3+).

---

## Phases

| # | Phase | Status | Wave | Priority | Est | Parallel |
|---|-------|--------|------|----------|-----|---------|
| 01 | [Overlay Widgets — Dialog, DropDown, Toast](phase-01-overlay-widgets.md) | **Complete** | G1.1 | Critical | 3-4d | W1 |
| 02 | [Navigation / Router](phase-02-navigation-router.md) | **Complete** | G1.1 | Critical | 2-3d | W1 |
| 03 | [Charts — LineChart + BarChart](phase-03-charts.md) | **Complete** | G1.1 | High | 2-3d | W1 |
| 04 | [DSL build.rs Integration](phase-04-dsl-build-integration.md) | **Complete** | G1.1 | High | 2d | W1 |
| 05 | [Virtual ListView + Perf](phase-05-virtual-listview.md) | **Complete** | G2.1 | High | 3d | W2 |
| 06 | [FlexBox v2 — Full Layout Engine](phase-06-flexbox-v2.md) | **Complete** | G2.1 | Medium | 3-4d | W2 |
| 07 | [DSL Advanced Bindings](phase-07-dsl-advanced-bindings.md) | **Complete** | G2.2 | Medium | 3d | W3 |

**Wave 1 (G1.1):** P01 + P02 + P03 + P04 — tất cả parallel (files khác nhau)  
**Wave 2 (G2.1):** P05 + P06 — parallel sau Wave 1 (P05 cần ViApp overlay từ P01)  
**Wave 3 (G2.2):** P07 — sau P04 (cần build.rs baseline)

**Tổng:** ~18-22 ngày nếu sequential, ~8-10 ngày nếu parallel đúng.

---

## Dependency Graph

```
P01 (Overlay widgets: Dialog/DropDown/Toast)
 └── OverlayLayer trong ViApp ←─ P02 (Navigation) cũng dùng overlay mechanism
 └── ToastManager               ←─ P03 (Charts) không phụ thuộc

P02 (Navigation) — cần OverlayLayer từ P01 nếu dùng modal transitions
                   nhưng core Router KHÔNG cần P01

P03 (Charts) — hoàn toàn độc lập, chỉ cần canvas API (đã có)

P04 (DSL build.rs) — hoàn toàn độc lập, tools/ territory

── Wave 2 (sau Wave 1) ──────────────────────────────────

P05 (Virtual ListView) — sau P01 (cần overlay để show loading state?)
                         thực ra độc lập, chỉ cần list_view.rs baseline

P06 (FlexBox v2) — độc lập với P01-P04, chỉ cần flex_box.rs baseline

── Wave 3 ────────────────────────────────────────────────

P07 (DSL advanced) — sau P04 (build.rs phải tồn tại để test E2E DSL pipeline)
```

**Thực tế:** P01-P04 hoàn toàn parallel. P05-P06 parallel với nhau. P07 sau P04.

---

## Gap Analysis (so với plan cũ)

| Capability | Plan cũ | Plan này |
|------------|---------|----------|
| Dialog/Modal | ❌ Không có | ✅ P01 |
| DropDown/Select | ❌ Không có | ✅ P01 |
| Toast/Notification | ❌ Không có | ✅ P01 |
| Navigation/Router | ❌ Không có | ✅ P02 |
| Charts | ❌ Không có | ✅ P03 |
| DSL build.rs | ❌ Không có | ✅ P04 |
| Virtual ListView | Planned (P08) | ✅ P05 |
| FlexBox v2 | Planned (P07) | ✅ P06 |
| DSL advanced (@=, #=) | Planned (P10, partial) | ✅ P07 |
| GPU backend | Planned (P09) | ❌ Excluded |
| Accessibility | Planned (P11) | ❌ Excluded (G3) |

---

## Success Criteria (full completion)

- App có thể dùng Dialog confirm, DropDown select, Toast notify mà không cần custom overlay logic
- App có nhiều screen, navigate push/pop với transition
- LineChart hiển thị sensor history theo thời gian thực
- `.vi` file compile tự động qua build.rs, không cần manual cli step
- ListView handle 10k+ items smooth với virtual rendering
- FlexBox layout đúng theo flex spec (wrap, space-between, flex-grow)
- DSL hai chiều: `@=` binding và `#=` computed property
- `cargo check` full workspace không warning sau mỗi phase
