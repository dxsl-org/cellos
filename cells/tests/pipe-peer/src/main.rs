//! Writer half of the phase-05 cross-domain pipe witness.
//!
//! The parent grants a write endpoint with `PipeShare`, then sends the opaque
//! handle token through ordinary IPC. Receiving the token does not grant access:
//! the kernel-owned task handle table is the authority checked by `PipeWrite`.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;
use alloc::vec::Vec;
use ostd::syscall::{
    sys_exit, sys_get_scheduler_ticks, sys_pipe_write, sys_recv, sys_send, sys_yield, PipeHandle,
    SyscallResult,
};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_FFI
);

api::declare_syscalls![Log, Exit, Send, Recv, Yield, GetTime, PipeWrite];

ostd::cell_main!(cell_main);

const CAPACITY: usize = 256;
const PAYLOAD: usize = 1024;
const PEER_FILLED_RING: [u8; 8] = *b"PIPEFULL";
const PEER_CLOSED: [u8; 8] = *b"PIPECLOS";

fn payload_byte(index: usize) -> u8 {
    (index % 251) as u8
}

fn fail() -> ! {
    sys_exit(1);
}

fn cell_main() {
    let mut handle_bytes = [0u8; core::mem::size_of::<usize>()];
    let parent_tid = match sys_recv(0, &mut handle_bytes) {
        SyscallResult::Ok(tid) if tid != 0 => tid,
        _ => fail(),
    };
    let handle = PipeHandle(usize::from_ne_bytes(handle_bytes));

    let mut written_total = 0usize;
    let mut full_ring_wake_ticks = 0u64;
    while written_total < PAYLOAD {
        let waiting_for_space = written_total == CAPACITY;
        let wait_started_at = waiting_for_space.then(|| sys_get_scheduler_ticks().unwrap_or(0));
        let chunk: Vec<u8> = (written_total..(written_total + 64).min(PAYLOAD))
            .map(payload_byte)
            .collect();
        match sys_pipe_write(handle, &chunk, 0) {
            Ok(0) => fail(),
            Ok(written) => {
                written_total += written;
                if let Some(started_at) = wait_started_at {
                    full_ring_wake_ticks = sys_get_scheduler_ticks()
                        .unwrap_or(started_at)
                        .saturating_sub(started_at);
                }
                if written_total == CAPACITY
                    && !matches!(
                        sys_send(parent_tid, &PEER_FILLED_RING),
                        SyscallResult::Ok(_)
                    )
                {
                    fail();
                }
            }
            Err(_) => fail(),
        }
        sys_yield();
    }

    // The parent closed its duplicate before this cell began writing, so this
    // is the last writer endpoint and must turn an empty reader into EOF.
    if !matches!(sys_pipe_write(handle, &[], 0), Ok(0)) {
        fail();
    }
    let mut completion = [0u8; 16];
    completion[..8].copy_from_slice(&PEER_CLOSED);
    completion[8..].copy_from_slice(&full_ring_wake_ticks.to_ne_bytes());
    if !matches!(sys_send(parent_tid, &completion), SyscallResult::Ok(_)) {
        fail();
    }
    sys_exit(0);
}
