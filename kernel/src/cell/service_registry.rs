//! Kernel Service Registry — stable `service_id → current provider` mapping.
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
//! Each entry also records the provider's **Cell identity** — `(cell_id,
//! generation)`. A tid says where to send; it is not an identity, because a
//! `CellId` slot may be reused later while the generation cannot. Recording the
//! identity here is what lets `LookupServiceBound` answer with a binding instead
//! of a bare tid, and it is the same axis [`api::caller_identity::CallerIdentity`]
//! and [`api::cell_owner::CellOwner`] already use — one identity concept, not two.
//! A provider whose recorded identity is not live (`cell_id == 0` or
//! `generation == 0`) is **not bindable** and is reported as "no live provider",
//! so the kernel never states a binding it cannot stand behind.
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

/// `service_id` → current provider. `0` is never stored as a tid (it is the ABI
/// "no provider" sentinel returned by `lookup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceEntry {
    Active {
        tid: usize,
        cell_id: u64,
        generation: u64,
    },
    Paused {
        tid: usize,
        cell_id: u64,
        generation: u64,
    },
}

impl ServiceEntry {
    fn tid(self) -> usize {
        match self {
            ServiceEntry::Active { tid, .. } | ServiceEntry::Paused { tid, .. } => tid,
        }
    }

