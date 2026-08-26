# Phase 03 — Charts: LineChart + BarChart

**Status:** Planned  
**Wave:** G1.1 (parallel với P01, P02, P04)  
**Priority:** High  
**Estimate:** 2-3 ngày  
**Depends on:** Không (canvas API đã có, Signal<T> đã có)

---

## Context Links

- Canvas API: `libs/viui/src/canvas.rs`
- Signal: `libs/viui/src/signal.rs`
- Existing widget pattern: `libs/viui/src/node_widgets/progress_bar.rs`
- Robot dashboard (target consumer): `cells/apps/robot-dashboard/src/main.rs`
- vi-compiler codegen: `tools/vi-compiler/src/codegen.rs`

---

## Overview

Robot dashboard và server dashboard đều cần hiển thị sensor history theo thời gian. Hiện tại chỉ có ProgressBar (scalar hiện tại, không có history). LineChart cho phép plot time-series data. BarChart cho so sánh discrete values.

Hai widgets này dùng canvas software rendering — không cần GPU. Tất cả trong no_std/alloc.

---

## Key Insights

- Data source: `Signal<Vec<f32>>` — series của float values. Thay đổi khi data mới đến.
- Multi-series: `Vec<Series>` = `{ data: Signal<Vec<f32>>, color: Color, label: String }`. Hiện tại G1 cần 1-3 series (battery, cpu, motor temp).
- Canvas API có `draw_line(x1, y1, x2, y2, color)` và `fill_rect(...)`. LineChart là polyline. BarChart là fill_rect.
- Auto-scaling: min/max computed từ data range, trừ khi explicit `y_min/y_max` set.
- Downsampling: nếu `data.len() > chart_width_px`, downsample về 1 point/pixel (LTTB algorithm hoặc simple average).
- Axes labels: optional text labels (dùng canvas::draw_text). Skip nếu space quá nhỏ.
- Không cần interactivity G1 (hover tooltip là G2+).

---

## Architecture

### LineChart Widget

```
libs/viui/src/node_widgets/line_chart.rs
```

```rust
pub struct Series {
    pub data:   Signal<Vec<f32>>,
    pub color:  Color,
    pub label:  String,
}

pub struct LineChart {
    series:      Vec<Series>,
    y_min:       Option<f32>,    // None = auto
    y_max:       Option<f32>,    // None = auto
    x_labels:    Vec<String>,    // bottom labels (time ticks)
    grid_lines:  bool,
    background:  Color,
    // Layout cache
    bounds:      Cell<Rect>,
    // Subscriptions
    subs:        Vec<SubscriptionHandle>,
    dirty:       Rc<Cell<bool>>,
}

impl LineChart {
    pub fn new(series: Vec<Series>) -> Self
    pub fn y_range(mut self, min: f32, max: f32) -> Self
    pub fn grid(mut self, enabled: bool) -> Self
    pub fn x_labels(mut self, labels: Vec<String>) -> Self
}
```

**Rendering algorithm:**
1. Compute `y_range = (effective_min, effective_max)` từ tất cả series data (nếu auto)
2. Compute plot area = bounds minus axis margins (left 40px, bottom 24px, top 8px, right 8px)
3. For each series: map `data[i]` → pixel coords, draw polyline
4. Draw grid lines (horizontal, mỗi 20% y range)
5. Draw axis labels (y: 5 ticks, x: từ x_labels hoặc index)

**Downsampling** (khi `data.len() > plot_width`):
```rust
fn downsample(data: &[f32], target_len: usize) -> Vec<f32> {
    // Simple: bucket average
    let bucket = data.len() / target_len;
    (0..target_len).map(|i| {
        let slice = &data[i*bucket..(i+1)*bucket];
        slice.iter().sum::<f32>() / slice.len() as f32
    }).collect()
}
```

### BarChart Widget

```
libs/viui/src/node_widgets/bar_chart.rs
```

