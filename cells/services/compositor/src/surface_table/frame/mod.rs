//! Generational window frames, multi-session tabs, and view registry.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use wire::display::{FramePlacement, Rect, ViewRef};

mod membership;
#[cfg(test)]
mod membership_tests;
mod placement;
#[cfg(test)]
mod placement_tests;
mod regroup;
#[cfg(test)]
mod tests;
mod tile;
mod views;

pub use membership::Reselected;
pub use tile::{Join, Side};

pub const MAX_VIEWS_PER_FRAME: usize = 32;
pub const MAX_FRAMES: usize = 64;

#[derive(Debug, Clone)]
pub struct Frame {
    pub generation: u32,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub views: Vec<ViewRef>,
    pub selected_view: Option<ViewRef>,
    pub min_w: u32,
    pub min_h: u32,
    pub placement: FramePlacement,
    pub saved_floating_rect: Option<Rect>,
    pub saved_placement: Option<FramePlacement>,
    /// The one task owning every view in the frame, re-derived on every
    /// membership change. Only it may add views to the frame or select, snap
    /// or fullscreen it over IPC. `None` once the user stacked windows of
    /// several apps together: such a frame belongs to the compositor and no
    /// client may plant a tab in, re-stack or move another client's window.
    pub owner_tid: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct View {
    pub incarnation: u32,
    pub frame_id: u32,
    pub surface_cap: u64,
    pub owner_tid: usize,
    /// Minimum content size the view asked for; the frame's minimum is the
    /// largest over its views.
    pub min_w: u32,
    pub min_h: u32,
}

#[derive(Default)]
pub struct FrameRegistry {
    frames: BTreeMap<u32, Frame>,
    views: BTreeMap<u32, View>,
    /// Surface cap -> view id, for every surface that is a view.
    by_cap: BTreeMap<u64, u32>,
    /// Tabs of a frame still to be asked by "Close all tabs", in order.
    close_queue: BTreeMap<u32, Vec<u64>>,
    /// How far a lone tab was slid along its frame's top edge.
    tab_offsets: BTreeMap<u32, u32>,
    /// Frames glued edge to edge (tile groups).
    joins: Vec<tile::Join>,
    /// Frames the user minimized together with their tile group.
    group_minimized: Vec<u32>,
    next_frame_id: u32,
    next_view_id: u32,
}

impl FrameRegistry {
    pub fn new() -> Self {
        Self {
            frames: BTreeMap::new(),
            views: BTreeMap::new(),
            by_cap: BTreeMap::new(),
            close_queue: BTreeMap::new(),
            tab_offsets: BTreeMap::new(),
            joins: Vec::new(),
            group_minimized: Vec::new(),
            next_frame_id: 1,
            next_view_id: 1,
        }
    }

    /// Look up backing surface cap for a view.
    pub fn surface_for_view(&self, view_id: u32) -> Option<u64> {
        self.views.get(&view_id).map(|v| v.surface_cap)
    }

    #[allow(dead_code)]
    pub fn frame_for_view(&self, view_id: u32) -> Option<u32> {
        self.views.get(&view_id).map(|v| v.frame_id)
    }

    #[allow(dead_code)]
    pub fn selected_view_for_frame(&self, frame_id: u32) -> Option<ViewRef> {
        self.frames.get(&frame_id).and_then(|f| f.selected_view)
    }

    /// Remove every view owned by a dying task. Returns the frames whose
    /// selected tab changed to another live surface, as for
    /// [`Self::remove_surface`].
    pub fn cleanup_owner(&mut self, dead_tid: usize) -> Vec<Reselected> {
        let dead: Vec<u64> = self
            .views
            .values()
            .filter(|view| view.owner_tid == dead_tid)
            .map(|view| view.surface_cap)
            .collect();
        let mut reselected: Vec<Reselected> = Vec::new();
        for cap in dead {
            if let Some(change) = self.remove_surface(cap) {
                reselected.retain(|prior| prior.frame_id != change.frame_id);
                if change
                    .selected
                    .is_some_and(|selected| self.frame_of(selected).is_some())
                {
                    reselected.push(change);
                }
            }
        }
        reselected.retain(|change| {
            change
                .selected
                .is_some_and(|selected| self.frame_of(selected) == Some(change.frame_id))
        });
        reselected
    }

    /// Forget `frame_id` and everything keyed by it; it leaves its tile group.
    fn drop_frame(&mut self, frame_id: u32) {
        self.frames.remove(&frame_id);
        self.close_queue.remove(&frame_id);
        self.tab_offsets.remove(&frame_id);
        self.untile(frame_id);
    }

    fn allocate_frame_id(&mut self) -> u32 {
        let fid = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1).max(1);
        fid
    }

    /// Re-derive the frame's owner and minimum size from its views after its
    /// membership changed; drop it once empty.
    fn refresh(&mut self, frame_id: u32) {
        let Some(frame) = self.frames.get_mut(&frame_id) else {
            return;
        };
        if frame.views.is_empty() {
            self.drop_frame(frame_id);
            return;
        }
        if frame.views.len() > 1 {
            // A stack's tabs span the frame; a lone tab again starts left.
            self.tab_offsets.remove(&frame_id);
        }
        let mut owner = None;
        let mut shared = false;
        let (mut min_w, mut min_h) = (0, 0);
        for view in frame
            .views
            .iter()
            .filter_map(|v| self.views.get(&v.view_id))
        {
            shared |= owner.is_some_and(|tid| tid != view.owner_tid);
            owner = Some(view.owner_tid);
            min_w = min_w.max(view.min_w);
            min_h = min_h.max(view.min_h);
        }
        frame.owner_tid = if shared { None } else { owner };
        frame.min_w = min_w;
        frame.min_h = min_h;
        if !frame
            .selected_view
            .is_some_and(|s| frame.views.contains(&s))
        {
            frame.selected_view = frame.views.first().copied();
        }
    }
}