    /// The provider's Cell identity, or `None` when it names no live Cell.
    ///
    /// `cell_id == 0` is the kernel's own allocation rather than any Cell, and a
    /// zero generation is not an epoch. Either one means the entry cannot support
    /// a binding, so `LookupServiceBound` treats it exactly like an absent provider.
    fn identity(self) -> Option<(u64, u64)> {
        let (ServiceEntry::Active {
            cell_id, generation, ..
        }
        | ServiceEntry::Paused {
            cell_id, generation, ..
        }) = self;
        (cell_id != 0 && generation != 0).then_some((cell_id, generation))
    }
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
///
/// `cell_id`/`generation` are the provider's live identity, read from the
/// scheduler by the caller. They are recorded, never invented here.
pub fn register(service_id: u16, tid: usize, cell_id: u64, generation: u64) -> bool {
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
    map.insert(
        service_id,
        ServiceEntry::Active {
            tid,
            cell_id,
            generation,
        },
    );
    log::info!(
        "[service-registry] {} -> tid {} (cell {} gen {})",
        service_id,
        tid,
        cell_id,
        generation
    );
    true
}

/// Resolve `service_id` to its current provider tid, or `None` if no live
/// provider is registered. The syscall layer maps `None` to the ABI value 0.
pub fn lookup(service_id: u16) -> Option<usize> {
    match REGISTRY.lock().get(&service_id).copied() {
        Some(ServiceEntry::Active { tid, .. }) => Some(tid),
        Some(ServiceEntry::Paused { .. }) | None => None,
    }
}

/// Resolve `service_id` to its current provider's full binding
/// `(tid, cell_id, generation)`.
///
/// Returns `None` for an absent provider, a paused provider (the hot-swap quiesce
/// barrier must stay in force for new work) and a provider whose recorded identity
/// is not live. All three are "no live binding", and the syscall layer maps them to
/// the ABI value 0 exactly like [`lookup`].
pub fn lookup_bound(service_id: u16) -> Option<(usize, u64, u64)> {
    let entry = REGISTRY.lock().get(&service_id).copied()?;
    if !matches!(entry, ServiceEntry::Active { .. }) {
        return None;
    }
    let (cell_id, generation) = entry.identity()?;
    Some((entry.tid(), cell_id, generation))
}

/// Hide a service from new lookups while its current provider remains runnable.
///
/// The compare-and-pause contract prevents a stale supervisor request from
/// pausing a replacement that another recovery path already registered.
pub fn pause(service_id: u16, expected_tid: usize) -> bool {
    let mut map = REGISTRY.lock();
    match map.get(&service_id).copied() {
        // Pausing preserves the recorded identity verbatim: the pause is a
        // visibility change, not a re-registration, and a provider's identity does
        // not change while it is being quiesced.
        Some(ServiceEntry::Active {
            tid,
            cell_id,
            generation,
        }) if tid == expected_tid => {
            map.insert(
                service_id,
                ServiceEntry::Paused {
                    tid: expected_tid,
                    cell_id,
                    generation,
                },
            );
            true
        }
        Some(ServiceEntry::Paused { tid, .. }) if tid == expected_tid => true,
        _ => false,
    }
}

/// Publish `new_tid` only when `service_id` is still paused at `old_tid`.
///
/// The hot-swap barrier calls this while holding `SCHEDULER`, preserving the
/// global `SCHEDULER -> service registry` lock order. `cell_id`/`generation` are
/// the replacement's live identity, read from the scheduler by that caller.
pub(crate) fn commit_paused(
    service_id: u16,
    old_tid: usize,
    new_tid: usize,
    cell_id: u64,
    generation: u64,
) -> bool {
    if new_tid == 0 {
        return false;
    }
    let mut map = REGISTRY.lock();
    match map.get(&service_id).copied() {
        Some(ServiceEntry::Paused { tid, .. }) if tid == old_tid => {
            map.insert(
                service_id,
                ServiceEntry::Active {
                    tid: new_tid,
                    cell_id,
                    generation,
                },
            );
            true
        }
        _ => false,
    }
}

/// Check the exact paused provider without exposing registry representation.
pub(crate) fn paused_matches(service_id: u16, expected_tid: usize) -> bool {
    matches!(
        REGISTRY.lock().get(&service_id).copied(),
        Some(ServiceEntry::Paused { tid, .. }) if tid == expected_tid
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
        .any(|entry| matches!(entry, ServiceEntry::Paused { tid: provider, .. } if *provider == tid))
}

/// Return whether `tid` is the active or paused provider of any trusted
/// supervisor-owned service registration.
pub fn is_registered_tid(tid: usize) -> bool {
    REGISTRY
        .lock()
        .values()
        .any(|entry| matches!(entry, ServiceEntry::Active { .. } | ServiceEntry::Paused { .. } if entry.tid() == tid))
}

/// Remove every registration that points at `tid`. Called from `exit_task` when a
/// task dies so a client never resolves a service to a dead provider; the
/// supervisor re-registers the replacement's tid on respawn.
pub fn clear_tid(tid: usize) {
    let mut map = REGISTRY.lock();
    let before = map.len();
    map.retain(|_, entry| entry.tid() != tid);
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
            ServiceEntry::Active { tid, .. } => (*service_id, *tid, false),
            ServiceEntry::Paused { tid, .. } => (*service_id, *tid, true),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: u64 = 7;
    const GEN: u64 = 3;

    #[test]
    fn register_then_lookup() {
        assert!(register(api::syscall::service::VFS, 4, CELL, GEN));
        assert_eq!(lookup(api::syscall::service::VFS), Some(4));
    }

    #[test]
    fn reject_zero_tid() {
        assert!(!register(api::syscall::service::NET, 0, CELL, GEN));
        assert_eq!(lookup(api::syscall::service::NET), None);
    }

    #[test]
    fn reregister_updates_tid() {
        register(api::syscall::service::INPUT, 7, CELL, GEN);
        register(api::syscall::service::INPUT, 9, CELL + 1, GEN + 1);
        assert_eq!(lookup(api::syscall::service::INPUT), Some(9));
    }

    #[test]
    fn clear_tid_removes_dead_provider() {
        register(api::syscall::service::CONFIG, 12, CELL, GEN);
        clear_tid(12);
        assert_eq!(lookup(api::syscall::service::CONFIG), None);
    }

    #[test]
    fn pause_hides_only_the_expected_provider() {
        const TEST_SERVICE: u16 = 60_000;
        register(TEST_SERVICE, 21, CELL, GEN);
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
        register(TEST_SERVICE, 31, CELL, GEN);
        assert!(pause(TEST_SERVICE, 31));
        assert!(register(TEST_SERVICE, 31, CELL, GEN));
        assert_eq!(lookup(TEST_SERVICE), Some(31));
    }

    #[test]
    fn commit_requires_exact_paused_provider() {
        const TEST_SERVICE: u16 = 60_002;
        register(TEST_SERVICE, 41, CELL, GEN);
        assert!(!commit_paused(TEST_SERVICE, 40, 42, CELL, GEN));
        assert_eq!(lookup(TEST_SERVICE), Some(41));
        assert!(pause(TEST_SERVICE, 41));
        assert!(!commit_paused(TEST_SERVICE, 40, 42, CELL, GEN));
        assert!(commit_paused(TEST_SERVICE, 41, 42, CELL, GEN + 1));
        assert_eq!(lookup(TEST_SERVICE), Some(42));
        // The replacement's identity is the one that was committed, not the source's.
        assert_eq!(lookup_bound(TEST_SERVICE), Some((42, CELL, GEN + 1)));
    }

    #[test]
    fn lookup_bound_reports_the_recorded_identity() {
        const TEST_SERVICE: u16 = 60_003;
        register(TEST_SERVICE, 51, CELL, GEN);
        assert_eq!(lookup_bound(TEST_SERVICE), Some((51, CELL, GEN)));

        // Re-registering a new incarnation replaces both tid and identity, so a
        // holder of the old binding can tell the provider changed.
        register(TEST_SERVICE, 52, CELL, GEN + 1);
        assert_eq!(lookup_bound(TEST_SERVICE), Some((52, CELL, GEN + 1)));
    }

    #[test]
    fn lookup_bound_denies_absent_paused_and_identityless_providers() {
        const ABSENT: u16 = 60_004;
        const PAUSED: u16 = 60_005;
        const IDENTITYLESS: u16 = 60_006;

        assert_eq!(lookup_bound(ABSENT), None);

        register(PAUSED, 61, CELL, GEN);
        assert!(pause(PAUSED, 61));
        // A paused provider must not hand out a binding: the quiesce barrier holds.
        assert_eq!(lookup_bound(PAUSED), None);

        // A provider with no Cell identity (kernel-owned task) is not bindable, and
        // the kernel must not state a binding it cannot stand behind.
        register(IDENTITYLESS, 71, 0, GEN);
        assert_eq!(lookup(IDENTITYLESS), Some(71));
        assert_eq!(lookup_bound(IDENTITYLESS), None);
        register(IDENTITYLESS, 72, CELL, 0);
        assert_eq!(lookup_bound(IDENTITYLESS), None);
    }

    #[test]
    fn clear_tid_removes_paused_and_active_entries() {
        const ACTIVE: u16 = 60_007;
        const PAUSED: u16 = 60_008;
        register(ACTIVE, 81, CELL, GEN);
        register(PAUSED, 82, CELL, GEN);
        assert!(pause(PAUSED, 82));

        clear_tid(81);
        clear_tid(82);
        assert_eq!(lookup_bound(ACTIVE), None);
        assert_eq!(lookup_bound(PAUSED), None);
        assert!(!is_paused_tid(82));
    }
}
