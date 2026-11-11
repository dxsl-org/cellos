//! Compositor-internal frame membership. Every window lives in a frame (a
//! stack of Cell Tabs); the user stacks windows together and pulls tabs out.
//!
//! These operations check no owner: they run only on behalf of the user's
//! gestures in the compositor (pointer and window menu), never on a client's
//! request. A frame holding windows of several apps becomes compositor-owned
//! (`owner_tid == None`), which closes every owner-gated IPC view op on it.

use alloc::vec::Vec;
use api::display::ViewRef;

use super::{FrameRegistry, View, MAX_FRAMES, MAX_VIEWS_PER_FRAME};

// Every surface can be its own frame, so adoption never runs out of frames.
const _: () = assert!(crate::surface_table::MAX_SURFACES <= MAX_FRAMES);

/// A frame whose selected tab changed because its selected surface left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reselected {
    pub frame_id: u32,
    /// The surface now shown in the frame.
    pub selected: Option<u64>,
}

impl FrameRegistry {
    /// Put the new window `cap` of `owner` into a frame of its own.
    pub fn adopt(&mut self, cap: u64, owner: usize) -> Option<u32> {
        if self.by_cap.contains_key(&cap) || self.frames.len() >= MAX_FRAMES {
            return None;
        }
        let view_id = self.next_view_id;
        self.next_view_id = self.next_view_id.wrapping_add(1).max(1);
        self.views.insert(
            view_id,
            View {
                incarnation: 1,
                frame_id: 0,
                surface_cap: cap,
                owner_tid: owner,
                min_w: 0,
                min_h: 0,
            },
        );
        self.by_cap.insert(cap, view_id);
        let view = ViewRef {
            view_id,
            incarnation: 1,
        };
        Some(self.insert_single_frame(view, owner, (0, 0, 0, 0)))
    }

    /// Frame holding the window `cap`.
    pub fn frame_of(&self, cap: u64) -> Option<u32> {
        self.by_cap
            .get(&cap)
            .and_then(|id| self.views.get(id))
            .map(|view| view.frame_id)
    }

    /// Windows of `frame_id` in tab order.
    pub fn members(&self, frame_id: u32) -> Vec<u64> {
        self.frames.get(&frame_id).map_or_else(Vec::new, |frame| {
            frame
                .views
                .iter()
                .filter_map(|v| self.surface_for_view(v.view_id))
                .collect()
        })
    }

    /// The window `frame_id` shows.
    pub fn selected_cap(&self, frame_id: u32) -> Option<u64> {
        let frame = self.frames.get(&frame_id)?;
        self.surface_for_view(frame.selected_view?.view_id)
    }

    /// Whether the frame holding `cap` holds windows of several apps (it
    /// belongs to the compositor).
    pub fn is_shared(&self, cap: u64) -> bool {
        self.frame_of(cap)
            .and_then(|frame| self.frames.get(&frame))
            .is_some_and(|frame| frame.owner_tid.is_none())
    }

    /// Whether `cap` sits behind another tab of its frame.
    pub fn is_hidden(&self, cap: u64) -> bool {
        self.frame_of(cap)
            .is_some_and(|frame| self.selected_cap(frame) != Some(cap))
    }

    /// Show `cap` in its frame. Returns the window shown before, if it changed.
    pub fn select(&mut self, cap: u64) -> Option<u64> {
        let view_id = *self.by_cap.get(&cap)?;
        let frame_id = self.views.get(&view_id)?.frame_id;
        let previous = self.selected_cap(frame_id).filter(|&prior| prior != cap)?;
        let frame = self.frames.get_mut(&frame_id)?;
        frame.selected_view = frame.views.iter().find(|v| v.view_id == view_id).copied();
        frame.generation = frame.generation.wrapping_add(1);
        Some(previous)
    }

    /// Append every tab of frame `source` to frame `target`; the window
    /// `source` showed is shown in `target`. `source` is dropped.
    pub fn stack(&mut self, source: u32, target: u32) -> bool {
        let (Some(src), Some(dst)) = (self.frames.get(&source), self.frames.get(&target)) else {
            return false;
        };
        if source == target || src.views.len() + dst.views.len() > MAX_VIEWS_PER_FRAME {
            return false;
        }
        let Some(src) = self.frames.get(&source).cloned() else {
            return false;
        };
        // A frame stacked into another leaves its tile group.
        self.drop_frame(source);
        for view in &src.views {
            if let Some(entry) = self.views.get_mut(&view.view_id) {
                entry.frame_id = target;
            }
        }
        if let Some(dst) = self.frames.get_mut(&target) {
            dst.views.extend_from_slice(&src.views);
            dst.selected_view = src.selected_view.or(dst.selected_view);
            dst.generation = dst.generation.wrapping_add(1);
        }
        self.refresh(target);
        true
    }

