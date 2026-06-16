// SPDX-License-Identifier: MIT
extern crate alloc;
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::cell::Cell;

use crate::canvas::Color;
use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;

pub struct TabEntry {
    pub label: String,
    builder:   Box<dyn Fn() -> Box<dyn ViNode>>,
    cached:    Option<Box<dyn ViNode>>,
}

/// Tab navigator with a bottom tab bar.
///
/// Pages are built lazily on first activation and kept alive for the lifetime
/// of the navigator — no rebuild on re-switch. Add tabs via the builder pattern:
/// `TabNavigator::new().add_tab("Home", || Box::new(home_page()))`.
pub struct TabNavigator {
    tabs:      Vec<TabEntry>,
    active:    usize,
    tab_bar_h: f32,
    bounds:    Cell<Rect>,
}

impl TabNavigator {
    pub fn new() -> Self {
        Self {
            tabs:      Vec::new(),
            active:    0,
            tab_bar_h: 48.0,
            bounds:    Cell::new(Rect::ZERO),
        }
    }

    pub fn add_tab(
        mut self,
        label: impl Into<String>,
        builder: impl Fn() -> Box<dyn ViNode> + 'static,
    ) -> Self {
        self.tabs.push(TabEntry {
            label:   label.into(),
            builder: Box::new(builder),
            cached:  None,
        });
        self
    }

    fn ensure_page(&mut self, idx: usize) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            if tab.cached.is_none() {
                tab.cached = Some((tab.builder)());
            }
        }
    }
}

impl ViNode for TabNavigator {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let full = Size::new(constraints.max.w, constraints.max.h);
        self.bounds.set(Rect::from_origin_size(constraints.origin, full));

        self.ensure_page(self.active);

        let page_h = (constraints.max.h - self.tab_bar_h).max(0.0);
        let page_constraints = Constraints::new(
            constraints.origin,
            Size::new(constraints.max.w, page_h),
        );

        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let Some(page) = &mut tab.cached {
                page.layout(page_constraints);
            }
        }

        full
    }

    fn bounds(&self) -> Rect { self.bounds.get() }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let b = self.bounds.get();

        // Active page content
        if let Some(tab) = self.tabs.get(self.active) {
            if let Some(page) = &tab.cached {
                page.paint(cx);
            }
        }

        // Bottom tab bar background
        let bar_y = b.y + b.h - self.tab_bar_h;
        cx.canvas.fill_rect(
            Rect { x: b.x, y: bar_y, w: b.w, h: self.tab_bar_h },
            Color::rgb(20, 22, 32),
        );

        let n = self.tabs.len();
        if n == 0 { return; }
        let tab_w = b.w / n as f32;

        for (i, tab) in self.tabs.iter().enumerate() {
            let tx = b.x + i as f32 * tab_w;
            let is_active = i == self.active;
            let text_color = if is_active {
                cx.theme.accent()
            } else {
                Color::rgb(120, 120, 150)
            };

            // Active indicator: 2 px line at top of bar
            if is_active {
                cx.canvas.fill_rect(
                    Rect { x: tx, y: bar_y, w: tab_w, h: 2.0 },
                    cx.theme.accent(),
                );
            }

            // Tab label centered horizontally
            let label_x = tx + (tab_w / 2.0) - (tab.label.chars().count() as f32 * 4.0);
            cx.draw_text(Point::new(label_x, bar_y + 16.0), &tab.label, text_color);
        }
    }

    fn event(&mut self, event: &Event) -> bool {
        let b = self.bounds.get();
        let bar_y = b.y + b.h - self.tab_bar_h;
        let bar_rect = Rect { x: b.x, y: bar_y, w: b.w, h: self.tab_bar_h };
        let n = self.tabs.len();

        let try_switch = |pos: Point| -> Option<usize> {
            if bar_rect.contains(pos) && n > 0 {
                let tab_w = b.w / n as f32;
                let idx = ((pos.x - b.x) / tab_w) as usize;
                if idx < n { Some(idx) } else { None }
            } else {
                None
            }
        };

        match event {
            Event::MousePress { pos, .. } | Event::TouchBegin { pos, .. } => {
                if let Some(idx) = try_switch(*pos) {
                    if idx != self.active {
                        self.active = idx;
                        self.ensure_page(idx);
                    }
                    return true; // bar always consumes touch/click
                }
            }
            _ => {}
        }

        // Forward to active page
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let Some(page) = &mut tab.cached {
                return page.event(event);
            }
        }
        false
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        // Only the active page needs to queue repaints while visible.
        let mut handles = Vec::new();
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let Some(page) = &mut tab.cached {
                handles.extend(page.collect_dirty_handles(Rc::clone(&region)));
            }
        }
        handles
    }

    fn collect_focusable_bounds(&mut self) -> Vec<Rect> {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let Some(page) = &mut tab.cached {
                return page.collect_focusable_bounds();
            }
        }
        Vec::new()
    }

    fn activate_at(&mut self, target: Rect) -> bool {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let Some(page) = &mut tab.cached {
                return page.activate_at(target);
            }
        }
        false
    }
}
