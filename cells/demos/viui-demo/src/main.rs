#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use alloc::boxed::Box;
use api::display::PixelFormat;
use ostd::{
    display::{wait_for_compositor, ViSurface},
    syscall::sys_exit,
    task::yield_now,
};
use viui::{
    input_bridge::collect_input_events,
    managed_surface::{ClosePolicy, ManagedSurfaceApp, ManagedTick},
};

// Counter is generated from counter.vi by viui-build into OUT_DIR.
include!(concat!(env!("OUT_DIR"), "/counter.rs"));

api::declare_syscalls![
    Log,
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantUnregister,
    Send,
    Recv,
    TryRecv,
    LookupService
];

const DISPLAY_W: u32 = 640;
const DISPLAY_H: u32 = 400;

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("[viui-demo] starting managed Counter surface");

    let compositor_tid = wait_for_compositor();
    let surface =
        match ViSurface::create(compositor_tid, DISPLAY_W, DISPLAY_H, PixelFormat::Bgra8888) {
            Ok(surface) => surface,
            Err(_) => {
                ostd::io::println("[viui-demo] ERROR: could not create surface");
                sys_exit(1);
            }
        };
    if surface.set_title("ViUI Counter").is_err() {
        ostd::io::println("[viui-demo] ERROR: could not set surface title");
        sys_exit(1);
    }

    // Keep generated state alive so its signal subscriptions serve the live tree.
    let (_counter_state, counter_root) = Counter::build();
    let mut app = ManagedSurfaceApp::new(Box::new(counter_root), surface);
    app.set_close_policy(ClosePolicy::Accept);

    loop {
        let events = collect_input_events(16);
        if app.tick(&events) == ManagedTick::Closed {
            ostd::io::println("[viui-demo] close request accepted");
            app.shutdown();
            sys_exit(0);
        }
        yield_now();
    }
}
