//! Generational window frames, multi-session tabs, and view registry.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use api::display::{
    CreateViewRequest, SurfaceRole, ViewOpResponse, ViewOpResult, ViewRef,
};

pub const MAX_VIEWS_PER_FRAME: usize = 32;
pub const MAX_FRAMES: usize = 64;

#[derive(Debug, Clone)]
pub struct Frame {
    pub frame_id: u32,
    pub generation: u32,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub views: Vec<ViewRef>,
    pub selected_view: Option<ViewRef>,
    pub min_w: u32,
    pub min_h: u32,
}

#[derive(Debug, Clone)]
pub struct View {
    pub view_id: u32,
    pub incarnation: u32,
    pub frame_id: u32,
    pub surface_cap: u64,
    pub owner_tid: usize,
    pub role: SurfaceRole,
    pub min_w: u32,
    pub min_h: u32,
}

#[derive(Default)]
pub struct FrameRegistry {
    frames: BTreeMap<u32, Frame>,
    views: BTreeMap<u32, View>,
    next_frame_id: u32,
    next_view_id: u32,
}

impl FrameRegistry {
    pub fn new() -> Self {
        Self {
            frames: BTreeMap::new(),
            views: BTreeMap::new(),
            next_frame_id: 1,
            next_view_id: 1,
        }
    }

    /// Allocate a new view and optionally attach it to an existing or new frame.
    pub fn create_view(
        &mut self,
        req: &CreateViewRequest,
        caller_tid: usize,
        surface_cap: u64,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        let view_id = self.next_view_id;
        self.next_view_id = self.next_view_id.wrapping_add(1);
        if self.next_view_id == 0 {
            self.next_view_id = 1;
        }
        let view_ref = ViewRef {
            view_id,
            incarnation: 1,
        };

        let target_frame_id = if req.frame_id == 0 {
            if self.frames.len() >= MAX_FRAMES {
                return Err(ViewOpResult::CapacityExhausted);
            }
            let fid = self.next_frame_id;
            self.next_frame_id = self.next_frame_id.wrapping_add(1);
            if self.next_frame_id == 0 {
                self.next_frame_id = 1;
            }
            let frame = Frame {
                frame_id: fid,
                generation: 1,
                x: 100,
                y: 100,
                w: req.w,
                h: req.h,
                views: alloc::vec![view_ref],
                selected_view: Some(view_ref),
                min_w: req.min_w,
                min_h: req.min_h,
            };
            self.frames.insert(fid, frame);
            fid
        } else {
            let frame = self.frames.get_mut(&req.frame_id).ok_or(ViewOpResult::FrameNotFound)?;
            if frame.generation != req.frame_generation {
                return Err(ViewOpResult::StaleGeneration);
            }
            if frame.views.len() >= MAX_VIEWS_PER_FRAME {
                return Err(ViewOpResult::CapacityExhausted);
            }
            frame.generation = frame.generation.wrapping_add(1);
            frame.views.push(view_ref);
            frame.min_w = frame.min_w.max(req.min_w);
            frame.min_h = frame.min_h.max(req.min_h);
            if frame.selected_view.is_none() {
                frame.selected_view = Some(view_ref);
            }
            req.frame_id
        };

        let frame_gen = self.frames.get(&target_frame_id).map_or(1, |f| f.generation);
        let view = View {
            view_id,
            incarnation: 1,
            frame_id: target_frame_id,
            surface_cap,
            owner_tid: caller_tid,
            role: req.role,
            min_w: req.min_w,
            min_h: req.min_h,
        };
        self.views.insert(view_id, view);

        Ok(ViewOpResponse {
            opcode: api::display::compositor_ops::CREATE_VIEW,
            result: ViewOpResult::Success,
            _pad: [0; 2],
            frame_id: target_frame_id,
            frame_generation: frame_gen,
            view_id,
            view_incarnation: 1,
        })
    }

