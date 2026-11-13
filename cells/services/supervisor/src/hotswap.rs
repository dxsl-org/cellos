//! Hotswap orchestration ported from kernel/src/cell/hotswap.rs.
//!
//! Kernel mechanisms replaced by SupervisorCap syscalls:
//!   set_task_frozen(tid)   → sys_freeze_cell(tid)
//!   unfreeze_task(tid)     → sys_resume_cell(tid)
//!   exit_task_internal(..) → sys_kill_cell(tid, 0xAAAA_AAAA)
//!
//! Polling replaced by yield-and-retry loops using sys_yield() + sys_state_restore()
//! and sys_query_hotswap_ready(), each bounded by `WAIT_TICKS` scheduler ticks
//! (one tick ≈ 10 ms, so 5 seconds).

extern crate alloc;

use crate::transfer::{send_restore_event, send_snapshot_event};
use api::services::proto::supervisor::HotswapError;
use ostd::hotswap::hotswap_key;
use ostd::service::namespace_flags;
use ostd::service::provider_tid;
use ostd::syscall::{
    sys_commit_hotswap, sys_freeze_cell, sys_get_scheduler_ticks, sys_kill_cell, sys_pause_service,
    sys_query_hotswap_ready, sys_register_service, sys_resume_cell, sys_spawn_replacement,
    sys_state_restore, sys_state_stash_clear, sys_yield,
};

/// Budget for the stash and for readiness, in 10 ms scheduler ticks (5 s). The
/// snapshot and restore events are queued, not handed over, so the budget is
/// time, not polls: it covers the target waking and, for restore, the new cell's
/// whole startup.
const WAIT_TICKS: u64 = 500;

/// Whether `WAIT_TICKS` have passed since `start`; an unreadable clock counts
/// as expired so a wait can never become unbounded.
fn expired(start: Option<u64>) -> bool {
    match (start, sys_get_scheduler_ticks()) {
        (Some(start), Some(now)) => now.saturating_sub(start) >= WAIT_TICKS,
        _ => true,
    }
}

// ── Poll helpers ──────────────────────────────────────────────────────────────

/// Poll until the stash entry for `key` exists, yielding between iterations.
///
/// Uses the empty-buffer status probe (`sys_state_restore(key, &mut [])` returns the
/// stored length): the supervisor may see that state exists, never its bytes.
/// Returns `Err(SnapshotTimeout)` after `WAIT_TICKS` without success.
///
/// A stash miss is tolerated here: a target that hands over no state never
/// stashes, and its replacement then never signals Ready, so the swap rolls back.
fn wait_for_stash_key(key: u64) -> Result<(), HotswapError> {
    let start = sys_get_scheduler_ticks();
    loop {
        if sys_state_restore(key, &mut []) > 0 {
            return Ok(());
        }
        if expired(start) {
            return Err(HotswapError::SnapshotTimeout);
        }
        sys_yield();
    }
}

/// Poll until the new cell has called `sys_hotswap_ready()`, yielding between
/// iterations.
///
/// Returns `Ok(())` when the flag is set, `Err(ReadyTimeout)` on timeout.
fn wait_for_hotswap_ready(new_tid: usize) -> Result<(), HotswapError> {
    let start = sys_get_scheduler_ticks();
    loop {
        match sys_query_hotswap_ready(new_tid) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => return Err(HotswapError::ReadyTimeout), // tid vanished
        }
        if expired(start) {
            return Err(HotswapError::ReadyTimeout);
        }
        sys_yield();
    }
}

// ── IPC senders ───────────────────────────────────────────────────────────────

// ── Swap-ID counter (monotone — wraps after u64::MAX, which is fine) ─────────

