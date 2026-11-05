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
//! Publishing is owner-bound ([`register_as`]): the well-known ids below
//! [`PRIVATE_ID_BASE`] belong to the namespace owners (trusted init and
//! `SupervisorCap` holders), so a cell that merely holds `SpawnCap` — e.g. a USB
//! host driver that spawns its protocol child — cannot rebind VFS, NET, KMS or
//! SUPERVISOR. A spawn-only cell may publish a private id, and the first such
//! registrar owns it until it exits. `lookup` is open to all cells.

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
            cell_id,
            generation,
            ..
        }
        | ServiceEntry::Paused {
            cell_id,
            generation,
            ..
        }) = self;
        (cell_id != 0 && generation != 0).then_some((cell_id, generation))
    }
}

static REGISTRY: Spinlock<BTreeMap<u16, ServiceEntry>> = Spinlock::new(BTreeMap::new());

/// Private id → tid of the spawn-only registrar that owns it. Lock order:
/// `OWNERS → REGISTRY`; nothing takes `OWNERS` while holding `REGISTRY`.
static OWNERS: Spinlock<BTreeMap<u16, usize>> = Spinlock::new(BTreeMap::new());

/// Ids below this are the well-known namespace of [`api::syscall::service`];
/// ids at or above it are private ids a lifecycle parent publishes for its own
/// children (the demo and test fixtures use `0x7Axx`).
pub const PRIVATE_ID_BASE: u16 = 0x100;

/// Who asks to publish a service id, as established by the syscall dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registrar {
    /// Trusted init or a `SupervisorCap` holder: owns the whole namespace and
    /// rebinds any id (respawn, hot-swap rollback).
    Namespace,
    /// A `SpawnCap`-only task, by tid: private ids only, first registrar owns.
    Spawner(usize),
}

/// Why [`register_as`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The id is well-known or owned by another live registrar.
    NotOwner,
    /// Zero tid, or the registry/owner table is full.
    Rejected,
}

/// Force-release this module's locks during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    OWNERS.force_unlock();
    REGISTRY.force_unlock();
}

/// Publish `tid` as the provider of `service_id` on behalf of `registrar`.
///
/// A [`Registrar::Spawner`] may publish only a private id, and only one that no
/// other live spawner owns; its first successful publish makes it the owner
/// until it exits ([`clear_tid`]), so it can rebind its own restarted child.
pub fn register_as(
    registrar: Registrar,
    service_id: u16,
    tid: usize,
    cell_id: u64,
    generation: u64,
) -> Result<(), Refusal> {
    let Registrar::Spawner(spawner) = registrar else {
        return register(service_id, tid, cell_id, generation)
            .then_some(())
            .ok_or(Refusal::Rejected);
    };
    if service_id < PRIVATE_ID_BASE {
        return Err(Refusal::NotOwner);
    }
    let mut owners = OWNERS.lock();
    match owners.get(&service_id) {
        Some(&owner) if owner != spawner => return Err(Refusal::NotOwner),
        None if owners.len() >= MAX_SERVICES => return Err(Refusal::Rejected),
        _ => {}
    }
    if !register(service_id, tid, cell_id, generation) {
        return Err(Refusal::Rejected);
    }
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::SERVICE_SHARED,
    );
    owners.insert(service_id, spawner);
    Ok(())
}

/// Register `tid` as the current provider of `service_id`, replacing any prior
/// entry. Returns `false` (rejected) if the registry is full and `service_id` is
/// new, or if `tid` is 0 (the reserved "none" sentinel). No authority check: the
/// generic `RegisterService` path uses [`register_as`]; the kernel-defined
/// self-registration paths (GPU driver, Silo, demo) check their own authority.
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
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::SERVICE_SHARED,
    );
    map.insert(
        service_id,
        ServiceEntry::Active {
            tid,
            cell_id,
            generation,
        },
    );
    #[cfg(cellos_p01_trace)]
    drop(_semantic);
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
    REGISTRY.lock().values().any(
        |entry| matches!(entry, ServiceEntry::Paused { tid: provider, .. } if *provider == tid),
    )
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
/// supervisor re-registers the replacement's tid on respawn. A dead registrar
/// also gives up its private ids, so its restarted successor can claim them.
pub fn clear_tid(tid: usize) {
    {
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
    OWNERS.lock().retain(|_, owner| *owner != tid);
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

    /// H-11: a spawn-only cell cannot bind a well-known id (here SUPERVISOR and
    /// KMS), while the namespace owner rebinds it freely.
    #[test]
    fn spawner_cannot_bind_well_known_ids() {
        use api::syscall::service::{KMS, SUPERVISOR};
        const HIJACKER: usize = 0x5_11A0;
        for id in [SUPERVISOR, KMS] {
            assert_eq!(
                register_as(Registrar::Spawner(HIJACKER), id, HIJACKER, CELL, GEN),
                Err(Refusal::NotOwner)
            );
            assert_ne!(lookup(id), Some(HIJACKER));
        }
        assert_eq!(
            register_as(Registrar::Namespace, KMS, 0x5_11A1, CELL, GEN),
            Ok(())
        );
        assert_eq!(lookup(KMS), Some(0x5_11A1));
        clear_tid(0x5_11A1);
    }

    /// A private id belongs to its first spawn-only registrar: it rebinds its
    /// restarted child, another spawner is refused, and the owner's exit frees
    /// the id for its own restarted successor.
    #[test]
    fn private_id_is_owned_by_its_first_registrar_until_it_exits() {
        const ID: u16 = 0x7A0B;
        const OWNER: usize = 0x5_11B0;
        const OTHER: usize = 0x5_11B1;
        const SUCCESSOR: usize = 0x5_11B2;
        let spawner = Registrar::Spawner;

        assert_eq!(register_as(spawner(OWNER), ID, 0x5_11C0, CELL, GEN), Ok(()));
        assert_eq!(
            register_as(spawner(OWNER), ID, 0x5_11C1, CELL, GEN + 1),
            Ok(())
        );
        assert_eq!(lookup(ID), Some(0x5_11C1));
        assert_eq!(
            register_as(spawner(OTHER), ID, OTHER, CELL, GEN),
            Err(Refusal::NotOwner)
        );
        assert_eq!(lookup(ID), Some(0x5_11C1));
        assert_eq!(
            register_as(spawner(OWNER), ID, 0, CELL, GEN),
            Err(Refusal::Rejected)
        );

        clear_tid(OWNER);
        assert_eq!(
            register_as(spawner(SUCCESSOR), ID, 0x5_11C2, CELL, GEN),
            Ok(())
        );
        assert_eq!(
            register_as(spawner(OWNER), ID, 0x5_11C3, CELL, GEN),
            Err(Refusal::NotOwner)
        );
        clear_tid(SUCCESSOR);
        clear_tid(0x5_11C2);
    }
}
