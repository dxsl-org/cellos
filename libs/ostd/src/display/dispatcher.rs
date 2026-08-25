//! Bounded storage for compositor lifecycle and forwarded input events.

use api::display::{compositor_events, WindowCloseRequest, WindowConfigure, WindowStateChanged};

use super::events::{SurfaceEvent, MAX_SURFACE_EVENT_CAPS};

const MAX_FORWARDED_INPUT_EVENTS: usize = 32;

#[derive(Clone, Copy)]
struct Stamped<T: Copy> {
    sequence: u64,
    value: T,
}

#[derive(Clone, Copy)]
struct SurfaceEventSlot {
    cap: u32,
    used: bool,
    configure: Option<Stamped<WindowConfigure>>,
    close: Option<Stamped<WindowCloseRequest>>,
    state: Option<Stamped<WindowStateChanged>>,
}

impl SurfaceEventSlot {
    const EMPTY: Self = Self {
        cap: 0,
        used: false,
        configure: None,
        close: None,
        state: None,
    };
}

struct SurfaceEventDispatcher {
    next_sequence: u64,
    slots: [SurfaceEventSlot; MAX_SURFACE_EVENT_CAPS],
    forwarded_input: heapless::Vec<api::input::InputEvent, MAX_FORWARDED_INPUT_EVENTS>,
}

impl SurfaceEventDispatcher {
    const fn new() -> Self {
        Self {
            next_sequence: 1,
            slots: [SurfaceEventSlot::EMPTY; MAX_SURFACE_EVENT_CAPS],
            forwarded_input: heapless::Vec::new(),
        }
    }

    fn insert(&mut self, event: SurfaceEvent) {
        let cap = match event {
            SurfaceEvent::Configure(frame) => frame.cap,
            SurfaceEvent::CloseRequest(frame) => frame.cap,
            SurfaceEvent::StateChanged(frame) => frame.cap,
        };
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        let slot = match self
            .slots
            .iter()
            .position(|slot| slot.used && slot.cap == cap)
        {
            Some(index) => &mut self.slots[index],
            None => {
                let index = match self.slots.iter().position(|slot| !slot.used) {
                    Some(index) => index,
                    None => return,
                };
                self.slots[index] = SurfaceEventSlot {
                    cap,
                    used: true,
                    ..SurfaceEventSlot::EMPTY
                };
                &mut self.slots[index]
            }
        };
        match event {
            SurfaceEvent::Configure(frame) => {
                slot.configure = Some(Stamped {
                    sequence,
                    value: frame,
                })
            }
            SurfaceEvent::CloseRequest(frame) => {
                slot.close = Some(Stamped {
                    sequence,
                    value: frame,
                })
            }
            SurfaceEvent::StateChanged(frame) => {
                slot.state = Some(Stamped {
                    sequence,
                    value: frame,
                })
            }
        }
    }

    fn take_surface(&mut self) -> Option<SurfaceEvent> {
        let mut selected = None;
        for (index, slot) in self.slots.iter().enumerate() {
            for (kind, sequence) in [
                (0, slot.configure.map(|event| event.sequence)),
                (1, slot.close.map(|event| event.sequence)),
                (2, slot.state.map(|event| event.sequence)),
            ] {
                if let Some(sequence) = sequence {
                    let replace = match selected {
                        Some((_, _, oldest)) => sequence < oldest,
                        None => true,
                    };
                    if replace {
                        selected = Some((index, kind, sequence));
                    }
                }
            }
        }
        let (index, kind, _) = selected?;
        let slot = &mut self.slots[index];
        let event = match kind {
            0 => slot
                .configure
                .take()
                .map(|event| SurfaceEvent::Configure(event.value)),
            1 => slot
                .close
                .take()
                .map(|event| SurfaceEvent::CloseRequest(event.value)),
            _ => slot
                .state
                .take()
                .map(|event| SurfaceEvent::StateChanged(event.value)),
        };
        if slot.configure.is_none() && slot.close.is_none() && slot.state.is_none() {
            *slot = SurfaceEventSlot::EMPTY;
        }
        event
    }
}

static SURFACE_EVENTS: crate::sync::Mutex<SurfaceEventDispatcher> =
    crate::sync::Mutex::new(SurfaceEventDispatcher::new());

/// Route one trusted compositor frame without allocating.
pub(crate) fn route_compositor_frame(frame: &[u8]) {
    let Some(opcode) = frame.first().copied() else {
        return;
    };
    let mut dispatcher = SURFACE_EVENTS.lock();
    match opcode {
        api::input::INPUT_EVENT_OPCODE => {
            if let Some(event) = crate::input::parse_frame(frame) {
                let _ = dispatcher.forwarded_input.push(event);
            }
        }
        compositor_events::WINDOW_CONFIGURE => {
            if let Ok(event) = WindowConfigure::decode(frame.get(..28).unwrap_or_default()) {
                dispatcher.insert(SurfaceEvent::Configure(event));
            }
        }
        compositor_events::WINDOW_CLOSE_REQUEST => {
            if let Ok(event) = WindowCloseRequest::decode(frame.get(..12).unwrap_or_default()) {
                dispatcher.insert(SurfaceEvent::CloseRequest(event));
            }
        }
        compositor_events::WINDOW_STATE_CHANGED => {
            if let Ok(event) = WindowStateChanged::decode(frame.get(..12).unwrap_or_default()) {
                dispatcher.insert(SurfaceEvent::StateChanged(event));
            }
        }
        _ => {}
    }
}

pub(crate) fn take_forwarded_input_event() -> Option<api::input::InputEvent> {
    let mut dispatcher = SURFACE_EVENTS.lock();
    if dispatcher.forwarded_input.is_empty() {
        None
    } else {
        Some(dispatcher.forwarded_input.remove(0))
    }
}

pub(crate) fn take_surface() -> Option<SurfaceEvent> {
    SURFACE_EVENTS.lock().take_surface()
}
