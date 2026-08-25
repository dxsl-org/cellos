//! Case C: Syscall::RecvScatter valid-first / invalid-later atomicity regression test.

use super::{
    cleanup_task, fill_page, read_page, RECEIVER_CELL, RECEIVER_TID, RECEIVER_VA, SENDER_CELL,
    SENDER_TID,
};
use crate::memory::address_space::AddressSpace;
use crate::memory::frame::phys_to_virt;
use crate::task::syscall::{handle_syscall, Syscall, SyscallError};

pub(super) fn run_scatter_case(harts: usize, receiver_space: &AddressSpace) -> bool {
    const SCATTER_MSG_LEN: usize = 64;
    const IOVEC_ARRAY_OFFSET: usize = 2048;
    const SENTINEL_VAL: u8 = 0xA5;

    // Post a message to the receiver.
    let msg = [0x42u8; SCATTER_MSG_LEN];
    if crate::task::ipc_post_nonblock(SENDER_TID, RECEIVER_TID, &msg).is_err() {
        log::error!("S22-RV64-IPC-SCATTER: FAIL post message");
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    }

    // Fill receiver destination buffer with sentinel.
    fill_page(receiver_space, RECEIVER_VA, SENTINEL_VAL, 128);

    // Write an iovec array with:
    // iovec[0]: (RECEIVER_VA, 32) -> valid mapped destination
    // iovec[1]: (0x0500_0000, 32) -> unmapped invalid destination
    let iovec_ptr = RECEIVER_VA + IOVEC_ARRAY_OFFSET;
    let mut iovec_raw = [0u8; 32];
    iovec_raw[..8].copy_from_slice(&RECEIVER_VA.to_ne_bytes());
    iovec_raw[8..16].copy_from_slice(&32usize.to_ne_bytes());
    iovec_raw[16..24].copy_from_slice(&0x0500_0000usize.to_ne_bytes());
    iovec_raw[24..32].copy_from_slice(&32usize.to_ne_bytes());

    // Write iovec array into receiver's mapped page through physical alias.
    let Some((_, rx_pa)) = receiver_space.page_proof_for(RECEIVER_VA) else {
        log::error!("S22-RV64-IPC-SCATTER: FAIL receiver proof");
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    };
    unsafe {
        for (i, &b) in iovec_raw.iter().enumerate() {
            (phys_to_virt(rx_pa + IOVEC_ARRAY_OFFSET + i) as *mut u8).write_volatile(b);
        }
    }

    // Invoke Syscall::RecvScatter with the valid-first/invalid-later iovec.
    let scatter_err = handle_syscall(
        RECEIVER_TID,
        Syscall::RecvScatter {
            mask: 0,
            iovec_ptr,
            iovec_count: 2,
        },
    );

    // 1. Syscall must return Err(SyscallError::InvalidInput).
    let err_ok = matches!(scatter_err, Err(SyscallError::InvalidInput));

    // 2. Earlier destination (RECEIVER_VA .. RECEIVER_VA + 32) must be COMPLETELY UNTOUCHED.
    let first_dst_untouched = read_page(receiver_space, RECEIVER_VA, SENTINEL_VAL, 32);

    // 3. Message must STILL be queued in receiver's pending_msgs.
    let msg_retained = crate::task::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&RECEIVER_TID).is_some_and(|t| {
            t.pending_msgs
                .iter()
                .any(|m| m.sender_tid == SENDER_TID && m.payload() == msg)
        })
    });

    if !err_ok || !first_dst_untouched || !msg_retained {
        log::error!(
            "S22-RV64-IPC-SCATTER: FAIL atomicity err_ok={} untouched={} retained={}",
            err_ok,
            first_dst_untouched,
            msg_retained
        );
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    }

    // 4. Now perform a valid scatter receive to prove the retained message can be dequeued.
    iovec_raw[16..24].copy_from_slice(&(RECEIVER_VA + 64).to_ne_bytes());
    unsafe {
        for (i, &b) in iovec_raw.iter().enumerate() {
            (phys_to_virt(rx_pa + IOVEC_ARRAY_OFFSET + i) as *mut u8).write_volatile(b);
        }
    }

    let scatter_ok = handle_syscall(
        RECEIVER_TID,
        Syscall::RecvScatter {
            mask: 0,
            iovec_ptr,
            iovec_count: 2,
        },
    );

    let scatter_res_ok = scatter_ok == Ok(SENDER_TID);
    let first_chunk_ok = read_page(receiver_space, RECEIVER_VA, 0x42, 32);
    let second_chunk_ok = read_page(receiver_space, RECEIVER_VA + 64, 0x42, 32);
    let queue_drained = crate::task::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched
            .tasks
            .get(&RECEIVER_TID)
            .is_some_and(|t| t.pending_msgs.is_empty())
    });

    if !scatter_res_ok || !first_chunk_ok || !second_chunk_ok || !queue_drained {
        log::error!(
            "S22-RV64-IPC-SCATTER: FAIL valid scatter_ok={} chunk1={} chunk2={} drained={}",
            scatter_res_ok,
            first_chunk_ok,
            second_chunk_ok,
            queue_drained
        );
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    }
    log::info!("S22-RV64-IPC-SCATTER: PASS harts={}", harts);
    true
}
