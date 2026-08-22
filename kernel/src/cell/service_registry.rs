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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceEntry {
    Active(usize),
    Paused(usize),
}

static REGISTRY: Spinlock<BTreeMap<u16, ServiceEntry>> = Spinlock::new(BTreeMap::new());

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
    map.insert(service_id, ServiceEntry::Active(tid));
    log::info!("[service-registry] {} -> tid {}", service_id, tid);
    true
}

/// Resolve `service_id` to its current provider tid, or `None` if no live
/// provider is registered. The syscall layer maps `None` to the ABI value 0.
pub fn lookup(service_id: u16) -> Option<usize> {
    match REGISTRY.lock().get(&service_id).copied() {
        Some(ServiceEntry::Active(tid)) => Some(tid),
        Some(ServiceEntry::Paused(_)) | None => None,
    }
}

/// Hide a service from new lookups while its current provider remains runnable.
///
/// The compare-and-pause contract prevents a stale supervisor request from
/// pausing a replacement that another recovery path already registered.
pub fn pause(service_id: u16, expected_tid: usize) -> bool {
    let mut map = REGISTRY.lock();
    match map.get(&service_id).copied() {
        Some(ServiceEntry::Active(tid)) if tid == expected_tid => {
            map.insert(service_id, ServiceEntry::Paused(expected_tid));
            true
        }
        Some(ServiceEntry::Paused(tid)) if tid == expected_tid => true,
        _ => false,
    }
}

/// Publish `new_tid` only when `service_id` is still paused at `old_tid`.
///
/// The hot-swap barrier calls this while holding `SCHEDULER`, preserving the
/// global `SCHEDULER -> service registry` lock order.
pub(crate) fn commit_paused(service_id: u16, old_tid: usize, new_tid: usize) -> bool {
    if new_tid == 0 {
        return false;
    }
    let mut map = REGISTRY.lock();
    match map.get(&service_id).copied() {
        Some(ServiceEntry::Paused(tid)) if tid == old_tid => {
            map.insert(service_id, ServiceEntry::Active(new_tid));
            true
        }
        _ => false,
    }
}

/// Check the exact paused provider without exposing registry representation.
pub(crate) fn paused_matches(service_id: u16, expected_tid: usize) -> bool {
    matches!(
        REGISTRY.lock().get(&service_id).copied(),
        Some(ServiceEntry::Paused(tid)) if tid == expected_tid
    )
}

/// Return whether `tid` is hidden behind any paused service mapping.
///
/// IPC admission uses this as the quiesce barrier for callers that cached the
/// provider tid before the mapping was paused.
pub fn is_paused_tid(tid: usize) -> bool {
    REGISTRY
        .lock()
        .values()
        .any(|entry| matches!(entry, ServiceEntry::Paused(provider) if *provider == tid))
}

/// Remove every registration that points at `tid`. Called from `exit_task` when a
/// task dies so a client never resolves a service to a dead provider; the
/// supervisor re-registers the replacement's tid on respawn.
pub fn clear_tid(tid: usize) {
    let mut map = REGISTRY.lock();
    let before = map.len();
    map.retain(|_, entry| match entry {
        ServiceEntry::Active(provider) | ServiceEntry::Paused(provider) => *provider != tid,
    });
    if map.len() != before {
        log::info!(
            "[service-registry] cleared stale entries for dead tid {}",
            tid
        );
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn snapshot() -> alloc::vec::Vec<(u16, usize, bool)> {
    REGISTRY
        .lock()
        .iter()
        .map(|(service_id, entry)| match entry {
            ServiceEntry::Active(tid) => (*service_id, *tid, false),
            ServiceEntry::Paused(tid) => (*service_id, *tid, true),
        })
        .collect()
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

    #[test]
    fn pause_hides_only_the_expected_provider() {
        const TEST_SERVICE: u16 = 60_000;
        register(TEST_SERVICE, 21);
        assert!(!pause(TEST_SERVICE, 20));
        assert_eq!(lookup(TEST_SERVICE), Some(21));

        assert!(pause(TEST_SERVICE, 21));
        assert_eq!(lookup(TEST_SERVICE), None);
        assert!(pause(TEST_SERVICE, 21));
        assert!(is_paused_tid(21));
    }

    #[test]
    fn register_reactivates_a_paused_service() {
        const TEST_SERVICE: u16 = 60_001;
        register(TEST_SERVICE, 31);
        assert!(pause(TEST_SERVICE, 31));
        assert!(register(TEST_SERVICE, 31));
        assert_eq!(lookup(TEST_SERVICE), Some(31));
    }

    #[test]
    fn commit_requires_exact_paused_provider() {
        const TEST_SERVICE: u16 = 60_002;
        register(TEST_SERVICE, 41);
        assert!(!commit_paused(TEST_SERVICE, 40, 42));
        assert_eq!(lookup(TEST_SERVICE), Some(41));
        assert!(pause(TEST_SERVICE, 41));
        assert!(!commit_paused(TEST_SERVICE, 40, 42));
        assert!(commit_paused(TEST_SERVICE, 41, 42));
        assert_eq!(lookup(TEST_SERVICE), Some(42));
    }
}
