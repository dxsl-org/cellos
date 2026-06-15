#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use api::ipc::{IPC_BUF_SIZE, VfsRequest, VfsResponse};
use ostd::app::{AppContext, AppEvent};
use ostd::service::VfsRef;
use ostd::io::println;

// No hardware caps required — demo communicates via IPC only.
api::declare_manifest!(block_io = false, network = false, spawn = false);

#[no_mangle]
pub fn main() {
    println("[sdk-demo] ViCell App SDK demo starting");

    // ── Phase 1: service lookup via ServiceRef ────────────────────────────────
    let mut vfs: VfsRef = VfsRef::new();
    let mut resp_buf = [0u8; IPC_BUF_SIZE];

    match vfs.call(&VfsRequest::Stat("/"), &mut resp_buf) {
        Ok(VfsResponse::Stat { size, is_dir }) => {
            let s = format!("[sdk-demo] VFS stat('/') ok — size={size} is_dir={is_dir}");
            println(&s);
        }
        Ok(_) => println("[sdk-demo] VFS stat: unexpected response variant"),
        Err(_) => {
            // VFS may not be registered in minimal test boots — that is expected.
            println("[sdk-demo] VFS not available (service lookup returned None)");
        }
    }

    // ── Phase 2: AppContext event loop (runs until Shutdown or one message) ───
    println("[sdk-demo] entering AppContext event loop");

    let mut ctx = AppContext::new();
    let mut received = 0u32;

    ctx.run(move |ctx, event| {
        match event {
            AppEvent::Message { sender_tid, data } => {
                received += 1;
                let reply = format!(
                    "[sdk-demo] echo #{received} from tid={sender_tid} ({} bytes)",
                    data.len()
                );
                println(&reply);
                // Echo payload back.
                ctx.send_msg(sender_tid, &data).ok();

                if received >= 3 {
                    // Gracefully exit after 3 messages to keep the demo bounded.
                    println("[sdk-demo] 3 messages handled — exiting");
                    ostd::syscall::sys_exit(0);
                }
            }
            AppEvent::RawMessage { sender_tid, data } => {
                let s = format!(
                    "[sdk-demo] raw msg from tid={sender_tid} ({} bytes) — ignoring",
                    data.len()
                );
                println(&s);
            }
            AppEvent::Shutdown => {
                println("[sdk-demo] shutdown requested — exiting");
                ostd::syscall::sys_exit(0);
            }
            AppEvent::Input(_) => {} // sdk-demo does not request input focus
            AppEvent::Timeout => {}
        }
    });
}
