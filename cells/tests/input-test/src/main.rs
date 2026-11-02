#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

use api::cell_session::Status;
use api::input::{InputEvent, KeyState};
use ostd::app::{AppContext, AppEvent};
use ostd::child_context::ChildContext;
use ostd::io::println;
use ostd::syscall::sys_exit;
use terminal_sdk::input::EventKind;
use terminal_sdk::session::TerminalSession;

// No hardware caps — input delivery uses IPC only.
api::declare_manifest!(block_io = false, network = false, spawn = false);

ostd::cell_main!(cell_main);

/// One bounded session wait; the legacy focus queue is polled between waits.
const SESSION_SLICE_NS: u64 = 100_000_000;

fn cell_main() {
    println("[input-test] starting");
    // Retry focus request until granted (boot-race: input service may not
    // be registered yet on first attempt).
    while !ostd::input::request_focus() {
        ostd::task::yield_now();
    }
    println("[input-test] focus granted");
    // Launched as a terminal session's foreground child (Cellbox's serial shell):
    // while that session is attached and no compositor is registered, the input
    // service hands physical keys to the session's foreground reader and never to
    // legacy focus (`Dispatcher::serial_byte` / `Dispatcher::dispatch`).
    let session = ChildContext::read_own()
        .ok()
        .and_then(|context| TerminalSession::from_own_context(&context).ok());
    match session {
        Some(session) => session_loop(session),
        None => focus_loop(),
    }
}

/// Print the one-shot success marker integration tests grep for.
fn report_key(done: &mut bool) {
    if !*done {
        *done = true;
        println("[input-test] key received");
        println("[input-test] input ok");
    }
}

fn is_pressed_key(event: &InputEvent) -> bool {
    matches!(event, InputEvent::Key(key) if key.state == KeyState::Pressed)
}

/// Focus-delivered input only: the cell was not launched with a terminal session.
fn focus_loop() -> ! {
    let mut ctx = AppContext::new();
    let mut done = false;
    ctx.run(|_ctx, ev| {
        if let AppEvent::Input(event) = ev {
            if is_pressed_key(&event) {
                report_key(&mut done);
            }
        }
    });
}

/// Read the session's typed input, and keep draining legacy focus events too:
/// with a compositor registered the input service routes keys by focus even to a
/// session child. End of input exits 0 and an interrupt 130, so the shell that
/// launched this cell in the foreground gets its prompt back.
fn session_loop(mut session: TerminalSession) -> ! {
    println("[input-test] session reader ready");
    let mut done = false;
    loop {
        let deadline = match ostd::session::monotonic_ns() {
            Ok(now) => now.saturating_add(SESSION_SLICE_NS),
            Err(_) => sys_exit(3),
        };
        match session.next_event(deadline) {
            Ok(event) => match event.kind {
                EventKind::Text(text) if !text.is_empty() => report_key(&mut done),
                EventKind::Key { pressed: 1, .. } => report_key(&mut done),
                EventKind::Eof => sys_exit(0),
                EventKind::Interrupt => sys_exit(130),
                EventKind::Lost(_) => sys_exit(3),
                _ => {}
            },
            Err(Status::TimedOut) => {}
            Err(_) => sys_exit(3),
        }
        if ostd::input::poll_events(8).iter().any(is_pressed_key) {
            report_key(&mut done);
        }
    }
}