    /// Destroy a view and clean up the frame if empty.
    pub fn destroy_view(
        &mut self,
        frame_id: u32,
        frame_gen: u32,
        view_id: u32,
        view_inc: u32,
        caller_tid: usize,
    ) -> Result<(ViewOpResponse, u64), ViewOpResult> {
        let view = self.views.get(&view_id).ok_or(ViewOpResult::ViewNotFound)?;
        if view.incarnation != view_inc {
            return Err(ViewOpResult::StaleGeneration);
        }
        if view.owner_tid != caller_tid {
            return Err(ViewOpResult::AccessDenied);
        }
        if view.frame_id != frame_id {
            return Err(ViewOpResult::FrameNotFound);
        }

        let frame = self.frames.get_mut(&frame_id).ok_or(ViewOpResult::FrameNotFound)?;
        if frame.generation != frame_gen {
            return Err(ViewOpResult::StaleGeneration);
        }

        frame.generation = frame.generation.wrapping_add(1);
        frame.views.retain(|v| v.view_id != view_id);
        if frame.selected_view.map_or(false, |v| v.view_id == view_id) {
            frame.selected_view = frame.views.first().copied();
        }

        let new_gen = frame.generation;
        let frame_empty = frame.views.is_empty();
        let surface_cap = view.surface_cap;
        self.views.remove(&view_id);
        if frame_empty {
            self.frames.remove(&frame_id);
        }

        Ok((
            ViewOpResponse {
                opcode: api::display::compositor_ops::DESTROY_VIEW,
                result: ViewOpResult::Success,
                _pad: [0; 2],
                frame_id,
                frame_generation: if frame_empty { frame_gen } else { new_gen },
                view_id,
                view_incarnation: view_inc,
            },
            surface_cap,
        ))
    }

    /// Select active tab view within a frame.
    pub fn select_view(
        &mut self,
        frame_id: u32,
        frame_gen: u32,
        view_id: u32,
        view_inc: u32,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        let view = self.views.get(&view_id).ok_or(ViewOpResult::ViewNotFound)?;
        if view.incarnation != view_inc {
            return Err(ViewOpResult::StaleGeneration);
        }
        if view.frame_id != frame_id {
            return Err(ViewOpResult::FrameNotFound);
        }

        let frame = self.frames.get_mut(&frame_id).ok_or(ViewOpResult::FrameNotFound)?;
        if frame.generation != frame_gen {
            return Err(ViewOpResult::StaleGeneration);
        }

        let view_ref = ViewRef {
            view_id,
            incarnation: view_inc,
        };
        frame.selected_view = Some(view_ref);

        Ok(ViewOpResponse {
            opcode: api::display::compositor_ops::SELECT_VIEW,
            result: ViewOpResult::Success,
            _pad: [0; 2],
            frame_id,
            frame_generation: frame_gen,
            view_id,
            view_incarnation: view_inc,
        })
    }

    /// Move a view from source frame to target frame.
    pub fn group_view(
        &mut self,
        target_frame_id: u32,
        target_frame_gen: u32,
        source_frame_id: u32,
        source_frame_gen: u32,
        view_id: u32,
        view_inc: u32,
        caller_tid: usize,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        let view = self.views.get(&view_id).ok_or(ViewOpResult::ViewNotFound)?;
        if view.incarnation != view_inc {
            return Err(ViewOpResult::StaleGeneration);
        }
        if view.owner_tid != caller_tid {
            return Err(ViewOpResult::AccessDenied);
        }
        if view.frame_id != source_frame_id {
            return Err(ViewOpResult::FrameNotFound);
        }
        if target_frame_id == source_frame_id {
            let frame = self.frames.get(&target_frame_id).ok_or(ViewOpResult::FrameNotFound)?;
            return Ok(ViewOpResponse {
                opcode: api::display::compositor_ops::GROUP_VIEW,
                result: ViewOpResult::Success,
                _pad: [0; 2],
                frame_id: target_frame_id,
                frame_generation: frame.generation,
                view_id,
                view_incarnation: view_inc,
            });
        }

        {
            let src = self.frames.get(&source_frame_id).ok_or(ViewOpResult::FrameNotFound)?;
            if src.generation != source_frame_gen {
                return Err(ViewOpResult::StaleGeneration);
            }
            let dst = self.frames.get(&target_frame_id).ok_or(ViewOpResult::FrameNotFound)?;
            if dst.generation != target_frame_gen {
                return Err(ViewOpResult::StaleGeneration);
            }
            if dst.views.len() >= MAX_VIEWS_PER_FRAME {
                return Err(ViewOpResult::CapacityExhausted);
            }
        }

        let view_ref = ViewRef {
            view_id,
            incarnation: view_inc,
        };

        // Remove from source
        let src_empty = {
            let src = self.frames.get_mut(&source_frame_id).ok_or(ViewOpResult::FrameNotFound)?;
            src.generation = src.generation.wrapping_add(1);
            src.views.retain(|v| v.view_id != view_id);
            if src.selected_view.map_or(false, |v| v.view_id == view_id) {
                src.selected_view = src.views.first().copied();
            }
            src.views.is_empty()
        };
        if src_empty {
            self.frames.remove(&source_frame_id);
        }

        // Add to target
        let dst_gen = {
            let dst = self.frames.get_mut(&target_frame_id).ok_or(ViewOpResult::FrameNotFound)?;
            dst.generation = dst.generation.wrapping_add(1);
            dst.views.push(view_ref);
            dst.selected_view = Some(view_ref);
            dst.generation
        };
        if let Some(v) = self.views.get_mut(&view_id) {
            v.frame_id = target_frame_id;
        }

        Ok(ViewOpResponse {
            opcode: api::display::compositor_ops::GROUP_VIEW,
            result: ViewOpResult::Success,
            _pad: [0; 2],
            frame_id: target_frame_id,
            frame_generation: dst_gen,
            view_id,
            view_incarnation: view_inc,
        })
    }

