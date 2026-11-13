//! IPC frame placement: snap zones and fullscreen.

use wire::display::{FramePlacement, Rect, SnapZone, ViewOpResponse, ViewOpResult};

use super::FrameRegistry;

impl FrameRegistry {
    pub fn snap_frame(
        &mut self,
        frame_id: u32,
        frame_gen: u32,
        zone: SnapZone,
        work_area: Rect,
        caller_tid: usize,
    ) -> Result<(ViewOpResponse, Rect), ViewOpResult> {
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

        let target_rect = calculate_snap_rect(zone, work_area);
        if target_rect.w < frame.min_w || target_rect.h < frame.min_h {
            return Err(ViewOpResult::Refused);
        }

        if frame.placement == FramePlacement::Floating {
            frame.saved_floating_rect = Some(Rect {
                x: frame.x,
                y: frame.y,
                w: frame.w,
                h: frame.h,
            });
        }

        frame.x = target_rect.x;
        frame.y = target_rect.y;
        frame.w = target_rect.w;
        frame.h = target_rect.h;
        frame.placement = FramePlacement::Snapped(zone);
        frame.generation = frame.generation.wrapping_add(1);

        Ok((
            ViewOpResponse {
                opcode: wire::display::compositor_ops::SNAP_FRAME,
                result: ViewOpResult::Success,
                _pad: [0; 2],
                frame_id,
                frame_generation: frame.generation,
                view_id: frame.selected_view.map_or(0, |v| v.view_id),
                view_incarnation: frame.selected_view.map_or(0, |v| v.incarnation),
            },
            target_rect,
        ))
    }

    pub fn toggle_fullscreen(
        &mut self,
        frame_id: u32,
        frame_gen: u32,
        enable: bool,
        screen_w: u32,
        screen_h: u32,
        caller_tid: usize,
    ) -> Result<(ViewOpResponse, Rect), ViewOpResult> {
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

        let result_rect = if enable {
            if frame.placement != FramePlacement::Fullscreen {
                frame.saved_placement = Some(frame.placement);
                frame.saved_floating_rect = Some(Rect {
                    x: frame.x,
                    y: frame.y,
                    w: frame.w,
                    h: frame.h,
                });
            }
            frame.y = 0;
            frame.w = screen_w;
            frame.h = screen_h;
            frame.placement = FramePlacement::Fullscreen;
            frame.generation = frame.generation.wrapping_add(1);
            Rect {
                x: 0,
                y: 0,
                w: screen_w,
                h: screen_h,
            }
        } else {
            let restored = frame.saved_floating_rect.take().unwrap_or(Rect {
                x: 100,
                y: 100,
                w: 800.min(screen_w),
                h: 600.min(screen_h),
            });
            frame.x = restored.x;
            frame.y = restored.y;
            frame.w = restored.w;
            frame.h = restored.h;
            frame.placement = frame
                .saved_placement
                .take()
                .unwrap_or(FramePlacement::Floating);
            restored
        };

        Ok((
            ViewOpResponse {
                opcode: wire::display::compositor_ops::TOGGLE_FULLSCREEN,
                result: ViewOpResult::Success,
                _pad: [0; 2],
                frame_id,
                frame_generation: frame.generation,
                view_id: frame.selected_view.map_or(0, |v| v.view_id),
                view_incarnation: frame.selected_view.map_or(0, |v| v.incarnation),
            },
            result_rect,
        ))
    }
}

pub fn calculate_snap_rect(zone: SnapZone, work_area: Rect) -> Rect {
    let half_w = work_area.w / 2;
    let other_half_w = work_area.w - half_w;
    let half_h = work_area.h / 2;
    let other_half_h = work_area.h - half_h;

    match zone {
        SnapZone::LeftHalf => Rect {
            x: work_area.x,
            y: work_area.y,
            w: half_w,
            h: work_area.h,
        },
        SnapZone::RightHalf => Rect {
            x: work_area.x + half_w as i32,
            y: work_area.y,
            w: other_half_w,
            h: work_area.h,
        },
        SnapZone::TopLeftQuarter => Rect {
            x: work_area.x,
            y: work_area.y,
            w: half_w,
            h: half_h,
        },
        SnapZone::BottomLeftQuarter => Rect {
            x: work_area.x,
            y: work_area.y + half_h as i32,
            w: half_w,
            h: other_half_h,
        },
        SnapZone::TopRightQuarter => Rect {
            x: work_area.x + half_w as i32,
            y: work_area.y,
            w: other_half_w,
            h: half_h,
        },
        SnapZone::BottomRightQuarter => Rect {
            x: work_area.x + half_w as i32,
            y: work_area.y + half_h as i32,
            w: other_half_w,
            h: other_half_h,
        },
    }
}
