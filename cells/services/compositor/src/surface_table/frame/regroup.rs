//! IPC regrouping: move a view into another frame or out into its own.

use api::display::{FramePlacement, FrameRef, ViewOpResponse, ViewOpResult, ViewRef};

use super::{Frame, FrameRegistry, MAX_FRAMES, MAX_VIEWS_PER_FRAME};

impl FrameRegistry {
    /// Move the caller's view into the caller's own frame `target`, shown there.
    pub fn group_view(
        &mut self,
        target: FrameRef,
        source: FrameRef,
        view: ViewRef,
        caller_tid: usize,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        let view_ref = view;
        let entry = self
            .views
            .get(&view.view_id)
            .ok_or(ViewOpResult::ViewNotFound)?;
        if entry.incarnation != view.incarnation {
            return Err(ViewOpResult::StaleGeneration);
        }
        if entry.owner_tid != caller_tid {
            return Err(ViewOpResult::AccessDenied);
        }
        if entry.frame_id != source.frame_id {
            return Err(ViewOpResult::FrameNotFound);
        }
        let dst = self
            .frames
            .get(&target.frame_id)
            .ok_or(ViewOpResult::FrameNotFound)?;
        if target.frame_id == source.frame_id {
            return Ok(response(
                api::display::compositor_ops::GROUP_VIEW,
                target.frame_id,
                dst.generation,
                view_ref,
            ));
        }
        let src = self
            .frames
            .get(&source.frame_id)
            .ok_or(ViewOpResult::FrameNotFound)?;
        if src.generation != source.generation {
            return Err(ViewOpResult::StaleGeneration);
        }
        if dst.owner_tid != Some(caller_tid) {
            return Err(ViewOpResult::AccessDenied);
        }
        if dst.generation != target.generation {
            return Err(ViewOpResult::StaleGeneration);
        }
        if dst.views.len() >= MAX_VIEWS_PER_FRAME {
            return Err(ViewOpResult::CapacityExhausted);
        }

        self.unlink(view.view_id, source.frame_id);
        if let Some(entry) = self.views.get_mut(&view.view_id) {
            entry.frame_id = target.frame_id;
        }
        let dst_gen = {
            let dst = self
                .frames
                .get_mut(&target.frame_id)
                .ok_or(ViewOpResult::FrameNotFound)?;
            dst.generation = dst.generation.wrapping_add(1);
            dst.views.push(view_ref);
            dst.selected_view = Some(view_ref);
            dst.generation
        };
        self.refresh(target.frame_id);
        Ok(response(
            api::display::compositor_ops::GROUP_VIEW,
            target.frame_id,
            dst_gen,
            view_ref,
        ))
    }

    /// Move the caller's view into a new floating frame of its own at `(x, y)`.
    pub fn detach_view(
        &mut self,
        source: FrameRef,
        view: ViewRef,
        x: i32,
        y: i32,
        caller_tid: usize,
    ) -> Result<ViewOpResponse, ViewOpResult> {
        let entry = self
            .views
            .get(&view.view_id)
            .ok_or(ViewOpResult::ViewNotFound)?;
        if entry.incarnation != view.incarnation {
            return Err(ViewOpResult::StaleGeneration);
        }
        if entry.owner_tid != caller_tid {
            return Err(ViewOpResult::AccessDenied);
        }
        if entry.frame_id != source.frame_id {
            return Err(ViewOpResult::FrameNotFound);
        }
        if self.frames.len() >= MAX_FRAMES {
            return Err(ViewOpResult::CapacityExhausted);
        }
        let src = self
            .frames
            .get(&source.frame_id)
            .ok_or(ViewOpResult::FrameNotFound)?;
        if src.generation != source.generation {
            return Err(ViewOpResult::StaleGeneration);
        }
        let (w, h) = (src.w, src.h);
        self.unlink(view.view_id, source.frame_id);
        let frame_id = self.insert_single_frame(view, caller_tid, (x, y, w, h));
        Ok(response(
            api::display::compositor_ops::DETACH_VIEW,
            frame_id,
            1,
            view,
        ))
    }

    /// Register a new floating frame showing only `view`, owned by `owner`.
    pub(super) fn insert_single_frame(
        &mut self,
        view: ViewRef,
        owner: usize,
        (x, y, w, h): (i32, i32, u32, u32),
    ) -> u32 {
        let frame_id = self.allocate_frame_id();
        self.frames.insert(
            frame_id,
            Frame {
                generation: 1,
                x,
                y,
                w,
                h,
                views: alloc::vec![view],
                selected_view: Some(view),
                min_w: 0,
                min_h: 0,
                placement: FramePlacement::Floating,
                saved_floating_rect: None,
                saved_placement: None,
                owner_tid: Some(owner),
            },
        );
        if let Some(entry) = self.views.get_mut(&view.view_id) {
            entry.frame_id = frame_id;
        }
        self.refresh(frame_id);
        frame_id
    }
}

fn response(opcode: u8, frame_id: u32, frame_generation: u32, view: ViewRef) -> ViewOpResponse {
    ViewOpResponse {
        opcode,
        result: ViewOpResult::Success,
        _pad: [0; 2],
        frame_id,
        frame_generation,
        view_id: view.view_id,
        view_incarnation: view.incarnation,
    }
}