    /// Detach a view into a new floating frame.
    pub fn detach_view(
        &mut self,
        source_frame_id: u32,
        source_frame_gen: u32,
        view_id: u32,
        view_inc: u32,
        x: i32,
        y: i32,
        caller_tid: usize,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        let view = self.views.get(&view_id).ok_or(ViewOpResult::ViewNotFound)?;
        if view.incarnation != view_inc {
            return Err(ViewOpResult::StaleGeneration);
        }
        if view.owner_tid != caller_tid {
            return Err(ViewOpResult::AccessDenied);
        }
        if view.frame_id != source_frame_id {
            return Err(ViewOpResult::FrameNotFound);
        }

        if self.frames.len() >= MAX_FRAMES {
            return Err(ViewOpResult::CapacityExhausted);
        }

        let view_ref = ViewRef {
            view_id,
            incarnation: view_inc,
        };

        // Remove from source
        let (w, h, min_w, min_h, src_empty) = {
            let src = self.frames.get_mut(&source_frame_id).ok_or(ViewOpResult::FrameNotFound)?;
            if src.generation != source_frame_gen {
                return Err(ViewOpResult::StaleGeneration);
            }
            src.generation = src.generation.wrapping_add(1);
            src.views.retain(|v| v.view_id != view_id);
            if src.selected_view.map_or(false, |v| v.view_id == view_id) {
                src.selected_view = src.views.first().copied();
            }
            (src.w, src.h, src.min_w, src.min_h, src.views.is_empty())
        };
        if src_empty {
            self.frames.remove(&source_frame_id);
        }

        let new_fid = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1);
        if self.next_frame_id == 0 {
            self.next_frame_id = 1;
        }

        let new_frame = Frame {
            frame_id: new_fid,
            generation: 1,
            x,
            y,
            w,
            h,
            views: alloc::vec![view_ref],
            selected_view: Some(view_ref),
            min_w,
            min_h,
        };
        self.frames.insert(new_fid, new_frame);

        if let Some(v) = self.views.get_mut(&view_id) {
            v.frame_id = new_fid;
        }

        Ok(ViewOpResponse {
            opcode: api::display::compositor_ops::DETACH_VIEW,
            result: ViewOpResult::Success,
            _pad: [0; 2],
            frame_id: new_fid,
            frame_generation: 1,
            view_id,
            view_incarnation: view_inc,
        })
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

    pub fn frame_position(&self, frame_id: u32) -> Option<(i32, i32)> {
        self.frames.get(&frame_id).map(|f| (f.x, f.y))
    }

