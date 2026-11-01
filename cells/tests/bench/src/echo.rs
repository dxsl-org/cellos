//! The 64-byte request / 1-byte reply echo peer shared by every IPC scenario.
//!
//! A valid request is `0x42` followed by zeros and is answered with `0`; any
//! other request is answered with `1`. Used by the Tier-1 SAS peer
//! (`bench-probe ipc-echo`) and the Tier-2 domain peer (`bench-peer-t2`), so the
//! two scenarios differ only in the peer's protection tier.

use ostd::syscall::{sys_recv, sys_send, SyscallResult};

pub fn run() -> ! {
    let mut buf = [0xa5u8; 64];
    loop {
        buf.fill(0xa5);
        let sender = match sys_recv(0, &mut buf) {
            SyscallResult::Ok(sid) if sid != 0 => sid,
            _ => continue,
        };
        let valid_request = buf[0] == 0x42 && buf[1..].iter().all(|&byte| byte == 0);
        let reply = if valid_request { 0 } else { 1 };
        let _ = sys_send(sender, &[reply]);
    }
}