    /// Pull `cap` out of its frame into a new frame of its own, shown there.
    /// Returns the new frame and the window its old frame shows now.
    pub fn detach(&mut self, cap: u64) -> Option<(u32, Reselected)> {
        let view_id = *self.by_cap.get(&cap)?;
        let source = self.views.get(&view_id)?.frame_id;
        if self.frames.get(&source)?.views.len() < 2 || self.frames.len() >= MAX_FRAMES {
            return None;
        }
        let reselected = self.unlink(view_id, source);
        let entry = self.views.get(&view_id)?;
        let view = ViewRef {
            view_id,
            incarnation: entry.incarnation,
        };
        let owner = entry.owner_tid;
        let frame_id = self.insert_single_frame(view, owner, (0, 0, 0, 0));
        Some((frame_id, reselected))
    }

    /// Forget the destroyed window `cap`. When it was the shown tab, the tab
    /// beside it is shown instead.
    pub fn remove_surface(&mut self, cap: u64) -> Option<Reselected> {
        let view_id = self.by_cap.remove(&cap)?;
        let frame_id = self.views.get(&view_id)?.frame_id;
        let reselected = self.unlink(view_id, frame_id);
        self.views.remove(&view_id);
        Some(reselected)
    }

    /// Take `view_id` out of `frame_id`, keeping it registered. The tab after
    /// it (else before it) is shown if it was the shown one.
    pub(super) fn unlink(&mut self, view_id: u32, frame_id: u32) -> Reselected {
        if let Some(frame) = self.frames.get_mut(&frame_id) {
            let index = frame.views.iter().position(|v| v.view_id == view_id);
            if let Some(index) = index {
                frame.views.remove(index);
                if frame.selected_view.is_some_and(|s| s.view_id == view_id) {
                    let next = index.min(frame.views.len().saturating_sub(1));
                    frame.selected_view = frame.views.get(next).copied();
                }
            }
            frame.generation = frame.generation.wrapping_add(1);
        }
        self.refresh(frame_id);
        Reselected {
            frame_id,
            selected: self.selected_cap(frame_id),
        }
    }

    /// How far the lone tab of `frame_id` was slid right.
    pub fn tab_offset(&self, frame_id: u32) -> u32 {
        self.tab_offsets.get(&frame_id).copied().unwrap_or(0)
    }

    /// Slide the lone tab of `frame_id`; a stack keeps its tabs spanning.
    pub fn set_tab_offset(&mut self, frame_id: u32, offset: u32) {
        if self
            .frames
            .get(&frame_id)
            .is_some_and(|frame| frame.views.len() == 1)
        {
            self.tab_offsets.insert(frame_id, offset);
        }
    }

    /// Queue `caps` to be asked to close, in order ("Close all tabs").
    pub fn set_close_queue(&mut self, frame_id: u32, mut caps: Vec<u64>) {
        caps.reverse();
        self.close_queue.insert(frame_id, caps);
    }

    /// Next queued tab still in `frame_id`, if any.
    pub fn pop_close_queue(&mut self, frame_id: u32) -> Option<u64> {
        loop {
            let queue = self.close_queue.get_mut(&frame_id)?;
            let Some(cap) = queue.pop() else {
                self.close_queue.remove(&frame_id);
                return None;
            };
            if self.frame_of(cap) == Some(frame_id) {
                return Some(cap);
            }
        }
    }

    /// Stop "Close all tabs" in `frame_id` (the user acted on a tab by hand).
    pub fn clear_close_queue(&mut self, frame_id: u32) {
        self.close_queue.remove(&frame_id);
    }

    /// Whether "Close all tabs" is still going through `frame_id`.
    pub fn is_closing_all(&self, frame_id: u32) -> bool {
        self.close_queue.contains_key(&frame_id)
    }

    /// Every indexed window has a view in an existing frame and a live surface.
    #[cfg(test)]
    pub fn assert_consistent(&self, table: &crate::surface_table::SurfaceTable) {
        for (&cap, view_id) in &self.by_cap {
            let view = self.views.get(view_id).expect("indexed view exists");
            assert_eq!(view.surface_cap, cap);
            let frame = self
                .frames
                .get(&view.frame_id)
                .expect("view's frame exists");
            assert!(frame.views.iter().any(|v| v.view_id == *view_id));
            assert!(table.get(cap).is_some(), "framed surface exists");
        }
    }
}
