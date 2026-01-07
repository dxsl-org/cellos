#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use alloc::string::String;
use ostd::io::{print, println, stdin};

#[no_mangle]
pub fn main() {
    let _ = ostd::syscall::sys_log("DEBUG: Start\n");

    loop {
        // Stack-based prompt to avoid rodata issues?
        let p = [b'V', b'>', b' '];
        if let Ok(s) = core::str::from_utf8(&p) {
            let _ = ostd::syscall::sys_log(s);
        }

        // Manual Simple Read
        let mut buffer = [0u8; 64];
        let mut idx = 0;
        
        loop {
            let mut c = [0u8; 1];
            if let Ok(n) = ostd::syscall::sys_read(0, &mut c) {
                if n > 0 {
                    let ch = c[0];
                    if ch == b'\r' || ch == b'\n' {
                         let nl = [b'\n'];
                         if let Ok(s) = core::str::from_utf8(&nl) {
                             let _ = ostd::syscall::sys_log(s);
                         }
                        break;
                    }
                    if idx < 64 {
                        buffer[idx] = ch;
                        idx += 1;
                        // Echo
                        let e = [ch];
                        if let Ok(s) = core::str::from_utf8(&e) {
                             let _ = ostd::syscall::sys_log(s);
                         }
                    }
                }
            }
            ostd::syscall::sys_yield();
        }
        
        let cmd_str = core::str::from_utf8(&buffer[0..idx]).unwrap_or("");
        // Compare with stack bytes to avoid rodata literals for "help"
        // "help" = [104, 101, 108, 112]
        if cmd_str.as_bytes() == [104, 101, 108, 112] {
             let msg = [b'O', b'K', b'\n'];
             let _ = ostd::syscall::sys_log(core::str::from_utf8(&msg).unwrap_or(""));
        }
    }
}
