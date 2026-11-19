//! The system palette as last announced by the compositor.
//!
//! The compositor sends `APPEARANCE_CHANGED` to every surface owner when the
//! palette changes and after each window's Grant attach. Whatever path receives
//! a compositor frame (input polling, lifecycle polling, IPC replies) routes it
//! here, so a toolkit only reads [`palette`] and watches [`serial`].

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use wire::display::{AppearanceChanged, AppearancePalette};

static PALETTE: AtomicU8 = AtomicU8::new(AppearancePalette::Light as u8);
static SERIAL: AtomicU32 = AtomicU32::new(0);

/// Palette to draw with: Light until the compositor says otherwise.
pub fn palette() -> AppearancePalette {
    match PALETTE.load(Ordering::Acquire) {
        1 => AppearancePalette::Dark,
        2 => AppearancePalette::Paper,
        _ => AppearancePalette::Light,
    }
}

/// Increments each time [`palette`] changes in this process; repaint when it
/// differs from the value seen at the last paint.
pub fn serial() -> u32 {
    SERIAL.load(Ordering::Acquire)
}

/// Record an `APPEARANCE_CHANGED` frame. Returns whether `frame` was one
/// (well-formed), whether or not it changed the palette. Loops that receive
/// compositor frames themselves (instead of through `ostd::input`) call this.
pub fn note_frame(frame: &[u8]) -> bool {
    let Ok(event) = AppearanceChanged::decode(frame) else {
        return false;
    };
    set(event.palette);
    true
}

fn set(palette: AppearancePalette) {
    if PALETTE.swap(palette as u8, Ordering::AcqRel) != palette as u8 {
        SERIAL.fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_wellformed_events_change_the_palette_and_bump_the_serial() {
        let start = serial();
        assert!(
            !note_frame(&[0xA3, 1, 0, 0, 0, 0, 0, 0]),
            "a CREATE_SURFACE reply is not an event"
        );
        assert_eq!((palette(), serial()), (AppearancePalette::Light, start));
        let dark = AppearanceChanged {
            palette: AppearancePalette::Dark,
            generation: 2,
        }
        .encode();
        assert!(note_frame(&dark));
        assert_eq!((palette(), serial()), (AppearancePalette::Dark, start + 1));
        assert!(note_frame(&dark), "repeat is accepted");
        assert_eq!(serial(), start + 1, "but changes nothing");
        let light = AppearanceChanged {
            palette: AppearancePalette::Light,
            generation: 3,
        }
        .encode();
        assert!(note_frame(&light));
        assert_eq!((palette(), serial()), (AppearancePalette::Light, start + 2));
    }
}