    /// Clean up all views owned by a dying TID.
    pub fn cleanup_owner(&mut self, dead_tid: usize) -> Vec<u64> {
        let mut dead_caps = Vec::new();
        let mut dead_view_ids = Vec::new();

        for (&vid, view) in &self.views {
            if view.owner_tid == dead_tid {
                dead_view_ids.push(vid);
                dead_caps.push(view.surface_cap);
            }
        }

        for vid in dead_view_ids {
            if let Some(view) = self.views.remove(&vid) {
                if let Some(frame) = self.frames.get_mut(&view.frame_id) {
                    frame.generation = frame.generation.wrapping_add(1);
                    frame.views.retain(|v| v.view_id != vid);
                    if frame.selected_view.map_or(false, |v| v.view_id == vid) {
                        frame.selected_view = frame.views.first().copied();
                    }
                    if frame.views.is_empty() {
                        self.frames.remove(&view.frame_id);
                    }
                }
            }
        }

        dead_caps
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn dummy_req(frame_id: u32, frame_gen: u32) -> CreateViewRequest {
        CreateViewRequest {
            opcode: api::display::compositor_ops::CREATE_VIEW,
            role: SurfaceRole::Interactive,
            _pad: [0; 2],
            frame_id,
            frame_generation: frame_gen,
            w: 800,
            h: 600,
            min_w: 400,
            min_h: 300,
            recipient_incarnation: 1,
            file_grant_id: 0,
            request_generation: 1,
            _reserved: 0,
            launch_token: [0; 16],
        }
    }

    #[test]
    fn test_group_view_same_frame_does_not_panic() {
        let mut reg = FrameRegistry::new();
        let tid_a = 100;
        let r1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
        // Call group_view with target_frame_id == source_frame_id
        let res = reg.group_view(r1.frame_id, r1.frame_generation, r1.frame_id, r1.frame_generation, r1.view_id, r1.view_incarnation, tid_a);
        assert!(res.is_ok(), "same-frame group_view must succeed or no-op safely without panicking");
    }

    #[test]
    fn test_multi_view_creation_and_frame_association() {
        let mut reg = FrameRegistry::new();
        let tid_a = 100;
        let tid_b = 200;

        // Process A creates initial view in a new frame
        let req1 = dummy_req(0, 0);
        let resp1 = reg.create_view(&req1, tid_a, 10).expect("create view 1");
        assert_eq!(resp1.result, ViewOpResult::Success);
        assert_eq!(resp1.frame_id, 1);
        assert_eq!(resp1.frame_generation, 1);
        assert_eq!(resp1.view_id, 1);

        // Process A creates a second view in the same frame
        let req2 = dummy_req(1, 1);
        let resp2 = reg.create_view(&req2, tid_a, 11).expect("create view 2");
        assert_eq!(resp2.frame_id, 1);
        assert_eq!(resp2.view_id, 2);

        // Process B (mixed owner!) creates a third view in the same frame
        let req3 = dummy_req(resp2.frame_id, resp2.frame_generation);
        let resp3 = reg.create_view(&req3, tid_b, 12).expect("create view 3");
        assert_eq!(resp3.frame_id, 1);
        assert_eq!(resp3.view_id, 3);

        let frame = reg.frames.get(&1).expect("frame 1 exists");
        assert_eq!(frame.views.len(), 3);
        assert_eq!(frame.views[0].view_id, 1);
        assert_eq!(frame.views[1].view_id, 2);
        assert_eq!(frame.views[2].view_id, 3);
        assert_eq!(frame.selected_view.map(|v| v.view_id), Some(1));
    }

    #[test]
    fn test_select_view_and_stale_generation_rejection() {
        let mut reg = FrameRegistry::new();
        let tid_a = 100;

        let resp1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
        let resp2 = reg.create_view(&dummy_req(resp1.frame_id, resp1.frame_generation), tid_a, 11).unwrap();

        // Select view 2
        let sel = reg.select_view(resp2.frame_id, resp2.frame_generation, resp2.view_id, resp2.view_incarnation).unwrap();
        assert_eq!(sel.result, ViewOpResult::Success);
        assert_eq!(reg.selected_view_for_frame(resp2.frame_id).map(|v| v.view_id), Some(resp2.view_id));

        // Stale frame generation rejected
        let err1 = reg.select_view(resp2.frame_id, 999, resp2.view_id, resp2.view_incarnation);
        assert_eq!(err1, Err(ViewOpResult::StaleGeneration));

        // Stale view incarnation rejected
        let err2 = reg.select_view(resp2.frame_id, resp2.frame_generation, resp2.view_id, 999);
        assert_eq!(err2, Err(ViewOpResult::StaleGeneration));

        // Nonexistent view rejected
        let err3 = reg.select_view(resp2.frame_id, resp2.frame_generation, 999, 1);
        assert_eq!(err3, Err(ViewOpResult::ViewNotFound));
    }

    #[test]
    fn test_group_and_detach_view() {
        let mut reg = FrameRegistry::new();
        let tid_a = 100;
        let tid_b = 200;

        let r1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
        let r2 = reg.create_view(&dummy_req(r1.frame_id, r1.frame_generation), tid_a, 11).unwrap();
        let r3 = reg.create_view(&dummy_req(r2.frame_id, r2.frame_generation), tid_b, 12).unwrap();

        // Detach view 3 (owned by tid_b) into new floating frame at (200, 200)
        let detach = reg.detach_view(r3.frame_id, r3.frame_generation, r3.view_id, r3.view_incarnation, 200, 200, tid_b).unwrap();
        assert_eq!(detach.result, ViewOpResult::Success);
        let new_frame_id = detach.frame_id;
        assert_ne!(new_frame_id, r1.frame_id);
        assert_eq!(reg.frame_for_view(r3.view_id), Some(new_frame_id));
        assert_eq!(reg.frames.get(&r1.frame_id).unwrap().views.len(), 2);
        assert_eq!(reg.frames.get(&r1.frame_id).unwrap().generation, r3.frame_generation + 1);
        // Forged caller cannot group view 3
        let cur_r1_gen = reg.frames.get(&r1.frame_id).unwrap().generation;
        let forged = reg.group_view(r1.frame_id, cur_r1_gen, new_frame_id, detach.frame_generation, r3.view_id, r3.view_incarnation, tid_a);
        assert_eq!(forged, Err(ViewOpResult::AccessDenied));

        // Authorized owner groups view 3 back into frame 1
        let group = reg.group_view(r1.frame_id, cur_r1_gen, new_frame_id, detach.frame_generation, r3.view_id, r3.view_incarnation, tid_b).unwrap();
        assert_eq!(group.result, ViewOpResult::Success);
        assert_eq!(reg.frame_for_view(r3.view_id), Some(r1.frame_id));
        assert_eq!(reg.frames.get(&r1.frame_id).unwrap().views.len(), 3);
        // Empty source frame was destroyed
        assert!(!reg.frames.contains_key(&new_frame_id));
    }

    #[test]
    fn test_owner_crash_cleanup_and_focus_preservation() {
        let mut reg = FrameRegistry::new();
        let tid_a = 100;
        let tid_b = 200;

        let r1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
        let r2 = reg.create_view(&dummy_req(r1.frame_id, r1.frame_generation), tid_a, 11).unwrap();
        let r3 = reg.create_view(&dummy_req(r2.frame_id, r2.frame_generation), tid_b, 12).unwrap();

        // Select view 3
        reg.select_view(r3.frame_id, r3.frame_generation, r3.view_id, r3.view_incarnation).unwrap();
        assert_eq!(reg.selected_view_for_frame(r1.frame_id).map(|v| v.view_id), Some(r3.view_id));

        // Process B crashes
        let dead_caps = reg.cleanup_owner(tid_b);
        assert_eq!(dead_caps, alloc::vec![12]);
        assert_eq!(reg.views.len(), 2);
        assert_eq!(reg.frames.get(&r1.frame_id).unwrap().views.len(), 2);
        // Focus cleanly reverted to surviving view
        assert_eq!(reg.selected_view_for_frame(r1.frame_id).map(|v| v.view_id), Some(r1.view_id));

        // Destroy view 1
        let cur_gen1 = reg.frames.get(&r1.frame_id).unwrap().generation;
        let (d1, cap1) = reg.destroy_view(r1.frame_id, cur_gen1, r1.view_id, r1.view_incarnation, tid_a).unwrap();
        assert_eq!(cap1, 10);
        assert_eq!(d1.result, ViewOpResult::Success);
        assert_eq!(reg.selected_view_for_frame(r1.frame_id).map(|v| v.view_id), Some(r2.view_id));

        // Destroy view 2 -> frame destroyed
        let cur_gen2 = reg.frames.get(&r1.frame_id).unwrap().generation;
        let (d2, cap2) = reg.destroy_view(r1.frame_id, cur_gen2, r2.view_id, r2.view_incarnation, tid_a).unwrap();
        assert_eq!(cap2, 11);
        assert_eq!(d2.result, ViewOpResult::Success);
        assert!(reg.frames.is_empty());
        assert!(reg.views.is_empty());
    }
}
