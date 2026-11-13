//! IPC view lifecycle: create, destroy and select a tab view.

use wire::display::{CreateViewRequest, FramePlacement, ViewOpResponse, ViewOpResult, ViewRef};

use super::{Frame, FrameRegistry, Reselected, View, MAX_FRAMES, MAX_VIEWS_PER_FRAME};

impl FrameRegistry {
    /// Allocate a new view and optionally attach it to an existing or new frame.
    ///
    /// Only the frame's sole owner may add a view to it; a frame shared by
    /// several apps refuses everyone. A minimum size larger than the requested
    /// size is refused, so no frame carries an unsatisfiable minimum.
    pub fn create_view(
        &mut self,
        req: &CreateViewRequest,
        caller_tid: usize,
        surface_cap: u64,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        if req.min_w > req.w || req.min_h > req.h {
            return Err(ViewOpResult::Refused);
        }
        let target_frame_id = if req.frame_id == 0 {
            if self.frames.len() >= MAX_FRAMES {
                return Err(ViewOpResult::CapacityExhausted);
            }
            None
        } else {
            let frame = self
                .frames
                .get(&req.frame_id)
                .ok_or(ViewOpResult::FrameNotFound)?;
            if frame.owner_tid != Some(caller_tid) {
                return Err(ViewOpResult::AccessDenied);
            }
            if frame.generation != req.frame_generation {
                return Err(ViewOpResult::StaleGeneration);
            }
            if frame.views.len() >= MAX_VIEWS_PER_FRAME {
                return Err(ViewOpResult::CapacityExhausted);
            }
            Some(req.frame_id)
        };

        let view_id = self.next_view_id;
        self.next_view_id = self.next_view_id.wrapping_add(1).max(1);
        let view_ref = ViewRef {
            view_id,
            incarnation: 1,
        };
        let frame_id = match target_frame_id {
            Some(frame_id) => {
                if let Some(frame) = self.frames.get_mut(&frame_id) {
                    frame.generation = frame.generation.wrapping_add(1);
                    frame.views.push(view_ref);
                }
                frame_id
            }
            None => {
                let frame_id = self.allocate_frame_id();
                self.frames.insert(
                    frame_id,
                    Frame {
                        generation: 1,
                        x: 100,
                        y: 100,
                        w: req.w,
                        h: req.h,
                        views: alloc::vec![view_ref],
                        selected_view: Some(view_ref),
                        min_w: req.min_w,
                        min_h: req.min_h,
                        placement: FramePlacement::Floating,
                        saved_floating_rect: None,
                        saved_placement: None,
                        owner_tid: Some(caller_tid),
                    },
                );
                frame_id
            }
        };
        self.views.insert(
            view_id,
            View {
                incarnation: 1,
                frame_id,
                surface_cap,
                owner_tid: caller_tid,
                min_w: req.min_w,
                min_h: req.min_h,
            },
        );
        self.by_cap.insert(surface_cap, view_id);
        self.refresh(frame_id);
        let frame_generation = self.frames.get(&frame_id).map_or(1, |f| f.generation);

        Ok(ViewOpResponse {
            opcode: wire::display::compositor_ops::CREATE_VIEW,
            result: ViewOpResult::Success,
            _pad: [0; 2],
            frame_id,
            frame_generation,
            view_id,
            view_incarnation: 1,
        })
    }

    /// Destroy the caller's own view; the frame is dropped once empty.
    /// Returns the view's surface and the frame's tab shown afterwards.
    pub fn destroy_view(
        &mut self,
        frame_id: u32,
        frame_gen: u32,
        view_id: u32,
        view_inc: u32,
        caller_tid: usize,
    ) -> Result<(ViewOpResponse, u64, Reselected), ViewOpResult> {
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
        let frame = self
            .frames
            .get(&frame_id)
            .ok_or(ViewOpResult::FrameNotFound)?;
        if frame.generation != frame_gen {
            return Err(ViewOpResult::StaleGeneration);
        }
        let surface_cap = view.surface_cap;
        let reselected = self
            .remove_surface(surface_cap)
            .ok_or(ViewOpResult::ViewNotFound)?;
        let frame_generation = self
            .frames
            .get(&frame_id)
            .map_or(frame_gen, |f| f.generation);

        Ok((
            ViewOpResponse {
                opcode: wire::display::compositor_ops::DESTROY_VIEW,
                result: ViewOpResult::Success,
                _pad: [0; 2],
                frame_id,
                frame_generation,
                view_id,
                view_incarnation: view_inc,
            },
            surface_cap,
            reselected,
        ))
    }

    /// Select the caller's own view as the shown tab of the caller's own frame.
    pub fn select_view(
        &mut self,
        frame_id: u32,
        frame_gen: u32,
        view_id: u32,
        view_inc: u32,
        caller_tid: usize,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        let view = self.views.get(&view_id).ok_or(ViewOpResult::ViewNotFound)?;
        if view.incarnation != view_inc {
            return Err(ViewOpResult::StaleGeneration);
        }
        if view.frame_id != frame_id {
            return Err(ViewOpResult::FrameNotFound);
        }
        if view.owner_tid != caller_tid {
            return Err(ViewOpResult::AccessDenied);
        }

        let frame = self
            .frames
            .get_mut(&frame_id)
            .ok_or(ViewOpResult::FrameNotFound)?;
        if frame.owner_tid != Some(caller_tid) {
            return Err(ViewOpResult::AccessDenied);
        }
        if frame.generation != frame_gen {
            return Err(ViewOpResult::StaleGeneration);
        }

        frame.selected_view = Some(ViewRef {
            view_id,
            incarnation: view_inc,
        });

        Ok(ViewOpResponse {
            opcode: wire::display::compositor_ops::SELECT_VIEW,
            result: ViewOpResult::Success,
            _pad: [0; 2],
            frame_id,
            frame_generation: frame_gen,
            view_id,
            view_incarnation: view_inc,
        })
    }
}
