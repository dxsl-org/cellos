// SPDX-License-Identifier: MIT
//! BarChart — discrete comparison widget driven by `Signal<Vec<f32>>`.
//!
//! Renders one vertical bar per data point. Labels appear below each bar;
//! value annotations appear above bars taller than 14 pixels.

extern crate alloc;
use alloc::{rc::Rc, string::String, vec::Vec};

use crate::canvas::Color;
use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::{Signal, SubscriptionHandle};

/// Bar chart for discrete value comparison.
///
/// Bar width scales to fill available width with 4-pixel gaps between bars.
/// Y axis is scaled to the maximum data value unless overridden with `y_max`.
pub struct BarChart {
    data: Signal<Vec<f32>>,
    labels: Vec<String>,
    /// Bar fill; `None` uses the theme's `accent`.
    bar_color: Option<Color>,
    y_max: Option<f32>,
    show_values: bool,
    bounds: Rect,
    sub: Option<SubscriptionHandle>,
}

impl BarChart {
    pub fn new(data: Signal<Vec<f32>>) -> Self {
        Self {
            data,
            labels: Vec::new(),
            bar_color: None,
            y_max: None,
            show_values: true,
            bounds: Rect::ZERO,
            sub: None,
        }
    }

    pub fn labels(mut self, labels: Vec<impl Into<String>>) -> Self {
        self.labels = labels.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.bar_color = Some(color);
        self
    }

    pub fn y_max(mut self, max: f32) -> Self {
        self.y_max = Some(max);
        self
    }
}

impl ViNode for BarChart {
    fn layout(
        &mut self,
        constraints: Constraints,
        _font: &mut crate::font_context::FontContext,
    ) -> Size {
        self.bounds = Rect::from_origin_size(constraints.origin, constraints.max);
        constraints.max
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let b = self.bounds;
        let (mb, mt) = (24.0f32, 8.0f32);
        let plot_h = (b.h - mt - mb).max(1.0);
        let plot_y = b.y + mt;

        let data = self.data.get();
        if data.is_empty() {
            return;
        }

        let effective_max = self
            .y_max
            .unwrap_or_else(|| data.iter().cloned().fold(f32::MIN, f32::max).max(0.001));

        cx.canvas.fill_rect(b, cx.theme.surface());
        let bar_color = self.bar_color.unwrap_or_else(|| cx.theme.accent());
        let label_color = cx.theme.text_secondary();
        let value_color = cx.theme.text_primary();

        let n = data.len();
        let gap = 4.0f32;
        let bar_w = ((b.w - gap * (n + 1) as f32) / n as f32).max(1.0);

        for (i, &v) in data.iter().enumerate() {
            let bar_h = (v / effective_max * plot_h).max(0.0);
            let bx = b.x + gap + i as f32 * (bar_w + gap);
            let by = plot_y + plot_h - bar_h;

            cx.canvas.fill_rect(
                Rect {
                    x: bx,
                    y: by,
                    w: bar_w,
                    h: bar_h,
                },
                bar_color,
            );

            if let Some(label) = self.labels.get(i) {
                let x = bx + (bar_w - cx.measure(label)) * 0.5;
                cx.draw_text(Point::new(x, b.y + b.h - mb + 4.0), label, label_color);
            }

            if self.show_values && bar_h > cx.line_height() {
                let label = format_f32_short(v);
                let x = bx + (bar_w - cx.measure(&label)) * 0.5;
                let y = by - cx.line_height();
                cx.draw_text(Point::new(x, y), &label, value_color);
            }
        }
    }

    fn event(&mut self, _event: &Event) -> bool {
        false
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let b = self.bounds;
        let r = Rc::clone(&region);
        let sub = self.data.subscribe(move || {
            r.borrow_mut().mark(b);
        });
        self.sub = None; // old handle dropped, subscription pruned
        alloc::vec![sub]
    }
}

/// Format a f32 as a short one-decimal-place string without std.
fn format_f32_short(v: f32) -> String {
    let scaled = (v * 10.0) as i32;
    alloc::format!("{}.{}", scaled / 10, (scaled % 10).abs())
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::BarChart;

    use crate::canvas::Color;
    use crate::node_widgets::style::test_support::{fills, paint_node, text_color};
    use crate::signal::Signal;
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};

    #[test]
    fn plot_bars_and_labels_use_theme_tokens_unless_overridden() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let mut chart =
                BarChart::new(Signal::new(alloc::vec![4.0, 8.0])).labels(alloc::vec!["A", "B"]);
            let buf = paint_node(&mut chart, theme);
            let painted = fills(&buf);
            assert_eq!(painted[0].1, theme.surface());
            assert!(painted[1..].iter().all(|&(_, c)| c == theme.accent()));
            assert_eq!(text_color(&buf, "A"), Some(theme.text_secondary()));
            assert_eq!(text_color(&buf, "8.0"), Some(theme.text_primary()));

            let green = Color::rgb(0, 160, 0);
            let mut chart = BarChart::new(Signal::new(alloc::vec![1.0])).color(green);
            assert_eq!(fills(&paint_node(&mut chart, theme))[1].1, green);
        }
    }
}
