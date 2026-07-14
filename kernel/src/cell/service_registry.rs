//! Kernel Service Registry — stable `service_id → current provider tid` mapping.
//!
//! The supervisor (init) respawns a dead service under a NEW tid. Clients that
//! addressed the service by its old tid would break. This registry adds an
//! indirection: the supervisor registers each service's tid under a well-known
//! `service_id` ([`api::syscall::service`]) and re-registers the new tid on
//! respawn; a client resolves `service_id → tid` right before sending, so it
//! reconnects transparently. Keeping the map in the kernel (the never-die core)
//! means it survives any service's death, and a dead provider is auto-cleared
//! ([`clear_tid`]) so a lookup in the death→respawn window returns "none" (the
//! client retries) instead of a stale tid.
//!
//! Only `SpawnCap` holders may `register` (enforced at the syscall dispatch),
//! so a cell cannot hijack, e.g., the VFS endpoint — the trusted supervisor owns
//! the namespace. `lookup` is open to all cells.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;

/// Upper bound on distinct registered services. Bounds kernel memory and matches
/// the small, fixed set of well-known service IDs — a runaway registrar cannot
/// grow the map without bound.
pub const MAX_SERVICES: usize = 32;

/// `service_id` → current provider task id. `0` is never stored (it is the ABI
/// "no provider" sentinel returned by `lookup`).
static REGISTRY: Spinlock<BTreeMap<u16, usize>> = Spinlock::new(BTreeMap::new());

/// Force-release this module's lock during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    REGISTRY.force_unlock();
}

/// Register `tid` as the current provider of `service_id`, replacing any prior
/// entry. Returns `false` (rejected) if the registry is full and `service_id` is
/// new, or if `tid` is 0 (the reserved "none" sentinel). The SpawnCap authority
/// check is performed by the caller (syscall dispatch), not here.
pub fn register(service_id: u16, tid: usize) -> bool {
    if tid == 0 {
        return false;
    }
    let mut map = REGISTRY.lock();
    if map.len() >= MAX_SERVICES && !map.contains_key(&service_id) {
        log::warn!(
            "[service-registry] full ({} entries); rejecting id {}",
            MAX_SERVICES,
            service_id
        );
        return false;
    }
    map.insert(service_id, tid);
    log::info!("[service-registry] {} -> tid {}", service_id, tid);
    true
}

/// Resolve `service_id` to its current provider tid, or `None` if no live
/// provider is registered. The syscall layer maps `None` to the ABI value 0.
pub fn lookup(service_id: u16) -> Option<usize> {
    REGISTRY.lock().get(&service_id).copied()
}

/// Remove every registration that points at `tid`. Called from `exit_task` when a
/// task dies so a client never resolves a service to a dead provider; the
/// supervisor re-registers the replacement's tid on respawn.
pub fn clear_tid(tid: usize) {
    let mut map = REGISTRY.lock();
    let before = map.len();
    map.retain(|_, &mut t| t != tid);
    if map.len() != before {
        log::info!(
            "[service-registry] cleared stale entries for dead tid {}",
            tid
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_lookup() {
        assert!(register(api::syscall::service::VFS, 4));
        assert_eq!(lookup(api::syscall::service::VFS), Some(4));
    }

    #[test]
    fn reject_zero_tid() {
        assert!(!register(api::syscall::service::NET, 0));
        assert_eq!(lookup(api::syscall::service::NET), None);
    }

    #[test]
    fn reregister_updates_tid() {
        register(api::syscall::service::INPUT, 7);
        register(api::syscall::service::INPUT, 9);
        assert_eq!(lookup(api::syscall::service::INPUT), Some(9));
    }

    #[test]
    fn clear_tid_removes_dead_provider() {
        register(api::syscall::service::CONFIG, 12);
        clear_tid(12);
        assert_eq!(lookup(api::syscall::service::CONFIG), None);
    }
}
