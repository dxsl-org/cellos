//! Hotswap orchestration ported from kernel/src/cell/hotswap.rs.
//!
//! Kernel mechanisms replaced by SupervisorCap syscalls:
//!   set_task_frozen(tid)   → sys_freeze_cell(tid)
//!   unfreeze_task(tid)     → sys_resume_cell(tid)
//!   exit_task_internal(..) → sys_kill_cell(tid, 0xAAAA_AAAA)
//!
//! Polling replaced by yield-and-retry loops using sys_yield() + sys_state_restore()
//! and sys_query_hotswap_ready().  The timeouts are calibrated to scheduler tick rate
//! (one tick ≈ 10 ms) — HOTSWAP_MAX_ITERS×YIELD_COST_TICKS ≈ 5 seconds.

extern crate alloc;

use crate::error::HotswapError;
use ostd::syscall::{
    sys_freeze_cell, sys_kill_cell, sys_lookup_service, sys_query_hotswap_ready,
    sys_register_service, sys_resume_cell, sys_send, sys_spawn_from_path, sys_state_restore,
    sys_state_stash_clear, sys_yield,
};

/// Maximum poll iterations while waiting for stash/ready (≈ 5 s at 10 ms/tick).
const MAX_ITERS: u32 = 500;

// ── IPC envelope byte constants (must match kernel hotswap.rs) ───────────────

const APP_MSG_MAGIC: u8 = 0xAC;
const DISC_SNAPSHOT: u8 = 0xF0; // AppEvent::Snapshot { swap_id }
const DISC_RESTORE: u8 = 0xF1; // AppEvent::Restore  { key[64] }

// ── Hotswap stash key (must match ostd::hotswap::hotswap_key) ────────────────

fn stash_key_for(swap_id: u64) -> u64 {
    0x_A3_0000_0000_0000_u64 | (swap_id & 0xFFFF_FFFF_FFFF)
}

// ── Decimal formatter (no std) ────────────────────────────────────────────────

fn fmt_u64_decimal(mut val: u64, buf: &mut [u8; 20]) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut end = 20usize;
    while val > 0 {
        end -= 1;
        buf[end] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    let len = 20 - end;
    buf.copy_within(end..20, 0);
    len
}

// ── Poll helpers ──────────────────────────────────────────────────────────────

/// Poll until `sys_state_restore(key)` returns > 0, yielding between iterations.
///
/// Returns `Ok(())` when the stash entry appears.
/// Returns `Err(SnapshotTimeout)` after `MAX_ITERS` yields without success.
///
/// A stash miss is tolerated (cell that doesn't implement ViStateTransfer never
/// stashes anything); the hotswap continues with an empty stash.
fn wait_for_stash_key(key: u64) -> Result<(), HotswapError> {
    let mut probe = [0u8; 1];
    for _ in 0..MAX_ITERS {
        if sys_state_restore(key, &mut probe) > 0 {
            return Ok(());
        }
        sys_yield();
    }
    Err(HotswapError::SnapshotTimeout)
}

/// Poll until the new cell has called `sys_hotswap_ready()`, yielding between
/// iterations.
///
/// Returns `Ok(())` when the flag is set, `Err(ReadyTimeout)` on timeout.
fn wait_for_hotswap_ready(new_tid: usize) -> Result<(), HotswapError> {
    for _ in 0..MAX_ITERS {
        match sys_query_hotswap_ready(new_tid) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => break, // tid vanished — treat as timeout
        }
        sys_yield();
    }
    Err(HotswapError::ReadyTimeout)
}

// ── IPC senders ───────────────────────────────────────────────────────────────

