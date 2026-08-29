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
    event::{Event, KeyCode},
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
const SURFACE_X: i32 = 80;
const SURFACE_Y: i32 = 80;

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
    surface.move_to(SURFACE_X, SURFACE_Y);

    // Keep generated state alive so its signal subscriptions serve the live tree.
    let (counter_state, counter_root) = Counter::build();
    let count_for_log = counter_state.count.clone();
    let _count_log = counter_state.count.subscribe(move || {
        let count = *count_for_log.get();
        ostd::io::println(&alloc::format!("[viui-demo] count={count}"));
    });
    let mut ready = false;
    let mut app = ManagedSurfaceApp::new(Box::new(counter_root), surface);
    app.set_close_policy(ClosePolicy::Accept);

    loop {
        let events = collect_input_events(16);
        for event in &events {
            if let Event::KeyPress {
                key: KeyCode::Enter,
                ..
            } = event
            {
                ostd::io::println("[viui-demo] key=Enter");
            }
        }
        match app.tick(&events) {
            ManagedTick::Closed => {
                ostd::io::println("[viui-demo] close request accepted");
                app.shutdown();
                sys_exit(0);
            }
            ManagedTick::Rendered if !ready => {
                ready = true;
                ostd::io::println("[viui-demo] managed surface ready count=0");
            }
            ManagedTick::Rendered | ManagedTick::Idle => {}
        }
        yield_now();
    }
}
