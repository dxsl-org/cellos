//! The Chrome window: guest start, event loop, status text.

use api::display::{PixelFormat, Rect};
use api::services::vmm_session::VmmEvent;
use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::theme;
use viui::window_lifecycle::{ClosePolicy, WindowEvent, WindowLifecycle};

use crate::frame::GuestFrame;
use crate::guest::Guest;
use crate::input_bridge::InputBridge;

const DEFAULT_WIDTH: u32 = 1024;
const DEFAULT_HEIGHT: u32 = 680;

/// What the window shows when no guest frame is on screen.
fn paint_status(surface: &mut ViSurface, fonts: &mut TextFonts, text: &str) {
    let palette = theme::system();
    let (width, height, stride) = (surface.width(), surface.height(), surface.stride());
    let bg = palette.bg();
    let fill = [bg.b(), bg.g(), bg.r(), bg.a()];
    for pixel in surface.pixels_mut().chunks_exact_mut(4) {
        pixel.copy_from_slice(&fill);
    }
    let fg = palette.text_secondary();
    fonts.draw_text(
        surface.pixels_mut(),
        width,
        height,
        stride,
        24.0,
        (height / 2) as f32 - 8.0,
        text,
        FontFace::UiRegular,
        14.0,
        [fg.b(), fg.g(), fg.r(), fg.a()],
        (0, 0, width as i32, height as i32),
    );
    surface.damage_all();
}

pub fn cell_main() {
    #[cfg(target_os = "none")]
    crate::init_custom_heap();

    let comp_tid = ostd::display::wait_for_compositor();
    let (screen_w, screen_h) = ostd::syscall::sys_get_resolution();
    let width = DEFAULT_WIDTH.min(screen_w.max(320));
    let height = DEFAULT_HEIGHT.min(screen_h.saturating_sub(80).max(240));
    let Ok(mut surface) = ViSurface::create(comp_tid, width, height, PixelFormat::Bgra8888) else {
        ostd::io::println("[chrome] cannot create the window; exiting");
        ostd::syscall::sys_exit(1);
    };
    let _ = surface.set_title("Chrome");
    let _ = surface.set_icon(ostd::display::linear::icons::LineIcon::Globe);
    surface.move_to(
        (screen_w.saturating_sub(width) / 2) as i32,
        (screen_h.saturating_sub(height) / 2) as i32,
    );
    let mut fonts = TextFonts::new();
    paint_status(&mut surface, &mut fonts, "Starting Chromium in a Linux guest...");

    let guest = match Guest::start(width, height) {
        Ok(guest) => {
            ostd::io::println("[chrome] guest session bound");
            Some(guest)
        }
        Err(reason) => {
            ostd::io::println(&alloc::format!("[chrome] guest not started: {reason}"));
            paint_status(&mut surface, &mut fonts, reason);
            None
        }
    };
    while !ostd::input::request_focus() {
        ostd::syscall::sys_yield();
    }

    let mut window = WindowLifecycle::new(ClosePolicy::Accept);
    let mut frame: Option<GuestFrame> = None;
    let mut bridge = InputBridge::new();
    loop {
        let mut closed = false;
        let mut resized = false;
        window.poll(&mut surface, |event| match event {
            WindowEvent::Closed => closed = true,
            WindowEvent::Resized { .. } | WindowEvent::Restored => resized = true,
            WindowEvent::Minimized | WindowEvent::CloseRequested { .. } => {}
        });
        if closed {
            if let Some(guest) = &guest {
                guest.terminate();
            }
            break;
        }
        let Some(guest) = &guest else {
            if resized {
                paint_status(&mut surface, &mut fonts, "The Linux guest could not be started.");
            }
            let _ = ostd::syscall::sys_set_timer(1);
            continue;
        };
        if resized {
            // The old frame no longer matches; the guest re-announces at the new size.
            frame = None;
            paint_status(&mut surface, &mut fonts, "Resizing...");
            guest.resize(surface.width(), surface.height());
        }

        let mut busy = false;
        for event in ostd::input::poll_events(32) {
            busy = true;
            if let ostd::input::InputEvent::MouseButton { .. } = event {
                let _ = ostd::input::request_focus();
            }
            for (event_type, code, value) in
                bridge.translate(&event, surface.width(), surface.height())
            {
                guest.forward(event_type, code, value);
            }
        }

        while let Some(event) = guest.next_event() {
            busy = true;
            match event {
                VmmEvent::Scanout { grant, width, height } => {
                    frame = Some(GuestFrame::new(grant, guest.hypervisor_tid(), width, height));
                }
                VmmEvent::Damage { x, y, width, height } => {
                    let Some(frame) = &frame else { continue };
                    let rect = Rect { x: x as i32, y: y as i32, w: width, h: height };
                    let (w, h, stride) = (surface.width(), surface.height(), surface.stride());
                    if let Some(copied) = frame.copy_to(rect, surface.pixels_mut(), stride, w, h) {
                        surface.damage(copied);
                    }
                }
                VmmEvent::Exited { .. } => {
                    frame = None;
                    paint_status(&mut surface, &mut fonts, "The Linux guest has stopped.");
                }
            }
        }
        if busy {
            ostd::syscall::sys_yield();
        } else {
            let _ = ostd::syscall::sys_set_timer(1);
        }
    }
    ostd::syscall::sys_exit(0);
}