```rust
pub struct BarChart {
    data:       Signal<Vec<f32>>,
    labels:     Vec<String>,
    bar_color:  Color,
    y_max:      Option<f32>,
    show_values: bool,   // hiện số trên thanh bar
    bounds:     Cell<Rect>,
    sub:        Option<SubscriptionHandle>,
    dirty:      Rc<Cell<bool>>,
}

impl BarChart {
    pub fn new(data: Signal<Vec<f32>>) -> Self
    pub fn labels(mut self, labels: Vec<String>) -> Self
    pub fn color(mut self, color: Color) -> Self
    pub fn y_max(mut self, max: f32) -> Self
}
```

Bar rendering: fixed gap (4px) giữa bars, auto bar_width = `(plot_width - gaps) / bar_count`.

### DSL codegen additions

Thêm vào `map_element()` trong `tools/vi-compiler/src/codegen.rs`:
- `"LineChart"` → `ConstructStyle::Container` với series property
- `"BarChart"` → `ConstructStyle::SignalFirst` với Signal<Vec<f32>>

Note: Series phức tạp (nested struct) — DSL support có thể limited trong G1, Rust API đủ.

---

## Related Code Files

### Tạo mới
- `libs/viui/src/node_widgets/line_chart.rs` — LineChart, Series struct
- `libs/viui/src/node_widgets/bar_chart.rs` — BarChart

### Sửa
- `libs/viui/src/node_widgets.rs` — pub use line_chart::LineChart, bar_chart::BarChart
- `tools/vi-compiler/src/codegen.rs` — thêm LineChart + BarChart vào map_element
- `tools/vi-compiler/tests/codegen_tests.rs` — 2 tests mới
- `cells/apps/robot-dashboard/src/main.rs` — thêm LineChart hiển thị battery history

---

## Implementation Steps

1. **line_chart.rs** — Series struct, LineChart struct, builder methods
2. **LineChart::ViNode impl** — layout() (cache bounds), paint() (polyline rendering), event() (no-op G1), collect_dirty_handles()
3. **Downsampling helper** — `downsample(data: &[f32], target: usize) -> Vec<f32>`
4. **bar_chart.rs** — BarChart struct, builder, ViNode impl
5. **node_widgets.rs** — thêm pub use
6. **codegen.rs** — thêm LineChart + BarChart entries
7. **Codegen tests** — line_chart_codegen, bar_chart_codegen
8. **Robot dashboard update** — ring buffer cho battery_history: Signal<Vec<f32>>, mount LineChart
9. **cargo check** toàn bộ workspace

---

## Todo List

- [ ] Tạo `line_chart.rs`: Series, LineChart struct, builders
- [ ] Implement LineChart ViNode: layout, paint (polyline + grid + labels), event, dirty
- [ ] Implement downsample helper (bucket average)
- [ ] Tạo `bar_chart.rs`: BarChart struct, builders, ViNode impl
- [ ] Update `node_widgets.rs`: pub use LineChart, BarChart
- [ ] Update `codegen.rs`: thêm LineChart, BarChart
- [ ] Viết 2 codegen tests
- [ ] Update robot-dashboard: ring buffer `Vec<f32>` max 120 entries, LineChart với 3 series (battery, cpu, motor)
- [ ] `cargo check -p viui` pass

---

## Success Criteria

- LineChart với 3 series render đúng màu, không overlap axes
- Data thay đổi → chart repaint ngay (Signal subscription)
- Nếu data.len() > plot_width → downsample không crash
- BarChart với 5 bars render correctly, labels căn giữa mỗi bar
- Robot-dashboard: battery history plot cập nhật real-time
- `cargo check` pass, 2 codegen tests pass

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| canvas.draw_line không có anti-aliasing → jagged lines | High | Chấp nhận G1, note trong docs. G2: Bresenham smooth |
| y_range = 0 (all data same value) → div by zero | Low | Guard: nếu range < 0.001, dùng range = 1.0 |
| Many series → paint slow | Low | G1 max 3 series, < 1000 points, đủ cho 30fps |
| DSL Series binding phức tạp | Medium | Giới hạn DSL support: chỉ single series trong G1 DSL, multi-series qua Rust API |

---

## Security Considerations

Không relevant — pure rendering, data từ nội bộ Signal, không parse external input.

---

## Next Steps

Sau P03: Charts sẵn sàng cho server dashboard (G2). Kết hợp với P05 Virtual ListView để handle large time-series data sets.
