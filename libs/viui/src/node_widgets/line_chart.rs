// SPDX-License-Identifier: MIT
//! LineChart — time-series polyline widget driven by `Signal<Vec<f32>>`.
//!
//! Renders one polyline per series. Y axis auto-scales unless `y_range` is set.
//! Downsamples to plot pixel width via bucket averaging when data exceeds it.

extern crate alloc;
use alloc::{rc::Rc, string::String, vec::Vec};

use crate::canvas::{Color, TextStyle};
use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::{Signal, SubscriptionHandle};

/// One data series: reactive data, draw color, and display label.
pub struct Series {
    pub data:  Signal<Vec<f32>>,
    pub color: Color,
    pub label: String,
}

impl Series {
    pub fn new(data: Signal<Vec<f32>>, color: Color, label: impl Into<String>) -> Self {
        Self { data, color, label: label.into() }
    }
}

/// Time-series line chart. Subscribes to `Signal<Vec<f32>>` for each series.
///
/// Margins: left=40, bottom=24, top=8, right=8 pixels.
/// Grid: 5 horizontal lines at 0%, 25%, 50%, 75%, 100% of Y range.
pub struct LineChart {
    series:     Vec<Series>,
    y_min:      Option<f32>,
    y_max:      Option<f32>,
    grid_lines: bool,
    bounds:     Rect,
    subs:       Vec<SubscriptionHandle>,
}

impl LineChart {
    pub fn new(series: Vec<Series>) -> Self {
        Self {
            series,
            y_min: None,
            y_max: None,
            grid_lines: true,
            bounds: Rect::ZERO,
            subs: Vec::new(),
        }
    }

    /// Fix the Y axis range instead of auto-scaling.
    pub fn y_range(mut self, min: f32, max: f32) -> Self {
        self.y_min = Some(min);
        self.y_max = Some(max);
        self
    }

    pub fn grid(mut self, enabled: bool) -> Self {
        self.grid_lines = enabled;
        self
    }
}

/// Downsample `data` to at most `target_len` points via bucket averaging.
///
/// Invariant: divides by `count` only when `count > 0`. Empty buckets (which
/// can arise from integer rounding) return `0.0` rather than NaN.
fn downsample(data: &[f32], target_len: usize) -> Vec<f32> {
    if data.len() <= target_len {
        return data.to_vec();
    }
    let bucket = data.len() / target_len;
    (0..target_len).map(|i| {
        let start = i * bucket;
        let end   = if i == target_len - 1 { data.len() } else { (i + 1) * bucket };
        let count = end - start;
        if count == 0 { return 0.0; }   // guard: empty bucket → 0 rather than NaN
        data[start..end].iter().sum::<f32>() / count as f32
    }).collect()
}

impl ViNode for LineChart {
    fn layout(&mut self, constraints: Constraints) -> Size {
        self.bounds = Rect::from_origin_size(constraints.origin, constraints.max);
        constraints.max
    }

    fn bounds(&self) -> Rect { self.bounds }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let b = self.bounds;
        // Margins: left 40, bottom 24, top 8, right 8
        let (ml, mb, mt, mr) = (40.0f32, 24.0f32, 8.0f32, 8.0f32);
        let plot = Rect {
            x: b.x + ml,
            y: b.y + mt,
            w: (b.w - ml - mr).max(1.0),
            h: (b.h - mt - mb).max(1.0),
        };

        cx.canvas.fill_rect(b, Color::rgb(14, 16, 24));

        // Compute Y range across all series
        let (mut data_min, mut data_max) = (f32::MAX, f32::MIN);
        for s in &self.series {
            let d = s.data.get();
            for &v in d.iter() {
                if v < data_min { data_min = v; }
                if v > data_max { data_max = v; }
            }
        }
        if data_min == f32::MAX {
            data_min = 0.0;
            data_max = 1.0;
        }
        let y_lo = self.y_min.unwrap_or(data_min);
        let y_hi = self.y_max.unwrap_or(data_max);
        let y_range = (y_hi - y_lo).max(0.001);

        // 5 horizontal grid lines
        if self.grid_lines {
            let grid_color = Color::rgb(40, 44, 60);
            for i in 0..=4 {
                let t  = i as f32 / 4.0;
                let gy = plot.y + plot.h * (1.0 - t);
                cx.canvas.draw_line(
                    Point::new(plot.x, gy),
                    Point::new(plot.x + plot.w, gy),
                    grid_color,
                );
                let val   = y_lo + t * y_range;
                let label = format_f32_label(val);
                cx.canvas.draw_text(
                    Point::new(b.x + 2.0, gy - 6.0),
                    &label,
                    TextStyle { color: Color::rgb(120, 130, 150), size_px: 0 },
                );
            }
        }

        // Axis lines
        let axis_color = Color::rgb(60, 65, 85);
        cx.canvas.draw_line(
            Point::new(plot.x, plot.y),
            Point::new(plot.x, plot.y + plot.h),
            axis_color,
        );
        cx.canvas.draw_line(
            Point::new(plot.x, plot.y + plot.h),
            Point::new(plot.x + plot.w, plot.y + plot.h),
            axis_color,
        );

        // Plot each series as a polyline
        let plot_w_px = plot.w as usize;
        for s in &self.series {
            let data = s.data.get();
            if data.is_empty() { continue; }
            let pts = downsample(&data, plot_w_px.max(2));
            let n   = pts.len();
            for i in 1..n {
                let x0 = plot.x + (i - 1) as f32 / (n - 1) as f32 * plot.w;
                let x1 = plot.x + i         as f32 / (n - 1) as f32 * plot.w;
                let y0 = plot.y + plot.h * (1.0 - (pts[i - 1] - y_lo) / y_range);
                let y1 = plot.y + plot.h * (1.0 - (pts[i]     - y_lo) / y_range);
                cx.canvas.draw_line(Point::new(x0, y0), Point::new(x1, y1), s.color);
            }
        }
    }

    fn event(&mut self, _event: &Event) -> bool { false }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        self.subs.clear();
        let mut handles = Vec::new();
        let b = self.bounds;
        for s in &self.series {
            let r = Rc::clone(&region);
            handles.push(s.data.subscribe(move || {
                r.borrow_mut().mark(b);
            }));
        }
        handles
    }
}

/// Format a f32 value as a short axis label without std formatting.
///
/// Values ≥ 10 or ≤ -10 show as integer. Otherwise one decimal place.
fn format_f32_label(v: f32) -> String {
    if v.abs() >= 10.0 {
        alloc::format!("{}", v as i32)
    } else {
        let scaled = (v * 10.0) as i32;
        alloc::format!("{}.{}", scaled / 10, (scaled % 10).abs())
    }
}
