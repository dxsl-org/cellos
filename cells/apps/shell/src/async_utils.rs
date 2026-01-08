use ostd::prelude::*;
use core::future::Future;
use core::task::{Context, Poll};
use ostd::executor::yield_now;

pub struct AsyncStdin;

impl AsyncStdin {
    pub async fn read_line(&self, buffer: &mut [u8]) -> usize {
        let mut idx = 0;
        loop {
            if idx >= buffer.len() {
                break;
            }
            let mut c = [0u8; 1];
            // sys_read is blocking/yielding in kernel, but here we treat it as "potentially blocking"
            // To make this "async" in spirit (allowing other tasks to run if we had any),
            // we could yield before reading if we suspect no data.
            // But sys_read is the one that blocks.
            // So for now, we just call it.
            //
            // Ideally, we would have `sys_read_async` or `poll_read`.
            // Design 11 says "Async Shell: Shell will not block when waiting for RAM Disk".
            // Since we don't have true async syscalls exposed yet in ostd::syscall (they return Result, not Future),
            // and sys_read is likely blocking in the kernel implementation (waiting for interrupt),
            // we are limited.
            //
            // However, we can simulate async structure.

            // Try to read 1 byte
            match ostd::syscall::sys_read(0, &mut c) {
                Ok(n) if n > 0 => {
                     let ch = c[0];
                     if ch == b'\r' || ch == b'\n' {
                         ostd::io::print("\n");
                         break;
                     }
                     if ch == 8 || ch == 127 { // Backspace
                         if idx > 0 {
                             ostd::io::print("\x08 \x08");
                             idx -= 1;
                         }
                         continue;
                     }
                     // Echo
                     if let Ok(s) = core::str::from_utf8(&c) {
                         ostd::io::print(s);
                     }
                     buffer[idx] = ch;
                     idx += 1;
                },
                _ => {
                    // If read failed or returned 0 (and not EOF intended), yield.
                    yield_now().await;
                }
            }
        }
        idx
    }
}
