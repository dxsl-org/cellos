use api::display::{compositor_ops, Rect, WindowState};

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
    if control == Control::Close {
        return crate::stacking::ask_to_close(table, cap)
            .map(damage)
            .is_some();
    }
    let Some(surface) = table.get_mut(cap) else {
        return false;
    };
    let old = window_decoration::bounds(surface.screen_rect());
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
                let delivered = configure
                    .encode()
                    .is_ok_and(|event| crate::stacking::deliver(surface.owner, &event));
                if !delivered {
                    surface.cancel_configure(configure.serial);
                }
            }
            StateTransition::StateChanged(state) => {
                let delivered = state
                    .encode()
                    .is_ok_and(|event| crate::stacking::deliver(surface.owner, &event));
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
