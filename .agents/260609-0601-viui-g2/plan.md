# ViUI G2 — Reactive DSL · FlexBox · Virtual List · Accessibility · GPU

**Created:** 2026-06-09  
**Status:** Superseded/closed — P01-P04 landed; GPU deferred (D34, 2026-08-01)  
**Plan naming:** 260609-0601

> `260616-0755-viui-completion` is the canonical closed implementation record. The
> unimplemented GPU backend must be re-opened only as its own hardware/benchmark-gated
> plan. See `docs/specs/14-viui.md`.

---

## Context

All G1 phases complete (P01-P06, commit c99f1ecd). ViUI now has: Signal reactive tree, 15+
widgets, DSL codegen, robot dashboard, ViOS integration, theme system v2.

**G2 goal:** close the remaining quality gaps — proper DSL type-safety, flexible layout, large-list
performance, keyboard accessibility, and a hardware GPU backend for G2+ targets.

---

## Phases

| # | Phase | Status | Priority | Est | Parallel |
|---|-------|--------|----------|-----|---------|
| 01 | [DSL Reactive Bindings](phase-01-dsl-reactive.md) | **Complete** | High | 2-3d | Wave 1 |
| 02 | [FlexBox Container](phase-02-flexbox.md) | **Complete** | High | 2d | Wave 1 |
| 03 | [Virtual ListView v2](phase-03-virtual-listview.md) | **Complete** | Medium | 1-2d | Wave 1 |
| 04 | [Accessibility + Keyboard Nav](phase-04-accessibility.md) | **Complete** | Medium | 2d | Wave 1 |
| 05 | [HW GPU Backend (GLES2)](phase-05-hw-gpu.md) | **Planned** | Low | 3-4d | Wave 2 |

**Wave 1 (P01–P04):** all fully parallel — zero shared file writes between phases.  
**Wave 2 (P05):** after Wave 1 stabilizes layout API.

---

## Dependency graph

```
P01 (vi-compiler only)  ─╮
P02 (flex_box.rs NEW)   ─┤  → Wave 1 (all parallel)
P03 (list_view.rs only) ─┤
P04 (node.rs + app_runner additive) ─╯

P05 (GPU canvas) — after Wave 1
```

---

## Key decisions

- **FlexBox non-breaking**: new `FlexBox` widget; does NOT change `ViNode::layout()` signature.
  Avoids Law 1 (libs/api untouched). Consistent with ViCell additive-composition principle.
- **DSL Reactive**: vi-compiler-only change. `libs/viui` unchanged.
- **P04 Accessibility**: additive default-impl methods on `ViNode`. No breaking change.

---

## Success criteria (G2)

- `FlexBox::row()` / `FlexBox::column()` with `flex_grow` distributes space correctly
- DSL `.vi` files emit `Signal::map()` chains for computed props (no string-desugar false positives)
- ListView supports variable item heights; `item_at()` binary-search O(log n)
- Tab cycles focus across Button/Slider/CheckBox/TextEdit; Enter/Space activates focused widget
- `cargo check` full workspace — no warnings
