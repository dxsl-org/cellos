use api::display::{compositor_ops, Rect, WindowState};
use ostd::syscall::{sys_try_send, SyscallResult};

use crate::surface_table::{StateTransition, SurfaceTable};
use crate::window_decoration::{self, Control};

pub(super) fn apply<F>(
    cap: u64,
    control: Control,
    maximum: Rect,
    table: &mut SurfaceTable,
    damage: &mut F,
) -> bool
where
    F: FnMut(Rect),
{
    let Some(surface) = table.get_mut(cap) else {
        return false;
    };
    let old = window_decoration::bounds(surface.screen_rect());
    if control == Control::Close {
        if let Some((rect, close, state)) = surface.begin_close(cap as u32) {
            let delivered = close.encode().is_ok_and(|event| {
                matches!(sys_try_send(surface.owner, &event), SyscallResult::Ok(0))
            });
            if !delivered {
                surface.cancel_close(close.serial);
                return false;
            }
            if let Ok(event) = state.encode() {
                let _ = sys_try_send(surface.owner, &event);
            }
            damage(window_decoration::bounds(rect));
            return true;
        }
        return false;
    }
    let opcode = match control {
        Control::Minimize => compositor_ops::MINIMIZE,
        Control::Maximize if surface.state() == WindowState::Maximized => compositor_ops::RESTORE,
        Control::Maximize => compositor_ops::MAXIMIZE,
        Control::Close => return false,
    };
    let prior_state = surface.state();
    if let Some(transition) = surface.request_state(cap as u32, opcode, maximum) {
        match transition {
            StateTransition::Configure(configure) => {
                let delivered = configure.encode().is_ok_and(|event| {
                    matches!(sys_try_send(surface.owner, &event), SyscallResult::Ok(0))
                });
                if !delivered {
                    surface.cancel_configure(configure.serial);
                }
            }
            StateTransition::StateChanged(state) => {
                let delivered = state.encode().is_ok_and(|event| {
                    matches!(sys_try_send(surface.owner, &event), SyscallResult::Ok(0))
                });
                if !delivered {
                    surface.rollback_state_change(prior_state);
                    return false;
                }
                damage(old.union(&window_decoration::bounds(surface.screen_rect())));
                return !surface.accepts_input();
            }
        }
    }
    false
}
