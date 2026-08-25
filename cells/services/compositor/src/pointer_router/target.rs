use api::input::{encode_event, InputEvent, INPUT_EVENT_IPC_SIZE, INPUT_EVENT_OPCODE};
use ostd::syscall::sys_try_send;

use crate::surface_table::SurfaceTable;
use crate::window_decoration::{self, Hit};
use crate::z_order::ZOrder;

#[derive(Clone, Copy)]
pub(super) struct PointerTarget {
    pub(super) cap: u64,
    pub(super) owner: usize,
    origin_x: i32,
    origin_y: i32,
    pub(super) hit: Hit,
}

pub(super) fn at(x: i32, y: i32, table: &SurfaceTable, z_order: &ZOrder) -> Option<PointerTarget> {
    z_order.iter_top_to_bottom().find_map(|cap| {
        let surface = table.get(cap).filter(|surface| surface.accepts_input())?;
        window_decoration::hit_test(surface.screen_rect(), x, y).map(|hit| PointerTarget {
            cap,
            owner: surface.owner,
            origin_x: surface.x,
            origin_y: surface.y,
            hit,
        })
    })
}

pub(super) fn for_cap(cap: u64, table: &SurfaceTable) -> Option<PointerTarget> {
    let surface = table.get(cap).filter(|surface| surface.accepts_input())?;
    Some(PointerTarget {
        cap,
        owner: surface.owner,
        origin_x: surface.x,
        origin_y: surface.y,
        hit: Hit::Content,
    })
}

pub(super) fn send_position(target: PointerTarget, x: i32, y: i32) {
    send_local(target, InputEvent::MouseMove { x, y, dx: 0, dy: 0 });
}

pub(super) fn send_local(target: PointerTarget, event: InputEvent) {
    let event = match event {
        InputEvent::MouseMove { x, y, dx, dy } => InputEvent::MouseMove {
            x: x.saturating_sub(target.origin_x),
            y: y.saturating_sub(target.origin_y),
            dx,
            dy,
        },
        event => event,
    };
    let mut payload = [0u8; INPUT_EVENT_IPC_SIZE];
    encode_event(&event, &mut payload);
    let mut frame = [0u8; 1 + INPUT_EVENT_IPC_SIZE];
    frame[0] = INPUT_EVENT_OPCODE;
    frame[1..].copy_from_slice(&payload);
    let _ = sys_try_send(target.owner, &frame);
}
