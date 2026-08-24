//! Boot self-test for real user-thread first entry.
//!
//! This proves more than "the thread became Ready": the real `Syscall::Spawn`
//! path must switch into U-mode, hand the thread its `arg`, and let it complete
//! a syscall/exit round-trip before control returns to the boot context.

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
use super::syscall::{handle_syscall, Syscall};
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
use super::tcb::Task;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
use alloc::{boxed::Box, vec::Vec};
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
use types::CellId;

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
use crate::memory::{cell_quota, frame::FRAME_ALLOCATOR, paging};

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
const PARENT_TID: usize = 9202;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
const TEST_CELL_ID: u64 = (crate::memory::cell_quota::MAX_CELLS - 5) as u64;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
const ENTRY: usize = 0x0001_4000;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
const MSG_OFFSET: usize = 0x40;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
const MSG: &[u8] = b"[selftest] THREAD-ENTRY arg=ok\n";

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn insert_parent() {
    let mut parent = Box::new(Task::new(
        PARENT_TID,
        CellId(TEST_CELL_ID),
        "selftest",
        Vec::new(),
    ));
    parent.cell_generation = 1;
    parent.root_tid = PARENT_TID;
    parent.spawn_cap = Some(super::cap::SpawnCap::new());
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        let owner = api::cell_owner::CellOwner::new(TEST_CELL_ID, 1, PARENT_TID as u64);
        sched.publish_live_cell_owner(owner);
        sched.tasks.insert(PARENT_TID, parent);
    }
}

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            if (task.cell_id.0 as usize) < crate::memory::cell_quota::MAX_CELLS {
                let owner = api::cell_owner::CellOwner::new(
                    task.cell_id.0,
                    task.cell_generation,
                    task.root_tid as u64,
                );
                sched.clear_live_cell_owner_for_test(owner);
            }
        }
    }
    super::hart_local::ready::remove_from_all(tid);
}
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn reap() {
    let dead = super::SCHEDULER
        .lock()
        .as_mut()
        .map(|sched| sched.take_reapable_zombies())
        .unwrap_or_default();
    drop(dead);
}

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn drain_user_log() -> Vec<u8> {
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

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn rv_addi(rd: u32, imm: u32) -> u32 {
    (imm << 20) | (rd << 7) | 0x13
}

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn map_test_page() -> Result<usize, &'static str> {
    let mut frame_guard = FRAME_ALLOCATOR.lock();
    let allocator = frame_guard.as_mut().ok_or("allocator unavailable")?;
    let frame = allocator.allocate_frame().ok_or("frame unavailable")?;
    let flags = paging::Flags::from_bits(
        paging::Flags::VALID
            | paging::Flags::READ
            | paging::Flags::EXECUTE
            | paging::Flags::USER
            | paging::Flags::ACCESSED
            | paging::Flags::DIRTY,
    );
    paging::map_page(allocator, ENTRY, frame, flags).map_err(|_| "map failed")?;
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

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn unmap_test_page(frame: usize) {
    let _ = paging::unmap_page(ENTRY);
    paging::tlb_flush_all();
    if let Some(allocator) = FRAME_ALLOCATOR.lock().as_mut() {
        allocator.deallocate_frame(frame);
    }
}

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub fn self_test() -> Option<bool> {
    let saved_next_tid = super::SCHEDULER
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
        let still_live = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
            sched.tasks.contains_key(&tid) || sched.zombies.iter().any(|task| task.id == tid)
        });
        if still_live {
            ok = false;
            log::error!("[selftest] THREAD-ENTRY: FAIL — tid {} never exited", tid);
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                sched.exit_task(tid, usize::MAX);
            }
            reap();
        }
    }

    remove(PARENT_TID);
    cell_quota::deregister(CellId(TEST_CELL_ID));
    unmap_test_page(frame);
    if let (Some(sched), Some(next_tid)) = (super::SCHEDULER.lock().as_mut(), saved_next_tid) {
        sched.next_task_id = next_tid;
    }

    if ok {
        log::info!("[selftest] THREAD-ENTRY: PASS (user entry reached arg)");
    } else {
        log::error!("[selftest] THREAD-ENTRY: FAIL");
    }
    Some(ok)
}

#[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
pub fn self_test() -> Option<bool> {
    None
}
