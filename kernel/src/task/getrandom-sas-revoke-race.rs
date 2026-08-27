use super::super::{hart_local, smp, stack::Stack, syscall, SCHEDULER};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use types::CellId;

#[path = "getrandom-sas-race-actions.rs"]
mod actions;

const RAW_GETRANDOM: usize = 214;
const SPIN_LIMIT: usize = 400_000_000;
const WORKER_CELL: u64 = 91_304;

static CALLER_ID: AtomicUsize = AtomicUsize::new(0);
static GRANT_BASE: AtomicUsize = AtomicUsize::new(0);
static RACE_MODE: AtomicUsize = AtomicUsize::new(0);
static REPLACEMENT_BASE: AtomicUsize = AtomicUsize::new(0);
static WORKER_TID: AtomicUsize = AtomicUsize::new(0);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static WORKER_RESULT: AtomicUsize = AtomicUsize::new(0);

fn retire_worker(worker_tid: usize) -> ! {
    if let Some(scheduler) = SCHEDULER.lock().as_mut() {
        scheduler.exit_task(worker_tid, 0);
    }
    super::super::yield_cpu();
    loop {
        core::hint::spin_loop();
    }
}

extern "C" fn revoker_entry() -> ! {
    let caller_id = CALLER_ID.load(Ordering::Acquire);
    let grant_base = GRANT_BASE.load(Ordering::Acquire);
    let mode = RACE_MODE.load(Ordering::Acquire);
    let worker_tid = WORKER_TID.load(Ordering::Acquire);
    WORKER_STARTED.store(true, Ordering::Release);

    let mut entered = false;
    for _ in 0..SPIN_LIMIT {
        if syscall::test_getrandom_revoke_race_entered() {
            entered = true;
            break;
        }
        core::hint::spin_loop();
    }
    if !entered {
        WORKER_RESULT.store(2, Ordering::Release);
        syscall::test_finish_getrandom_revoke_race();
        retire_worker(worker_tid);
    }

    let probe_busy = syscall::test_probe_getrandom_revoke_lock();
    let (removal_complete, replacement) = actions::complete(caller_id, grant_base, mode);
    REPLACEMENT_BASE.store(replacement, Ordering::Release);
    WORKER_RESULT.store((probe_busy && removal_complete) as usize, Ordering::Release);
    syscall::test_finish_getrandom_revoke_race();
    retire_worker(worker_tid)
}

fn spawn_revoker() -> Option<usize> {
    let mut guard = SCHEDULER.lock();
    let scheduler = guard.as_mut()?;
    let kernel_stack = Stack::new_kernel(crate::task::STACK_PAGES).ok()?;
    let user_stack = Stack::new_user(1).ok()?;
    let worker_tid = scheduler.spawn_with_stacks(
        "getrandom-grant-racer",
        CellId(WORKER_CELL),
        Vec::new(),
        kernel_stack,
        user_stack,
    );
    WORKER_TID.store(worker_tid, Ordering::Release);
    hart_local::ready::remove_from_all(worker_tid);
    let priority = {
        let worker = scheduler.tasks.get_mut(&worker_tid)?;
        worker.context.ra = revoker_entry as *const () as usize;
        worker.priority
    };
    if !hart_local::ready::reserve_test_dispatch_on_hart(smp::HART_RT, worker_tid) {
        return None;
    }
    hart_local::ready::push_on_hart(smp::HART_RT, worker_tid, priority);
    Some(worker_tid)
}

fn worker_quiesced(worker_tid: usize) -> bool {
    for _ in 0..SPIN_LIMIT {
        if hart_local::ready::current_task_id_for(smp::HART_RT) == 0
            && !hart_local::ready::any_hart_running(worker_tid)
        {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn post_race_ok(caller_id: usize, grant_base: usize, mode: usize) -> bool {
    match mode {
        actions::ROOT_RETIRE => actions::root_is_terminal(caller_id),
        actions::REVOKE => matches!(
            syscall::dispatch_raw_for_test(caller_id, RAW_GETRANDOM, grant_base + 32, 64, 0, 0),
            Err(syscall::SyscallError::InvalidInput | syscall::SyscallError::PermissionDenied)
        ),
        actions::UNMAP_REUSE => REPLACEMENT_BASE.load(Ordering::Acquire) == grant_base,
        _ => false,
    }
}

fn run_case(caller_id: usize, grant_base: usize, mode: usize) -> bool {
    CALLER_ID.store(caller_id, Ordering::Release);
    GRANT_BASE.store(grant_base, Ordering::Release);
    RACE_MODE.store(mode, Ordering::Release);
    REPLACEMENT_BASE.store(0, Ordering::Release);
    WORKER_STARTED.store(false, Ordering::Release);
    WORKER_RESULT.store(0, Ordering::Release);
    syscall::test_arm_getrandom_revoke_race(caller_id);

    let Some(worker_tid) = spawn_revoker() else {
        return false;
    };
    let Some((mask, base)) = smp::logical_sbi_target(smp::HART_RT) else {
        return false;
    };
    if crate::hal::common::sbi::sbi_send_ipi(mask, base).is_err() {
        return false;
    }
    for _ in 0..SPIN_LIMIT {
        if WORKER_STARTED.load(Ordering::Acquire) {
            break;
        }
        core::hint::spin_loop();
    }
    if !WORKER_STARTED.load(Ordering::Acquire) {
        return false;
    }

    let committed =
        syscall::dispatch_raw_for_test(caller_id, RAW_GETRANDOM, grant_base + 32, 64, 0, 0)
            == Ok(64);
    for _ in 0..SPIN_LIMIT {
        if syscall::test_getrandom_revoke_race_result().2 {
            break;
        }
        core::hint::spin_loop();
    }
    let (probed_busy, no_early_done, done, probe_timed_out) =
        syscall::test_getrandom_revoke_race_result();
    let quiesced = done && worker_quiesced(worker_tid);
    let post_ok = quiesced && post_race_ok(caller_id, grant_base, mode);
    let ok = committed
        && probed_busy
        && !probe_timed_out
        && no_early_done
        && done
        && WORKER_RESULT.load(Ordering::Acquire) == 1
        && quiesced
        && post_ok;
    if !ok {
        log::warn!(
            "[selftest] getrandom-sas-race: mode={} committed={} probed_busy={} no_early_done={} done={} timed_out={} worker={} post_ok={}",
            mode,
            committed,
            probed_busy,
            no_early_done,
            done,
            probe_timed_out,
            WORKER_RESULT.load(Ordering::Acquire),
            post_ok
        );
    }
    ok
}

/// Race final authorization against real root retirement, revocation, and unmap/reuse.
pub(super) fn run(caller_id: usize, grant_base: usize) -> bool {
    if !smp::is_rt_hart_online() {
        return false;
    }
    let revoke_ok = run_case(caller_id, grant_base, actions::REVOKE);
    let reuse_base = if revoke_ok {
        syscall::dispatch_raw_for_test(caller_id, 215, 128, 0, 0, 0).unwrap_or(0)
    } else {
        0
    };
    let reuse_ok = reuse_base != 0 && run_case(caller_id, reuse_base, actions::UNMAP_REUSE);
    let replacement = REPLACEMENT_BASE.load(Ordering::Acquire);
    let reuse_cleaned = replacement != 0
        && syscall::test_unregister_registered_grant_for_race(caller_id, replacement).is_ok();
    let retirement_ok = actions::prepare_root().is_some_and(|(root_caller, root_grant)| {
        run_case(root_caller, root_grant, actions::ROOT_RETIRE)
    });
    revoke_ok && reuse_ok && reuse_cleaned && retirement_ok
}
