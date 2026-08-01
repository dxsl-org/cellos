# Cellos UI Toolkit: ViUI v2

**Version**: 2.0 (Reactive Signal Tree + dual-layer DSL)
**Status**: Definitive — library architecture shipped; product qualification remains evidence-gated
**Last Updated**: 2026-08-01 (D26)

---

## 1. Decision

ViUI is the Cellos-native `no_std + alloc` UI toolkit. Its normative model is a retained
tree of `ViNode` objects whose state is carried by fine-grained `Signal<T>` values. A
signal update invalidates the affected node/region; the app runner performs layout,
event dispatch, and painting into the app-owned surface before sending damage to the
compositor.

The shipped architecture does **not** promise egui or iced API compatibility. Earlier
immediate/Elm compatibility facades and their percentage claims are withdrawn.

## 2. Runtime architecture

```text
App state (`Signal<T>` / computed subscriptions)
        │
        ▼
Reactive node tree (`ViNode` + signal-driven widgets)
        │ layout / input / paint invalidation
        ▼
App runner + dirty-region renderer
        │
        ▼
App-owned `ViSurface` → DamageNotify → compositor
```

Normative properties:

- State belongs to signals and application objects, not a global widget-state store.
- Nodes receive constraints, events, and paint contexts through the `ViNode` contract.
- ViUI renders inside the App Cell; the compositor is toolkit-agnostic.
- Updates are event-driven. Animation ticks are explicit and feed signal changes.
- Dirty-region behavior is an optimization contract, not permission to skip correctness
  when layout changes affect ancestors or siblings.

Source anchors: `libs/viui/src/signal.rs`, `node.rs`, `app_runner.rs`, and
`dirty_region.rs`.

## 3. Dual authoring layer

"Dual layer" means two ways to create the same reactive node graph:

1. **Rust node API** — construct `ViNode` implementations and bind `Signal<T>` values
   directly. This is the escape hatch and the canonical generated-code target.
2. **`.vi` declarative DSL** — compile an inline document with `vi_design!`, or compile
   standalone `.vi` files at build time with `viui-build`.

Both paths must produce ordinary Rust/node structures. The DSL does not introduce a
second runtime, hidden interpreter, or compositor protocol.

Source anchors: `libs/viui-macros/src/lib.rs`, `tools/viui-build/src/lib.rs`, and the
shared compiler/code generator used by them.

## 4. Shipped component surface

The source tree currently provides:

- flex layout, wrapping, gap, grow, and shrink behavior;
- labels, buttons, checkbox, slider, progress, text editing, dropdown, and image nodes;
- list and virtual-list nodes with signal-backed data;
- stack and tab navigation;
- overlay/menu support;
- line and bar charts;
- signal-driven animation and dirty-region repaint.

This list describes implemented library surface. Exact widget/test counts belong in
generated project status, not this normative specification.

## 5. Rendering and text

ViUI paints directly into the app-owned pixel surface. It does not require a triangle
tessellation pipeline or compositor-side widget knowledge. Text may use the small bitmap
path for diagnostics or the cached scalable-font path for application UI. Applications
must damage every pixel region affected by a visual or layout change.

## 6. Non-goals

- Drop-in egui, iced, Slint, DOM, or web-framework compatibility.
- A second application event loop hidden inside the toolkit.
- Compositor-owned application state or widget layout.
- Unmeasured latency, allocation, or comparative performance guarantees.

Slint remains rejected as the project standard because its licensing/deployment model
does not fit the intended ecosystem. Third-party toolkit comparisons are rationale, not
an API compatibility contract.

## 7. Verification gates

A capability is "shipped" only when its source and focused tests build in the supported
workspace lane. End-to-end qualification additionally requires a signed App Cell,
input-to-render integration coverage, compositor damage validation, and measured target
hardware results. Performance claims require a checked-in benchmark command, fixture,
target profile, and generated result.

---

## See also

- [06-graphics.md](06-graphics.md) — surfaces, compositor, and damage protocol
- [10-testing.md](10-testing.md) — verification layers
- [21-documentation-architecture.md](21-documentation-architecture.md) — normative versus generated status
