#![no_std]
#![no_main]

extern crate ostd;

use api::input::{InputEvent, KeyState};
use ostd::app::{AppContext, AppEvent};
use ostd::io::println;

// No hardware caps — input delivery uses IPC only.
api::declare_manifest!(block_io = false, network = false, spawn = false);

#[no_mangle]
pub fn main() {
    println("[input-test] starting");
    let mut ctx = AppContext::new();
    // Retry focus request until granted (boot-race: input service may not
    // be registered yet on first attempt).
    while !ctx.request_input_focus() {
        ostd::task::yield_now();
    }
    println("[input-test] focus granted");
    ctx.run(|_ctx, ev| {
        if let AppEvent::Input(InputEvent::Key(k)) = ev {
            match k.state {
                KeyState::Pressed => println("[input-test] key received"),
                _ => {}
            }
        }
    });
}
