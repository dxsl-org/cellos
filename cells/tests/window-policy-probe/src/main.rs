//! Graphical two-owner probe for compositor window-policy integration evidence.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

mod events;
mod roles;

use api::display::PixelFormat;
use ostd::display::{wait_for_compositor, ViSurface};
use ostd::io::println;
use ostd::task::yield_now;

use crate::events::EventHandler;
use crate::roles::{parse_role, print_usage};

const SIZE: u32 = 160;

ostd::cell_main!(cell_main);

fn cell_main() {
    let Some(role) = parse_role() else {
        print_usage();
        return;
    };
    let comp = wait_for_compositor();
    let mut surface = match if role.background {
        ViSurface::create_background(comp, SIZE, SIZE, PixelFormat::Bgra8888)
    } else {
        ViSurface::create(comp, SIZE, SIZE, PixelFormat::Bgra8888)
    } {
        Ok(surface) => surface,
        Err(_) => {
            println(&alloc::format!(
                "[window-policy-probe {}] surface create failed",
                role.name
            ));
            return;
        }
    };
    surface.move_to(role.x, role.y);
    for pixel in surface.pixels_mut().chunks_exact_mut(4) {
        pixel.copy_from_slice(&role.color);
    }
    surface.damage_all();
    if !role.background && surface.set_title(role.title).is_ok() {
        println(&alloc::format!(
            "[window-policy-probe {}] title set",
            role.name
        ));
    }
    println(&alloc::format!("[window-policy-probe {}] ready", role.name));

    let mut events = EventHandler::new(&role);
    loop {
        if events.process(&role, &mut surface) {
            println(&alloc::format!(
                "[window-policy-probe {}] destroy",
                role.name
            ));
            surface.destroy();
            return;
        }
        yield_now();
    }
}