/// Send `AppEvent::Snapshot { swap_id }` to `tid`.
fn send_snapshot_event(tid: usize, swap_id: u64) -> Result<(), HotswapError> {
    let mut buf = [0u8; 10];
    buf[0] = APP_MSG_MAGIC;
    buf[1] = DISC_SNAPSHOT;
    buf[2..10].copy_from_slice(&swap_id.to_le_bytes());
    match sys_send(tid, &buf) {
        ostd::syscall::SyscallResult::Ok(_) => Ok(()),
        ostd::syscall::SyscallResult::Err(_) => Err(HotswapError::SnapshotIpcFailed),
    }
}

/// Send `AppEvent::Restore { key }` to `tid`.
fn send_restore_event(tid: usize, swap_id: u64) -> Result<(), HotswapError> {
    let mut buf = [0u8; 66];
    buf[0] = APP_MSG_MAGIC;
    buf[1] = DISC_RESTORE;
    let mut tmp = [0u8; 20];
    let n = fmt_u64_decimal(swap_id, &mut tmp);
    let key_len = n.min(63);
    buf[2..2 + key_len].copy_from_slice(&tmp[..key_len]);
    match sys_send(tid, &buf) {
        ostd::syscall::SyscallResult::Ok(_) => Ok(()),
        ostd::syscall::SyscallResult::Err(_) => Err(HotswapError::RestoreIpcFailed),
    }
}

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
/// 6. UNFREEZE + RE-REGISTER — kill old cell, register new service tid
pub fn hotswap(service_id: u16, new_elf_path: &str) -> Result<usize, HotswapError> {
    // ── Resolve target tid ────────────────────────────────────────────────────
    let old_tid = sys_lookup_service(service_id).ok_or(HotswapError::ServiceNotFound)?;

    let swap_id = next_swap_id();
    let stash_key = stash_key_for(swap_id);

    // ── Step 1a: FREEZE (soft) ────────────────────────────────────────────────
    // The old cell must still run to receive the Snapshot IPC.  We freeze the
    // service registry entry so new callers retry during the swap window.
    sys_freeze_cell(old_tid).map_err(|_| HotswapError::FreezeFailed)?;

    // ── Step 2: SERIALIZE ─────────────────────────────────────────────────────
    if let Err(e) = send_snapshot_event(old_tid, swap_id) {
        sys_resume_cell(old_tid).ok(); // roll back
        return Err(e);
    }

    // Wait for old cell to stash state.  Timeout is non-fatal — cells that
    // don't implement ViStateTransfer never stash; we continue with empty stash.
    match wait_for_stash_key(stash_key) {
        Ok(()) | Err(HotswapError::SnapshotTimeout) => {}
        Err(e) => {
            sys_resume_cell(old_tid).ok();
            return Err(e);
        }
    }

    // ── Step 3: SPAWN ─────────────────────────────────────────────────────────
    let new_tid = {
        let result = sys_spawn_from_path(new_elf_path);
        match result {
            ostd::syscall::SyscallResult::Ok(tid) => tid as usize,
            ostd::syscall::SyscallResult::Err(_) => {
                sys_resume_cell(old_tid).ok();
                return Err(HotswapError::SpawnFailed);
            }
        }
    };

    // ── Step 4: DESERIALIZE ───────────────────────────────────────────────────
    if let Err(e) = send_restore_event(new_tid, swap_id) {
        // Old cell stays frozen — split-brain risk; do NOT resume two instances.
        return Err(e);
    }

    if let Err(e) = wait_for_hotswap_ready(new_tid) {
        return Err(e);
    }

    // ── Step 5: COMMIT ────────────────────────────────────────────────────────
    // Re-register the service registry entry with the new tid.
    // Note: Supervisor must hold SpawnCap to call sys_register_service.
    let _ = sys_register_service(service_id, new_tid);

    // Terminate the old cell (it is Frozen at this point — KillCell bypasses the
    // Frozen kill-guard in the kernel, same as exit_task_internal in hotswap.rs).
    sys_kill_cell(old_tid, 0xAAAA_AAAA_u32).ok();

    // Clean up stash slot.
    sys_state_stash_clear(stash_key);

    Ok(new_tid)
}
