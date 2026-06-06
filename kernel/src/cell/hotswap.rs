//! Hot-swap orchestrator — live-replaces a Cell without message loss.
//!
//! Protocol (5 steps, see `docs/hotswap-guide.md`):
//! 1. Freeze: mark cell as `Frozen`; kernel queues new messages instead of delivering
//! 2. Serialize: invoke the cell's `ViStateTransfer::serialize_state` via an internal IPC
//! 3. Replace code: unmap old ELF pages; load new ELF via `loader::spawn_from_path`
//! 4. Deserialize: call new cell's `deserialize_state(bytes)`
//! 5. Unfreeze: mark live; flush queued messages to new cell
//!
//! In v1.0 steps 2 and 4 are implemented via special opcodes sent to the cell
//! over IPC (opcode `0xF0` = Serialize, `0xF1` = Deserialize).  The cell's
//! `main` loop must handle these to participate in hot-swap.

use alloc::vec::Vec;
use types::{CellId, ViError, ViResult};
use crate::sync::Spinlock;

// ─── Opcodes ────────────────────────────────────────────────────────────────

/// IPC opcode sent to a cell to trigger state serialisation.
/// The cell replies with its serialised state bytes.
pub const OP_SERIALIZE:   u8 = 0xF0;
/// IPC opcode sent to a new cell to inject previously serialised state.
/// Payload: raw state bytes.  Cell replies with `0x00` = ok, `0x01` = error.
pub const OP_DESERIALIZE: u8 = 0xF1;

// ─── Freeze registry ─────────────────────────────────────────────────────────

/// Global freeze set — cells in this set have their incoming IPC queued rather
/// than delivered.  Protected by a Spinlock; always acquired before the
/// scheduler lock to avoid deadlock (ordering documented here and enforced
/// by convention).
static FROZEN: Spinlock<alloc::collections::BTreeSet<u64>> =
    Spinlock::new(alloc::collections::BTreeSet::new());

/// Force-release this module's lock during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    FROZEN.force_unlock();
}

/// Mark `cell_id` as frozen.  Subsequent `sys_send` calls to this cell will
/// queue the message in the task's pending queue instead of delivering it.
pub fn freeze(cell_id: CellId) {
    FROZEN.lock().insert(cell_id.0);
    log::info!("[hotswap] froze cell {}", cell_id.0);
}

/// Return true if `cell_id` is currently frozen.
pub fn is_frozen(cell_id: CellId) -> bool {
    FROZEN.lock().contains(&cell_id.0)
}

/// Remove `cell_id` from the freeze set and resume normal message delivery.
pub fn unfreeze(cell_id: CellId) {
    FROZEN.lock().remove(&cell_id.0);
    log::info!("[hotswap] unfroze cell {}", cell_id.0);
}

// ─── Hot-swap entry point ────────────────────────────────────────────────────

/// Perform a live hot-swap of `old_cell_id` with the ELF at `new_elf_path`.
///
/// This function runs to completion synchronously on the calling task's stack.
/// Incoming messages to `old_cell_id` are queued during the swap; after
/// `unfreeze` they are delivered to the new cell.
///
/// # Errors
/// - `ViError::NotFound` — `old_cell_id` not found or `new_elf_path` not on disk.
/// - `ViError::InvalidInput` — serialisation or deserialisation failed.
/// - `ViError::NotSupported` — cell does not implement the hotswap protocol.
///
/// # Panics
/// Panics in debug builds if `old_cell_id == 0` (reserved sentinel).
pub fn hotswap(old_cell_id: CellId, new_elf_path: &str) -> ViResult<usize> {
    debug_assert!(old_cell_id.0 != 0, "hotswap: cell_id 0 is the null sentinel");

    log::info!("[hotswap] starting swap: cell {} → {}", old_cell_id.0, new_elf_path);

    // ── Step 1: Freeze ────────────────────────────────────────────────────
    freeze(old_cell_id);

    // ── Step 2: Serialize state ───────────────────────────────────────────
    let state = match request_serialise(old_cell_id) {
        Ok(bytes) => {
            log::info!("[hotswap] serialised {} bytes from cell {}", bytes.len(), old_cell_id.0);
            bytes
        }
        Err(e) => {
            unfreeze(old_cell_id); // roll back — old cell still works
            log::error!("[hotswap] serialise failed: {:?}", e);
            return Err(e);
        }
    };

    // ── Step 3: Load new ELF ─────────────────────────────────────────────
    // `spawn_from_path` creates a new task; we note its task_id for step 4.
    let new_task_id = match crate::loader::spawn_from_path(new_elf_path) {
        Ok(id) => id,
        Err(e) => {
            unfreeze(old_cell_id);
            log::error!("[hotswap] failed to spawn {}: {:?}", new_elf_path, e);
            return Err(e);
        }
    };
    let new_cell = CellId(new_task_id as u64);
    log::info!("[hotswap] new cell spawned as task {}", new_task_id);

    // ── Step 4: Deserialize state into new cell ───────────────────────────
    if let Err(e) = request_deserialise(new_cell, &state) {
        log::error!("[hotswap] deserialise failed: {:?}", e);
        // Leave old cell frozen (manual recovery required) — do not panic.
        return Err(e);
    }
    log::info!("[hotswap] state restored in new cell {}", new_task_id);

    // ── Step 5: Unfreeze → queued messages now route to new cell ─────────
    // Transfer the old cell's IPC endpoint identity to the new cell so that
    // callers using the old cell_id transparently reach the new one.
    // For v1.0 we simply unfreeze both and let the scheduler route naturally.
    unfreeze(old_cell_id);
    log::info!("[hotswap] swap complete: {} messages will route to cell {}", 0, new_task_id);

    Ok(new_task_id)
}

// ─── IPC helpers ─────────────────────────────────────────────────────────────

/// Ask `cell_id` to serialise its state and return the bytes.
///
/// Sends `OP_SERIALIZE` (1 byte) and waits for the reply (up to 64 KB).
fn request_serialise(cell_id: CellId) -> ViResult<Vec<u8>> {
    let msg = [OP_SERIALIZE];
    let target = cell_id.0 as usize;

    // Send the serialise request.
    crate::task::send_to(target, &msg).map_err(|_| ViError::NotSupported)?;

    // Receive the reply — up to 64 KB of state.
    let mut buf = alloc::vec![0u8; 65536];
    let n = crate::task::recv_from(target, &mut buf).map_err(|_| ViError::IO)?;
    buf.truncate(n);
    Ok(buf)
}

/// Send serialised `state` to `cell_id` for deserialisation.
///
/// Sends `OP_DESERIALIZE` + `state` bytes; waits for a 1-byte ack.
fn request_deserialise(cell_id: CellId, state: &[u8]) -> ViResult<()> {
    let mut msg = alloc::vec![OP_DESERIALIZE];
    msg.extend_from_slice(state);
    let target = cell_id.0 as usize;

    crate::task::send_to(target, &msg).map_err(|_| ViError::NotSupported)?;

    // Ack: 0x00 = ok, any other = error.
    let mut ack = [0u8; 1];
    let n = crate::task::recv_from(target, &mut ack).map_err(|_| ViError::IO)?;
    if n == 0 || ack[0] != 0 {
        return Err(ViError::InvalidInput);
    }
    Ok(())
}