use core::sync::atomic::{AtomicU64, Ordering};
static SWAP_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
fn next_swap_id() -> u64 {
    SWAP_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Execute a 5-phase live hotswap of the service identified by `service_id`
/// with the new ELF at `new_elf_path`.
///
/// Returns the new task tid on success.
///
/// # Phases
/// 1. FREEZE (soft) — lookup service tid, freeze old cell
/// 2. SERIALIZE — send Snapshot IPC, wait for stash entry
/// 3. FREEZE (hard) — apply TaskState::Frozen
/// 4. SPAWN — load new ELF via SpawnCap
/// 5. DESERIALIZE — send Restore IPC, wait for HotswapReady
/// 6. COMMIT — atomically drain/close old ingress and publish the replacement
pub fn hotswap(service_id: u16, new_elf_path: &str) -> Result<usize, HotswapError> {
    // ── Resolve target tid ────────────────────────────────────────────────────
    let old_tid = provider_tid(service_id).ok_or(HotswapError::ServiceNotFound)?;

    let swap_id = next_swap_id();
    let stash_key = hotswap_key(swap_id);

    // ── Step 1a: FREEZE (soft) ────────────────────────────────────────────────
    // The old cell must still run to receive the Snapshot IPC.  We freeze the
    // service registry entry so new callers retry during the swap window.
    wait_for_service_pause(service_id, old_tid)?;

    // ── Step 2: SERIALIZE ─────────────────────────────────────────────────────
    if let Err(e) = send_snapshot_event(old_tid, swap_id) {
        let _ = sys_register_service(service_id, old_tid, namespace_flags(service_id));
        return Err(e);
    }

    // Wait for old cell to stash state. A timeout is not fatal here: a target
    // without state never stashes, and the restore step decides.
    match wait_for_stash_key(stash_key) {
        Ok(()) | Err(HotswapError::SnapshotTimeout) => {}
        Err(e) => {
            let _ = sys_register_service(service_id, old_tid, namespace_flags(service_id));
            return Err(e);
        }
    }

    // ── Step 3: SPAWN ─────────────────────────────────────────────────────────
    // Snapshot is complete; now stop the old task and publish its cap ceiling.
    if sys_freeze_cell(old_tid).is_err() {
        let _ = sys_register_service(service_id, old_tid, namespace_flags(service_id));
        sys_state_stash_clear(stash_key);
        return Err(HotswapError::FreezeFailed);
    }

    let new_tid = {
        let result = sys_spawn_replacement(old_tid, new_elf_path);
        match result {
            ostd::syscall::SyscallResult::Ok(tid) => tid,
            ostd::syscall::SyscallResult::Err(_) => {
                sys_resume_cell(old_tid).ok();
                let _ = sys_register_service(service_id, old_tid, namespace_flags(service_id));
                sys_state_stash_clear(stash_key);
                return Err(HotswapError::SpawnFailed);
            }
        }
    };

    // ── Step 4: DESERIALIZE ───────────────────────────────────────────────────
    if let Err(error) = send_restore_event(new_tid, swap_id) {
        rollback_spawned(service_id, old_tid, new_tid, stash_key);
        return Err(error);
    }
    if let Err(error) = wait_for_hotswap_ready(new_tid) {
        rollback_spawned(service_id, old_tid, new_tid, stash_key);
        return Err(error);
    }

    // ── Step 5: COMMIT ────────────────────────────────────────────────────────
    // The kernel closes old ingress, moves its FIFO, and publishes new_tid at
    // one cutover barrier. Before this succeeds rollback to old remains valid.
    if sys_commit_hotswap(old_tid, new_tid, service_id).is_err() {
        rollback_spawned(service_id, old_tid, new_tid, stash_key);
        return Err(HotswapError::RegisterFailed);
    }

    // Terminate the old cell (it is Frozen at this point — KillCell bypasses the
    // Frozen kill-guard in the kernel, same as exit_task_internal in hotswap.rs).
    sys_kill_cell(old_tid, 0xAAAA_AAAA_u32).ok();

    // Clean up stash slot.
    sys_state_stash_clear(stash_key);

    Ok(new_tid)
}

fn rollback_spawned(service_id: u16, old_tid: usize, new_tid: usize, stash_key: u64) {
    let _ = sys_kill_cell(new_tid, 0xAAAA_AAAB_u32);
    sys_resume_cell(old_tid).ok();
    let _ = sys_register_service(service_id, old_tid, namespace_flags(service_id));
    sys_state_stash_clear(stash_key);
}

fn wait_for_service_pause(service_id: u16, old_tid: usize) -> Result<(), HotswapError> {
    let start = sys_get_scheduler_ticks();
    loop {
        if sys_pause_service(service_id, old_tid).is_ok() {
            return Ok(());
        }
        if expired(start) {
            let _ = sys_register_service(service_id, old_tid, namespace_flags(service_id));
            return Err(HotswapError::PauseFailed);
        }
        sys_yield();
    }
}
