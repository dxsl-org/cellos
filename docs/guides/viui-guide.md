# ViUI Reactive-v2 Guide

> ViUI's canonical native UI API is a `no_std + alloc` retained `ViNode` tree driven by `Signal<T>` values.

## Availability and boundaries

The reactive-v2 library surface is implemented. Product or platform qualification is separate: it still requires the supported build lane, signed App Cell, input-to-render integration, compositor damage validation, and target-hardware evidence. This guide makes no performance, compositor-availability, or deployment guarantee.

ViUI has two deliberately separate APIs:

- **Reactive-v2 (canonical):** `Signal<T>`, `ViNode`, `app_runner::ViApp`, and `node_widgets` construct a retained tree and are the APIs used below.
- **Legacy Elm API:** `elm::{ViApp, Element}` and the `prelude` expose the earlier message-based interface. Keep it isolated in applications that already use it; do not combine its widgets or runner with reactive-v2 nodes.

Neither API creates a hidden event loop. The App Cell owns its surface, collects input, and explicitly calls `tick()` or `tick_with_dt()`.

## Reactive-v2 application

Import reactive-v2 types explicitly. In particular, `app_runner::ViApp` is distinct from the legacy `elm::ViApp` trait re-exported by `prelude`.

```rust

extern crate alloc;

use alloc::{boxed::Box, string::String, vec::Vec};
use viui::{
    app_runner::ViApp,
    event::Event,
    node::ViNode,
    node_widgets::{button::Button, column::Column, label::Label},
    signal::{Signal, SubscriptionHandle},
    surface_renderer::ViSurfaceRenderer,
};

fn build_headless_ui() {
let count = Signal::new(0i32);
let increment = count.clone();
let (count_text, count_text_subscription): (Signal<String>, SubscriptionHandle) = count
    .map(|value| alloc::format!("Count: {value}"))
    .into_parts();

let mut children: Vec<Box<dyn ViNode>> = Vec::new();
children.push(Box::new(Label::new(count_text)));
children.push(Box::new(Button::new("Increment", move || {
    increment.update(|value| *value += 1);
})));

let root = Column::new(children).with_spacing(8.0);
let mut app = ViApp::new(Box::new(root), Box::new(ViSurfaceRenderer::new(320, 120)));

// Keep this handle alive for as long as the derived signal must update.
let _subscriptions = [count_text_subscription];
let events: [Event; 0] = [];
app.tick(&events); // First tick lays out and renders the complete frame.
}
```

`ViSurfaceRenderer` owns an in-process BGRA pixel buffer and is the headless/test renderer. A displayed App Cell instead creates its own `ostd::display::ViSurface` and passes it to `renderer::FramebufferRenderer::new`. ViUI paints into that app-owned surface; the compositor does not own the widget tree or application state.

### Signals, derived values, and subscriptions

`Signal::set` and `Signal::update` synchronously notify live subscribers before returning. Clones share the same value. A subscription remains active only while its `SubscriptionHandle` is stored somewhere live:

```rust
use viui::signal::Signal;

fn derived_signal_updates_synchronously() {
let temperature = Signal::new(20i32);
let alarm = temperature.map(|value| *value >= 80);

assert!(!*alarm.get());
temperature.set(90);
assert!(*alarm.get());
}
```

Use `Computed::into_parts()` when a widget needs the derived `Signal<T>` itself, and retain the returned handle as in the application example. Calling `set` from a subscriber updates the stored value but does not recursively start another notification pass; schedule dependent work through the App Cell's next frame when it needs a separate pass.

After layout, reactive widgets subscribe to their signals. A signal-only update marks the cached widget bounds dirty and the next tick repaints that region without a layout pass. A consumed input event and `ViApp::mark_dirty()` request a full layout and repaint.

### Explicit input and frame driving

A Cell collects its own input and decides when to render. A typical loop buffers events until its frame deadline, then advances animations and renders:

```rust
extern crate alloc;

use alloc::vec::Vec;
use viui::{app_runner::ViApp, event::Event};

fn tick_frame(app: &mut ViApp, elapsed_ms: u32) {
    let mut pending_events: Vec<Event> = Vec::new();
    pending_events.extend(viui::input_bridge::collect_input_events(32));
    app.tick_with_dt(&pending_events, elapsed_ms);
}
```

`tick()` is equivalent to `tick_with_dt(events, 0)`. It does not advance animations. Both methods return `true` exactly when a frame was rendered, so an application can use that result for any app-owned presentation bookkeeping.

## Nodes and widgets

`ViNode` implementations cache their final `Rect` in `layout()`. They use that same rectangle for `paint()`, hit testing, and signal dirty subscriptions. Node widgets include labels, buttons, checkboxes, sliders, progress bars, text editing, lists, charts, navigation, and overlays.

Buttons invoke their callback synchronously only after a left press followed by a left release inside their bounds. Signal-driven labels and other reactive widgets mark their cached bounds dirty when their source signals change.

## Declarative `.vi` components

The `vi_design!` macro takes one raw string containing current `.vi` syntax and emits Rust types; it is not JSX-like Rust markup. A raw string is required for interpolation:

```rust
extern crate alloc;

use viui::vi_design;

vi_design!(r#"
component Counter {
    in-out property <int> count: 0;
    VerticalLayout {
        Text { text: "Count: \{count}"; color: #ffffff; }
        Button { text: "Increment"; clicked => { count += 1; } }
    }
}
"#);

fn build_counter() {
let (_state, _ui) = Counter::build();
}
```

For standalone `.vi` files, use `viui-build` from `build.rs`. Both authoring paths generate ordinary Rust/node structures; they do not add another runtime or compositor protocol.

## Further reading

- [ViUI contract](../specs/14-viui.md) — normative architecture, non-goals, and verification gates.
- [`cells/apps/robot-dashboard/src/main.rs`](../../cells/apps/robot-dashboard/src/main.rs) — reactive-v2 Cell with an app-owned display surface, explicit input collection, signal ownership, and frame driving.
- `libs/viui/src/signal.rs` — signal and computed-value lifetime details.
