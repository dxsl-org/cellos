# Phase 04 — DSL Widget Registry + Codegen v2

**Status:** Planned  
**Stage:** G1  
**Priority:** Medium  
**Estimate:** 1-2 ngày  
**Depends on:** Phase 01 (ListView must exist), embedded/robot P02 (ProgressBar/Slider/TouchArea DONE)  
**Can run parallel with:** Phase 03, Phase 05

---

## Context Links

- [`tools/vi-compiler/src/codegen.rs`](../../../tools/vi-compiler/src/codegen.rs) — map_element() chỉ có 4 widget types
- [`tools/vi-compiler/src/eval.rs`](../../../tools/vi-compiler/src/eval.rs) — expression evaluation
- [`libs/viui/src/node_widgets/`](../../../libs/viui/src/node_widgets/) — all available widgets
- [`tools/vi-compiler/tests/codegen_tests.rs`](../../../tools/vi-compiler/tests/codegen_tests.rs) — existing tests

---

## Overview

`vi-compiler/src/codegen.rs` hiện chỉ biết map 4 widget names:
```
VBox/VerticalLayout → Column
HBox/HorizontalLayout → Row
Text/Label → Label
Button → Button
```

Sau embedded/robot readiness, ViUI có thêm: `ProgressBar`, `Slider`, `TouchArea`, và phase 01 sẽ thêm `ListView`.

Phase này: mở rộng registry + cải thiện codegen để `.vi` files có thể dùng full widget set.

---

## Part A — map_element Expansion

### Current

```rust
fn map_element(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "VerticalLayout" | "VBox"   => Some(("Column",   "column")),
        "HorizontalLayout" | "HBox" => Some(("Row",      "row")),
        "Text" | "Label"            => Some(("Label",    "label")),
        "Button"                    => Some(("Button",   "button")),
        _ => None,
    }
}
```

### Extended

```rust
fn map_element(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        // Layout
        "VerticalLayout" | "VBox" | "Column"   => Some(("Column",      "column")),
        "HorizontalLayout" | "HBox" | "Row"    => Some(("Row",         "row")),

        // Text
        "Text" | "Label"                        => Some(("Label",       "label")),

        // Interactive
        "Button"                                => Some(("Button",      "button")),
        "Slider"                                => Some(("Slider",      "slider")),
        "CheckBox" | "Checkbox"                 => Some(("CheckBox",    "checkbox")),

        // Display
        "ProgressBar" | "Progress"              => Some(("ProgressBar", "progress_bar")),
        "Image"                                 => Some(("Image",       "image")),

        // Input
        "TextInput" | "TextEdit"                => Some(("TextEdit",    "text_edit")),

        // Container
        "TouchArea"                             => Some(("TouchArea",   "touch_area")),
        "ListView" | "List"                     => Some(("ListView",    "list_view")),
        "ScrollArea" | "ScrollView"             => Some(("ScrollArea",  "scroll_area")),

        _ => None,
    }
}
```

Note: `ScrollArea`, `CheckBox`, `TextEdit` sẽ được migrate sang node_widgets trong P05.
Thêm mapping bây giờ không hại — chỉ fail compile nếu widget chưa exist; P05 fix đó.

---

## Part B — Property Codegen per Widget

Mỗi widget có different constructor signature. Codegen cần biết:
- Mandatory constructor arg (e.g. ProgressBar cần Signal<f32>)
- Optional builder methods (e.g. `.color()`, `.item_height()`)

### Widget constructor table

```rust
/// How to construct a widget from element bindings.
enum CtorStyle {
    /// Widget::new(signal) — signal comes from first binding
    SignalFirst,
    /// Widget::new(signal, callback) — Button style
    SignalCallback,
    /// Widget::new(items_signal) — ListView
    ItemsSignal,
    /// Column::new(children_vec) / Row::new(children_vec)
    Container,
    /// Widgets with no mandatory arg (e.g. Image with just properties)
    NoArg,
}

fn widget_ctor_style(rust_type: &str) -> CtorStyle {
    match rust_type {
        "Column" | "Row"                     => CtorStyle::Container,
        "Label"                              => CtorStyle::SignalFirst,
        "Button"                             => CtorStyle::SignalCallback,
        "ProgressBar" | "Slider"             => CtorStyle::SignalFirst,
        "ListView"                           => CtorStyle::ItemsSignal,
        "TouchArea" | "CheckBox" | "TextEdit"| "Image" | "ScrollArea" => CtorStyle::NoArg,
        _                                    => CtorStyle::NoArg,
    }
}
```

Codegen switch on `CtorStyle` khi emit widget instantiation.

---

## Part C — Emit builder method calls

Sau constructor call, emit property bindings như builder methods:

```rust
// .vi source:
ProgressBar {
    value: self.battery
    color: #00FF00
}

// Generated:
Box::new(
    ProgressBar::new(*self.battery.get())
        .color(viui::canvas::Color::from_hex(0x00FF00))
)
```

Cần thêm helper `emit_builder_call(prop_name, expr)` trong codegen.rs:

```rust
fn emit_builder_call(prop: &str, expr: &str) -> Option<String> {
    match prop {
        "color"       => Some(format!(".color({})", desugar_color_expr(expr))),
        "item_height" => Some(format!(".item_height({})", expr)),
        "text"        => None, // handled by constructor
        "value"       => None, // handled by constructor
        _             => None, // unknown property — skip with warning
    }
}
```

---

## Part D — Compile error recovery

Khi `map_element()` trả về `None` (widget chưa known), hiện tại codegen panic hoặc skip silently.

Cải thiện: emit compile error với source location:

```rust
// codegen.rs
if map_element(&e.name).is_none() {
    // emit a Rust compile_error! with location info
    writeln!(out, "compile_error!(\"vi-compiler: unknown widget '{}' at line {}\");",
        e.name, e.span.line)?;
}
```

---

## Related Code Files

| File | Action |
|------|--------|
| `tools/vi-compiler/src/codegen.rs` | MODIFY — map_element expand + CtorStyle + builder emit + error recovery |
| `tools/vi-compiler/tests/codegen_tests.rs` | MODIFY — test ProgressBar/Slider/ListView codegen |

---

## Implementation Steps

1. Extend `map_element()` với full widget set
2. Add `widget_ctor_style()` helper
3. Update `emit_element()` to switch on CtorStyle
4. Add `emit_builder_call()` for known properties
5. Add `compile_error!` for unknown widgets
6. Add codegen tests: ProgressBar, Slider, ListView, CheckBox
7. `cargo test` vi-compiler
8. `cargo check` main workspace

---

## Todo

- [ ] map_element: extend với full widget list
- [ ] CtorStyle enum + widget_ctor_style()
- [ ] emit_element: switch ctor style
- [ ] emit_builder_call: color, item_height
- [ ] compile_error! for unknown widget
- [ ] codegen_tests: ProgressBar/Slider/ListView test cases
- [ ] cargo test vi-compiler
- [ ] cargo check main workspace

---

## Success Criteria

- `.vi` file với `ProgressBar { value: self.battery }` → vi-compiler ra valid Rust
- `.vi` file với `ListView { items: self.log_items }` → vi-compiler ra valid Rust
- Unknown widget `FooWidget {}` → vi-compiler emits `compile_error!("unknown widget 'FooWidget'")`
- Existing 36 codegen tests vẫn pass
