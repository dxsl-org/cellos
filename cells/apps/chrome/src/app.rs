//! The Chrome window: guest start, event loop, status text.

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::theme;
use viui::window_lifecycle::{ClosePolicy, WindowEvent, WindowLifecycle};
use wire::display::{PixelFormat, Rect};
use wire::vmm_session::VmmEvent;

use crate::clipboard::GuestClipboard;
use crate::frame::GuestFrame;
use crate::guest::{Guest, InputOutbox};
use crate::input_bridge::InputBridge;
use crate::pointer::WindowPointer;

const DEFAULT_WIDTH: u32 = 1024;
const DEFAULT_HEIGHT: u32 = 680;

/// Shown until Chromium's first frame.
const STARTING: &str = "Starting Chromium in a Linux guest...";

/// Fill the window with the system background.
fn clear(surface: &mut ViSurface) {
    let bg = theme::system().bg();
    let fill = [bg.b(), bg.g(), bg.r(), bg.a()];
    for pixel in surface.pixels_mut().as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&fill);
    }
}

/// What the window shows when no guest frame is on screen.
fn paint_status(surface: &mut ViSurface, fonts: &mut TextFonts, text: &str) {
    clear(surface);
    let palette = theme::system();
    let (width, height, stride) = (surface.width(), surface.height(), surface.stride());
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

/// The whole guest frame, clipped to the window, over the window background.
fn paint_frame(surface: &mut ViSurface, frame: &GuestFrame) {
    clear(surface);
    let (width, height, stride) = (surface.width(), surface.height(), surface.stride());
    let _ = frame.copy_to(frame.bounds(), surface.pixels_mut(), stride, width, height);
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
    paint_status(&mut surface, &mut fonts, STARTING);

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
    // The guest's picture is in the window. Not before the guest has drawn: a
    // new scanout is blank, and until Chromium's first frame so is cage's
    // output, which would put an empty guest screen in the window.
    let mut shown = false;
    // Chromium has been on screen, so the guest stopping is Chromium ending.
    let mut started = false;
    let mut bridge = InputBridge::new();
    let mut outbox = InputOutbox::new();
    let mut pointer = WindowPointer::new();
    let mut clipboard = GuestClipboard::new();
    'window: loop {
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
                paint_status(
                    &mut surface,
                    &mut fonts,
                    "The Linux guest could not be started.",
                );
            }
            let _ = ostd::syscall::sys_set_timer(1);
            continue;
        };
        if resized {
            match &frame {
                // The picture stays, clipped to the new size, until the guest
                // redraws at it. A size the guest sees as unchanged (it rounds the
                // width down to 8 pixels; a restore changes nothing) brings no
                // new frame at all.
                Some(frame) if shown => paint_frame(&mut surface, frame),
                _ if started => paint_status(&mut surface, &mut fonts, "Resizing..."),
                _ => paint_status(&mut surface, &mut fonts, STARTING),
            }
            guest.resize(surface.width(), surface.height());
        }

        let mut busy = false;
        for event in ostd::input::poll_events(32) {
            busy = true;
            if let ostd::input::InputEvent::MouseButton { .. } = event {
                let _ = ostd::input::request_focus();
            }
            for guest_event in bridge.translate(&event, surface.width(), surface.height()) {
                outbox.push(guest_event);
            }
        }
        busy |= outbox.flush(guest);
        // Offers arrive with the compositor's frames, routed by the polls above.
        busy |= clipboard.poll(&surface, guest);

        while let Some(event) = guest.next_event() {
            busy = true;
            match event {
                VmmEvent::Scanout {
                    grant,
                    width,
                    height,
                } => {
                    frame = Some(GuestFrame::new(
                        grant,
                        guest.hypervisor_tid(),
                        width,
                        height,
                    ));
                    shown = false;
                }
                VmmEvent::Damage {
                    x,
                    y,
                    width,
                    height,
                } => {
                    let Some(frame) = &frame else { continue };
                    if !shown {
                        if frame.has_content() {
                            shown = true;
                            started = true;
                            paint_frame(&mut surface, frame);
                        }
                        continue;
                    }
                    let rect = Rect {
                        x: x as i32,
                        y: y as i32,
                        w: width,
                        h: height,
                    };
                    let (w, h, stride) = (surface.width(), surface.height(), surface.stride());
                    if let Some(copied) = frame.copy_to(rect, surface.pixels_mut(), stride, w, h) {
                        surface.damage(copied);
                    }
                }
                VmmEvent::Pointer {
                    grant,
                    width,
                    height,
                    hot_x,
                    hot_y,
                } => pointer.show(
                    &surface,
                    comp_tid,
                    guest.hypervisor_tid(),
                    grant,
                    (width, height),
                    (hot_x, hot_y),
                ),
                VmmEvent::PointerHidden => pointer.hide(&surface),
                VmmEvent::Clipboard { grant, len } => {
                    clipboard.guest_copied(&surface, guest, grant, len)
                }
                VmmEvent::Exited { .. } => {
                    if started {
                        // Chromium ran and is gone (quit from inside, or it
                        // crashed): its window goes with it.
                        break 'window;
                    }
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
    surface.destroy();
    ostd::syscall::sys_exit(0);
}
