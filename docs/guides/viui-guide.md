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

After layout, reactive widgets subscribe to their signals. Paint-only updates mark cached bounds dirty and repaint without layout. Text content updates also invalidate layout because proportional advances can change intrinsic size and sibling positions. A consumed input event and `ViApp::mark_dirty()` request a full layout and repaint.

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

GUI typography defaults to the bundled static Inter Regular font at 16px, rendered with grayscale coverage antialiasing. `ViNode::layout(constraints, &mut FontContext)` and the v1 `ViWidget` layout trait receive the same context used by paint; containers forward it to children. Widths include NFC normalization and kerning, and vertical alignment uses the font's line metrics. `ViApp::with_font(bytes, size)` replaces both layout and framebuffer paint typography; invalid bytes retain Inter.

Low-level `FramebufferCanvas::draw_text` remains an explicit bitmap/debug primitive; GUI widgets use `draw_text_scaled`. GPU command recording is not hardware text rendering: `CpuExecutor` caches Inter and rasterizes recorded text at playback, while the GLES2 backend explicitly reports unsupported scalable text. Recorded commands do not carry custom font identities, so custom fonts require the framebuffer renderer.

Each repaint clears its damage clip to the theme's opaque background before replaying the intersecting content, so grayscale glyph coverage does not accumulate and replaced text does not leave stale ink. Elm/window full-frame paths clear their whole surface. Recorded text stores its exact `f32` pixel size (including fractional and subpixel sizes); CPU playback preserves the recording clip stack.

Bundled fonts borrow static TTF data and rasterize outlines on demand using
`ab_glyph` (`no_std` + `libm`); only requested glyph bitmaps are cached.
Each face's bitmap cache is bounded to 512 entries and a 256KiB reclamation
threshold; a miss at the threshold discards old entries. Missing glyphs
share one entry rather than growing with unsupported Unicode scalars.
Layout reads font metrics without rasterizing or allocating. Cells still
need heap space for their widget tree and glyph cache; the desktop, console,
dashboard and managed demo initialize a 4MiB custom heap before allocations.
Supported scalable sizes are finite values in `(0, 128]` pixels. Custom font
contexts reject unsupported sizes; direct atlas calls fail explicitly before
rasterization. Each glyph's coverage allocation is checked and limited to
64KiB, in addition to the cache reclamation threshold.
The rasterizer does not perform OpenType GPOS/GSUB shaping or bidi layout.

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

## Linear theming, semantic tokens, and palettes

ViUI provides a unified `ViTheme` contract (`libs/viui/src/theme.rs`) defining semantic color tokens across the desktop shell and native applications. Palettes share identical layout geometry while providing distinct visual surfaces.

### Palettes and SystemPalette

The system defines three standard palettes via the `SystemPalette` enum:
- `SystemPalette::Light` (`LIGHT_THEME`): Clean light surface (`#FFFFFF` background, `#14141C` high-contrast primary text).
- `SystemPalette::Dark` (`DARK_THEME`): Low-glare dark surface (`#121216` background, `#F5F5FA` primary text).
- `SystemPalette::Paper` (`PAPER_THEME`): Warm sepia palette (`#F6F3EB` background, `#2B2B2A` ink text, `#9C5B28` terracotta accent) tailored for extended reading and writing.

Dynamic theme instantiation is provided by `theme_for_palette(palette: SystemPalette) -> Box<dyn ViTheme>`.

### Semantic token categories

`ViTheme` exposes semantic accessors rather than hardcoded hex colors:
- **Window and Tab Chrome:** `tab_active_bg()`, `tab_active_fg()`, `tab_inactive_bg()`, `tab_inactive_fg()`, `tab_border()`, `tab_close_hover()`.
- **Taskbar and Spotlight:** `taskbar_bg()`, `taskbar_fg()`, `taskbar_border()`, `spotlight_bg()`, `spotlight_border()`.
- **Focus and Status Badges:** `focus_ring()` (keyboard focus outline), `badge_dirty()` (unsaved changes), `badge_attention()` (notifications), `badge_error()` (errors and refusals).
- **Document Reader:** `reader_bg()`, `reader_text()`, `reader_subtext()`, and `reader_link()` (guaranteed strictly $\ge 5.0:1$ contrast against reader background).

### Preference state and revision monotonicity

`ThemePreferenceState` manages client-side preference tracking:
```rust
pub struct ThemePreferenceState {
    pub palette: SystemPalette,
    pub revision: u32,
}
```
`apply_update(new_palette, new_revision)` enforces monotonic revision sequencing and rejects duplicate or stale updates. `update_from_raw(raw_u8, new_revision)` converts untrusted input bytes, falling back safely to the documented default `Light` theme.

### WCAG 2.1 contrast compliance

All three standard palettes are verified by an automated mathematical relative luminance and contrast ratio test suite in `libs/viui/src/theme.rs`:
- Normal body text and secondary text on surface/background achieve $\ge 4.5:1$ contrast.
- Document hyperlinks (`reader_link`) achieve strictly $\ge 5.0:1$ contrast (Light 6.92:1, Dark 5.93:1, Paper 6.04:1).
- Active tabs, inactive tabs, and taskbar text achieve $\ge 4.5:1$ contrast.

### Window chrome geometry

`WindowChrome` (`libs/viui/src/window.rs`) provides client-side window decorations. To satisfy accessibility and pointer hit targets without compromising compact titlebar aesthetics, `close_btn_rect` defines a 20 DIP pointer hit area while `close_btn_icon_rect` centers a 16 DIP visual icon within that hit area.

## Further reading

- [ViUI contract](../specs/14-viui.md) — normative architecture, non-goals, and verification gates.
- [`cells/apps/robot-dashboard/src/main.rs`](../../cells/apps/robot-dashboard/src/main.rs) — reactive-v2 Cell with an app-owned display surface, explicit input collection, signal ownership, and frame driving.
- `libs/viui/src/signal.rs` — signal and computed-value lifetime details.
