//! Who receives keystrokes.
//!
//! Two routes, because the two keyboards have different owners:
//!
//! - **Physical keyboard** (virtio-input, USB HID, key repeat): once a
//!   compositor is live it alone decides this route. It registers itself and
//!   forwards each key to the surface the user selected (pointer press), so a
//!   cell that sends `SetFocus` can no longer pull every keystroke of a GUI
//!   session to itself.
//! - **Serial console** (kernel UART relay): foreground console programs
//!   (shell, a REPL launched from it) still claim it with `SetFocus`. The kernel
//!   attests no console principal, so this route stays self-service — the same
//!   residual the SessionV1 serial owner documents.
//!
//! Before any compositor has been seen the image is treated as headless and the
//! physical keyboard follows the console claimant, as it always has. Seeing a
//! live compositor once latches graphical mode for the life of the service, so
//! a compositor restart window does not reopen the physical route.

/// Where `SetFocus` / `ClearFocus` came from, judged against the kernel's live
/// `COMPOSITOR` binding and the message's attested caller identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Requester {
    /// The exact live compositor incarnation.
    Compositor,
    /// Any other cell, while a compositor is registered.
    ClientWithCompositor,
    /// Any cell while no compositor is registered.
    ClientHeadless,
}

#[derive(Default)]
pub struct FocusRoutes {
    keyboard: usize,
    console: usize,
    graphical: bool,
}

impl FocusRoutes {
    pub const fn new() -> Self {
        Self {
            keyboard: 0,
            console: 0,
            graphical: false,
        }
    }

    /// Apply `SetFocus` from `sender`. Returns true when the physical-keyboard
    /// target changed (the caller resets per-target key state).
    pub fn set(&mut self, sender: usize, requester: Requester) -> bool {
        let before = self.keyboard;
        match requester {
            Requester::Compositor => {
                self.graphical = true;
                self.keyboard = sender;
            }
            Requester::ClientWithCompositor => {
                self.graphical = true;
                self.console = sender;
            }
            Requester::ClientHeadless => {
                self.console = sender;
                if !self.graphical {
                    self.keyboard = sender;
                }
            }
        }
        before != self.keyboard
    }

    /// Apply `ClearFocus` from `sender`: a cell can only give up its own routes.
    pub fn clear(&mut self, sender: usize) -> bool {
        if self.console == sender {
            self.console = 0;
        }
        let changed = self.keyboard == sender;
        if changed {
            self.keyboard = 0;
        }
        changed
    }

    /// Target for physical-keyboard events (0 = drop).
    pub const fn keyboard(&self) -> usize {
        self.keyboard
    }

    /// Target for serial-console bytes: the console claimant, else the keyboard
    /// target (in a GUI session that is the compositor, as before).
    pub const fn console(&self) -> usize {
        if self.console != 0 {
            self.console
        } else {
            self.keyboard
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FocusRoutes, Requester};

    const COMPOSITOR: usize = 5;
    const SHELL: usize = 9;
    const ROGUE: usize = 40;

    #[test]
    fn a_client_cannot_take_the_keyboard_from_a_live_compositor() {
        let mut routes = FocusRoutes::new();
        assert!(routes.set(COMPOSITOR, Requester::Compositor));
        assert!(!routes.set(ROGUE, Requester::ClientWithCompositor));
        assert_eq!(routes.keyboard(), COMPOSITOR);
        // Clearing someone else's route does nothing.
        assert!(!routes.clear(ROGUE));
        assert_eq!(routes.keyboard(), COMPOSITOR);
    }

    #[test]
    fn compositor_restart_window_does_not_reopen_the_keyboard() {
        let mut routes = FocusRoutes::new();
        routes.set(COMPOSITOR, Requester::Compositor);
        // The compositor died: no live binding, but the image is graphical.
        assert!(!routes.set(ROGUE, Requester::ClientHeadless));
        assert_eq!(routes.keyboard(), COMPOSITOR);
        assert_eq!(routes.console(), ROGUE);
    }

    #[test]
    fn console_programs_keep_the_serial_route_in_a_gui_session() {
        let mut routes = FocusRoutes::new();
        routes.set(COMPOSITOR, Requester::Compositor);
        assert_eq!(routes.console(), COMPOSITOR);
        routes.set(SHELL, Requester::ClientWithCompositor);
        assert_eq!(routes.console(), SHELL);
        routes.clear(SHELL);
        assert_eq!(routes.console(), COMPOSITOR);
    }

    #[test]
    fn headless_images_keep_self_service_focus() {
        let mut routes = FocusRoutes::new();
        assert!(routes.set(SHELL, Requester::ClientHeadless));
        assert_eq!((routes.keyboard(), routes.console()), (SHELL, SHELL));
        assert!(routes.clear(SHELL));
        assert_eq!((routes.keyboard(), routes.console()), (0, 0));
    }
}
