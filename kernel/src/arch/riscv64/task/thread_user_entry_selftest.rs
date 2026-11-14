// SPDX-License-Identifier: MPL-2.0
//! riscv64 test-hooks body of `crate::task::thread_user_entry_selftest`,
//! compiled as its `rv` child module (`#[path]`); it reads `crate::task`'s
//! private scheduler state as the parent did.

use crate::memory::{cell_quota, paging};
use crate::task::syscall::{handle_syscall, Syscall};
use crate::task::tcb::Task;
use alloc::{boxed::Box, vec::Vec};
use types::CellId;

pub(super) const PARENT_TID: usize = 9202;

pub(super) const TEST_CELL_ID: u64 = (crate::memory::cell_quota::MAX_CELLS - 5) as u64;

pub(super) const ENTRY: usize = 0x0001_4000;

pub(super) const MSG_OFFSET: usize = 0x40;

pub(super) const MSG: &[u8] = b"[selftest] THREAD-ENTRY arg=ok\n";

pub(super) fn insert_parent() {
    let mut parent = Box::new(Task::new(
        PARENT_TID,
        CellId(TEST_CELL_ID),
        "selftest",
        Vec::new(),
    ));
    parent.cell_generation = 1;
    parent.root_tid = PARENT_TID;
    crate::task::cap::CapSet {
        spawn: true,
        ..crate::task::cap::CapSet::EMPTY
    }
    .apply_to(&mut parent);
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        let owner = abi::cell_owner::CellOwner::new(TEST_CELL_ID, 1, PARENT_TID as u64);
        sched.publish_live_cell_owner(owner);
        #[cfg(feature = "p01-trace")]
        crate::memory::heap::p01_owner::task_backing(&parent);
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::SCHEDULER_SHARED,
        );
        sched.tasks.insert(PARENT_TID, parent);
    }
}

pub(super) fn remove(tid: usize) {
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            if (task.cell_id.0 as usize) < crate::memory::cell_quota::MAX_CELLS {
                let owner = abi::cell_owner::CellOwner::new(
                    task.cell_id.0,
                    task.cell_generation,
                    task.root_tid as u64,
                );
                sched.clear_live_cell_owner_for_test(owner);
            }
        }
    }
    crate::sched::hart_local::ready::remove_from_all(tid);
}

pub(super) fn reap() {
    let dead = crate::task::SCHEDULER
        .lock()
        .as_mut()
        .map(|sched| sched.take_reapable_zombies())
        .unwrap_or_default();
    drop(dead);
}

pub(super) fn drain_user_log() -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 128];
    loop {
        let read = crate::task::read_log_ring(&mut buf);
        if read == 0 {
            return out;
        }
        out.extend_from_slice(&buf[..read]);
    }
}

pub(super) fn rv_addi(rd: u32, imm: u32) -> u32 {
    (imm << 20) | (rd << 7) | 0x13
}

pub(super) fn map_test_page() -> Result<usize, &'static str> {
    let flags = paging::Flags::from_bits(
        paging::Flags::VALID
            | paging::Flags::READ
            | paging::Flags::EXECUTE
            | paging::Flags::USER
            | paging::Flags::ACCESSED
            | paging::Flags::DIRTY,
    );
    let mut batch = crate::memory::frame_run::Batch::new();
    let frame = crate::memory::frame::with(|allocator| {
        let frame = allocator.allocate_frame().ok_or("frame unavailable")?;
        paging::map_page(allocator, &mut batch, ENTRY, frame, flags).map_err(|_| "map failed")?;
        Ok(frame)
    })
    .ok_or("allocator unavailable")??;
    let _ = batch.complete();
    unsafe {
        let base = frame as *mut u32;
        *base.add(0) = rv_addi(11, MSG.len() as u32);
        *base.add(1) = 0x00B0_0893;
        *base.add(2) = 0x0000_0073;
        *base.add(3) = 0x0000_0513;
        *base.add(4) = 0x03C0_0893;
        *base.add(5) = 0x0000_0073;
        core::ptr::copy_nonoverlapping(MSG.as_ptr(), (frame + MSG_OFFSET) as *mut u8, MSG.len());
    }
    Ok(frame)
}

pub(super) fn unmap_test_page(frame: usize) {
    let _ = paging::unmap_page(&mut crate::memory::frame_run::Batch::new(), ENTRY);
    paging::tlb_flush_all();
    crate::memory::frame::deallocate_frame(frame);
}

pub fn self_test() -> Option<bool> {
    let saved_next_tid = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .map(|sched| sched.next_task_id);
    let _ = drain_user_log();
    let frame = match map_test_page() {
        Ok(frame) => frame,
        Err(err) => {
            log::error!("[selftest] THREAD-ENTRY: FAIL — {}", err);
            return Some(false);
        }
    };

    cell_quota::register(CellId(TEST_CELL_ID), cell_quota::DEFAULT_QUOTA_BYTES);
    insert_parent();
    let spawned = handle_syscall(
        PARENT_TID,
        Syscall::Spawn {
            entry: ENTRY,
            arg: ENTRY + MSG_OFFSET,
        },
    );

    let mut ok = true;
    let mut spawned_tid = None;
    match spawned {
        Ok(tid) if tid != 0 => {
            spawned_tid = Some(tid);
            crate::task::yield_cpu();
        }
        other => {
            ok = false;
            log::error!("[selftest] THREAD-ENTRY: FAIL — spawn returned {:?}", other);
        }
    }

    reap();
    let log_bytes = drain_user_log();
    if !log_bytes.windows(MSG.len()).any(|window| window == MSG) {
        ok = false;
        log::error!("[selftest] THREAD-ENTRY: FAIL — marker missing after yield");
    }

    if let Some(tid) = spawned_tid {
        let still_live = crate::task::SCHEDULER.lock().as_ref().is_some_and(|sched| {
            sched.tasks.contains_key(&tid) || sched.zombies.iter().any(|task| task.id == tid)
        });
        if still_live {
            ok = false;
            log::error!("[selftest] THREAD-ENTRY: FAIL — tid {} never exited", tid);
            if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
                sched.exit_task(tid, usize::MAX);
            }
            reap();
        }
    }

    remove(PARENT_TID);
    cell_quota::deregister(CellId(TEST_CELL_ID));
    unmap_test_page(frame);
    if let (Some(sched), Some(next_tid)) = (crate::task::SCHEDULER.lock().as_mut(), saved_next_tid)
    {
        sched.next_task_id = next_tid;
    }

    if ok {
        log::info!("[selftest] THREAD-ENTRY: PASS (user entry reached arg)");
    } else {
        log::error!("[selftest] THREAD-ENTRY: FAIL");
    }
    Some(ok)
}
