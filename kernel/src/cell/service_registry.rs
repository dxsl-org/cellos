//! Kernel Service Registry — stable `service_id → current provider` mapping.
//!
//! The supervisor (init) respawns a dead service under a NEW tid. Clients that
//! addressed the service by its old tid would break. This registry adds an
//! indirection: the supervisor registers each service's tid under a well-known
//! `service_id` ([`abi::syscall::service`]) and re-registers the new tid on
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
//! of a bare tid, and it is the same axis [`abi::caller_identity::CallerIdentity`]
//! and [`abi::cell_owner::CellOwner`] already use — one identity concept, not two.
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
struct ServiceEntry {
    tid: usize,
    cell_id: u64,
    generation: u64,
    /// [`abi::syscall::service_flags`] granted to this incarnation.
    flags: usize,
    /// Hidden from lookups by a hot-swap quiesce; the provider still runs.
    paused: bool,
}

impl ServiceEntry {
    /// The provider's Cell identity, or `None` when it names no live Cell.
    ///
    /// `cell_id == 0` is the kernel's own allocation rather than any Cell, and a
    /// zero generation is not an epoch. Either one means the entry cannot support
    /// a binding, so `LookupServiceBound` treats it exactly like an absent provider.
    fn identity(self) -> Option<(u64, u64)> {
        (self.cell_id != 0 && self.generation != 0).then_some((self.cell_id, self.generation))
    }

    fn names(self, cell_id: u64, generation: u64) -> bool {
        self.identity() == Some((cell_id, generation))
    }
}

static REGISTRY: Spinlock<BTreeMap<u16, ServiceEntry>> =
    Spinlock::ranked(BTreeMap::new(), crate::sync::rank::SERVICE_REGISTRY);

/// Private id → tid of the spawn-only registrar that owns it. Lock order:
/// `OWNERS → REGISTRY`; nothing takes `OWNERS` while holding `REGISTRY`.
static OWNERS: Spinlock<BTreeMap<u16, usize>> =
    Spinlock::ranked(BTreeMap::new(), crate::sync::rank::OWNERS);

/// Ids below this are the well-known namespace of [`abi::syscall::service`];
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
    /// The id is well-known or owned by another live registrar, or flags were
    /// asked by a registrar that is not the namespace owner.
    NotOwner,
    /// Zero tid, an undefined flag bit, or the registry/owner table is full.
    Rejected,
}

/// Publish `tid` as the provider of `service_id` on behalf of `registrar`.
///
/// A [`Registrar::Spawner`] may publish only a private id, without flags, and
/// only one that no other live spawner owns; its first successful publish makes
/// it the owner until it exits ([`clear_tid`]), so it can rebind its own
/// restarted child.
pub fn register_as(
    registrar: Registrar,
    service_id: u16,
    tid: usize,
    cell_id: u64,
    generation: u64,
    flags: usize,
) -> Result<(), Refusal> {
    if flags & !abi::syscall::service_flags::ALL != 0 {
        return Err(Refusal::Rejected);
    }
    let Registrar::Spawner(spawner) = registrar else {
        return insert(service_id, tid, cell_id, generation, flags)
            .then_some(())
            .ok_or(Refusal::Rejected);
    };
    if service_id < PRIVATE_ID_BASE || flags != 0 {
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
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::SERVICE_SHARED,
    );
    owners.insert(service_id, spawner);
    Ok(())
}

/// Register `tid` as the current provider of `service_id`, without flags,
/// replacing any prior entry. Returns `false` (rejected) if the registry is
/// full and `service_id` is new, or if `tid` is 0 (the reserved "none"
/// sentinel). No authority check: the generic `RegisterService` path uses
/// [`register_as`]; the kernel-defined self-registration paths (GPU driver,
/// Silo, demo) check their own authority.
///
/// `cell_id`/`generation` are the provider's live identity, read from the
/// scheduler by the caller. They are recorded, never invented here.
pub fn register(service_id: u16, tid: usize, cell_id: u64, generation: u64) -> bool {
    insert(service_id, tid, cell_id, generation, 0)
}

fn insert(service_id: u16, tid: usize, cell_id: u64, generation: u64, flags: usize) -> bool {
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
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::SERVICE_SHARED,
    );
    map.insert(
        service_id,
        ServiceEntry {
            tid,
            cell_id,
            generation,
            flags,
            paused: false,
        },
    );
    #[cfg(feature = "p01-trace")]
    drop(_semantic);
    log::info!(
        "[service-registry] {} -> tid {} (cell {} gen {} flags {:#x})",
        service_id,
        tid,
        cell_id,
        generation,
        flags
    );
    true
}

/// Resolve `service_id` to its current provider tid, or `None` if no live
/// provider is registered. The syscall layer maps `None` to the ABI value 0.
pub fn lookup(service_id: u16) -> Option<usize> {
    REGISTRY
        .lock()
        .get(&service_id)
        .filter(|entry| !entry.paused)
        .map(|entry| entry.tid)
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
    if entry.paused {
        return None;
    }
    let (cell_id, generation) = entry.identity()?;
    Some((entry.tid, cell_id, generation))
}

/// Whether the Cell incarnation `(cell_id, generation)` provides a service
/// whose row carries `RETAINS_OUTER_CALLER`, Active or Paused.
///
/// Lock order SCHEDULER -> REGISTRY: callers hold the scheduler lock.
pub fn retains_outer_caller(cell_id: u64, generation: u64) -> bool {
    REGISTRY.lock().values().any(|entry| {
        entry.flags & abi::syscall::service_flags::RETAINS_OUTER_CALLER != 0
            && entry.names(cell_id, generation)
    })
}

/// Whether the Cell incarnation `(cell_id, generation)` is the Active provider
/// of a service whose row carries `OWNER_WATCH`. A Paused row grants nothing,
/// and a provider bound to one row has no right through another's binding.
pub fn grants_owner_watch(cell_id: u64, generation: u64) -> bool {
    REGISTRY.lock().values().any(|entry| {
        entry.flags & abi::syscall::service_flags::OWNER_WATCH != 0
            && !entry.paused
            && entry.names(cell_id, generation)
    })
}

/// Whether the hot-swap path may never pause, freeze or republish
/// `service_id`'s provider.
///
/// `MACHINE_PIN` is registered only by trusted init for its own `machine-pin`
/// child (`RegisterService`, ADR-0029 §3); the VFS admits the live provider to
/// the pin store and 433 answers only it. A swap would publish a
/// supervisor-spawned successor that init then adopts, so a `SupervisorCap`
/// holder could hand itself the machine facts and pin write access.
pub fn swap_refused(service_id: u16) -> bool {
    service_id == abi::syscall::service::MACHINE_PIN
}

/// Whether `tid` is the active or paused provider of a service the hot-swap
/// path refuses ([`swap_refused`]).
pub fn is_swap_refused_tid(tid: usize) -> bool {
    REGISTRY
        .lock()
        .iter()
        .any(|(&service_id, entry)| swap_refused(service_id) && entry.tid == tid)
}

/// Hide a service from new lookups while its current provider remains runnable.
///
/// The compare-and-pause contract prevents a stale supervisor request from
/// pausing a replacement that another recovery path already registered.
/// Pausing preserves the recorded identity and flags verbatim: it is a
/// visibility change, not a re-registration. A [`swap_refused`] id is never
/// paused.
pub fn pause(service_id: u16, expected_tid: usize) -> bool {
    if swap_refused(service_id) {
        return false;
    }
    match REGISTRY.lock().get_mut(&service_id) {
        Some(entry) if entry.tid == expected_tid => {
            entry.paused = true;
            true
        }
        _ => false,
    }
}

/// Publish `new_tid` only when `service_id` is still paused at `old_tid`.
///
/// The hot-swap barrier calls this while holding `SCHEDULER`, preserving the
/// global `SCHEDULER -> service registry` lock order. `cell_id`/`generation` are
/// the replacement's live identity, read from the scheduler by that caller. The
/// replacement inherits the row's flags. A [`swap_refused`] id is never
/// republished.
pub(crate) fn commit_paused(
    service_id: u16,
    old_tid: usize,
    new_tid: usize,
    cell_id: u64,
    generation: u64,
) -> bool {
    if new_tid == 0 || swap_refused(service_id) {
        return false;
    }
    match REGISTRY.lock().get_mut(&service_id) {
        Some(entry) if entry.paused && entry.tid == old_tid => {
            *entry = ServiceEntry {
                tid: new_tid,
                cell_id,
                generation,
                flags: entry.flags,
                paused: false,
            };
            true
        }
        _ => false,
    }
}

/// Check the exact paused provider without exposing registry representation.
pub(crate) fn paused_matches(service_id: u16, expected_tid: usize) -> bool {
    REGISTRY
        .lock()
        .get(&service_id)
        .is_some_and(|entry| entry.paused && entry.tid == expected_tid)
}

/// Register `tid` as the provider of `service_id`, without flags, only while
/// `service_id` has no row, active or paused: an existing row is never
/// replaced. The check and the insert share one `REGISTRY` hold. `false`, with
/// nothing changed, when a row exists, the registry is full, or `tid` is 0.
pub(crate) fn register_if_absent(
    service_id: u16,
    tid: usize,
    cell_id: u64,
    generation: u64,
) -> bool {
    if tid == 0 {
        return false;
    }
    let inserted = {
        let mut map = REGISTRY.lock();
        if map.contains_key(&service_id) || map.len() >= MAX_SERVICES {
            false
        } else {
            #[cfg(feature = "p01-trace")]
            let _semantic = crate::memory::heap::p01_owner::creation(
                None,
                crate::memory::heap::p01_owner::SERVICE_SHARED,
            );
            map.insert(
                service_id,
                ServiceEntry {
                    tid,
                    cell_id,
                    generation,
                    flags: 0,
                    paused: false,
                },
            );
            true
        }
    };
    if inserted {
        log::info!(
            "[service-registry] {} -> tid {} (cell {} gen {}), registered while vacant",
            service_id,
            tid,
            cell_id,
            generation
        );
    }
    inserted
}

/// Return whether `tid` is hidden behind any paused service mapping.
///
/// IPC admission uses this as the quiesce barrier for callers that cached the
/// provider tid before the mapping was paused.
pub fn is_paused_tid(tid: usize) -> bool {
    REGISTRY
        .lock()
        .values()
        .any(|entry| entry.paused && entry.tid == tid)
}

/// Return whether `tid` is the active or paused provider of any trusted
/// supervisor-owned service registration.
pub fn is_registered_tid(tid: usize) -> bool {
    REGISTRY.lock().values().any(|entry| entry.tid == tid)
}

/// Remove every registration that points at `tid`. Called from `exit_task` when a
/// task dies so a client never resolves a service to a dead provider; the
/// supervisor re-registers the replacement's tid on respawn. A dead registrar
/// also gives up its private ids, so its restarted successor can claim them.
pub fn clear_tid(tid: usize) {
    {
        let mut map = REGISTRY.lock();
        let before = map.len();
        map.retain(|_, entry| entry.tid != tid);
        if map.len() != before {
            log::info!(
                "[service-registry] cleared stale entries for dead tid {}",
                tid
            );
        }
    }
    OWNERS.lock().retain(|_, owner| *owner != tid);
}

/// Re-point `service_id` from provider `from_tid` to `to = (tid, cell_id,
/// generation)` in place: only an active row that still names `from_tid`
/// changes, so a racing re-registration is never overwritten. The row keeps
/// its map slot, so nothing is allocated (exit teardown, which may run in an
/// ISR, calls it); it takes `to`'s identity with no flags and no pause, as
/// [`register`] would write it. `false`, with nothing changed, when no row of
/// `service_id` names `from_tid`, the row is paused, or `to` names tid 0.
///
/// A paused row belongs to a hot swap in flight: only [`commit_paused`] moves
/// it, and [`clear_tid`] removes it when its provider dies, so the swap's
/// commit then fails instead of publishing over a re-pointed row.
pub(crate) fn repoint(service_id: u16, from_tid: usize, to: (usize, u64, u64)) -> bool {
    let (tid, cell_id, generation) = to;
    if tid == 0 {
        return false;
    }
    {
        let mut map = REGISTRY.lock();
        let Some(entry) = map
            .get_mut(&service_id)
            .filter(|entry| entry.tid == from_tid && !entry.paused)
        else {
            return false;
        };
        *entry = ServiceEntry {
            tid,
            cell_id,
            generation,
            flags: 0,
            paused: false,
        };
    }
    log::info!(
        "[service-registry] {} -> tid {} (cell {} gen {}), re-pointed from tid {}",
        service_id,
        tid,
        cell_id,
        generation,
        from_tid
    );
    true
}

/// Remove `service_id`'s row while it is active and names `tid`: a live
/// provider that lost the authority behind the row (a runtime capability
/// revoke). A paused row is left for its hot swap, as in [`repoint`]. Frees
/// at most one map node, never allocates. `false` when nothing was removed.
pub(crate) fn remove_active(service_id: u16, tid: usize) -> bool {
    let removed = {
        let mut map = REGISTRY.lock();
        let named = map
            .get(&service_id)
            .is_some_and(|entry| entry.tid == tid && !entry.paused);
        named && map.remove(&service_id).is_some()
    };
    if removed {
        log::info!(
            "[service-registry] {} removed: tid {} lost its authority",
            service_id,
            tid
        );
    }
    removed
}

/// `(service_id, tid, paused, flags)` for every row.
#[cfg(feature = "test-hooks")]
pub(crate) fn snapshot() -> alloc::vec::Vec<(u16, usize, bool, usize)> {
    REGISTRY
        .lock()
        .iter()
        .map(|(service_id, entry)| (*service_id, entry.tid, entry.paused, entry.flags))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: u64 = 7;
    const GEN: u64 = 3;

    #[test]
    fn register_then_lookup() {
        assert!(register(abi::syscall::service::VFS, 4, CELL, GEN));
        assert_eq!(lookup(abi::syscall::service::VFS), Some(4));
    }

    #[test]
    fn reject_zero_tid() {
        assert!(!register(abi::syscall::service::NET, 0, CELL, GEN));
        assert_eq!(lookup(abi::syscall::service::NET), None);
    }

    #[test]
    fn reregister_updates_tid() {
        register(abi::syscall::service::INPUT, 7, CELL, GEN);
        register(abi::syscall::service::INPUT, 9, CELL + 1, GEN + 1);
        assert_eq!(lookup(abi::syscall::service::INPUT), Some(9));
    }

    #[test]
    fn clear_tid_removes_dead_provider() {
        register(abi::syscall::service::CONFIG, 12, CELL, GEN);
        clear_tid(12);
        assert_eq!(lookup(abi::syscall::service::CONFIG), None);
    }

    #[test]
    fn repoint_moves_only_a_row_naming_the_old_provider() {
        const TEST_SERVICE: u16 = 60_020;
        const OLD: usize = 0x6_0201;
        const NEW: usize = 0x6_0202;
        assert!(!repoint(TEST_SERVICE, OLD, (NEW, CELL, GEN)), "no row");
        assert_eq!(lookup(TEST_SERVICE), None);

        register(TEST_SERVICE, OLD, CELL, GEN);
        assert!(
            !repoint(TEST_SERVICE, OLD - 1, (NEW, CELL, GEN)),
            "another provider"
        );
        assert!(!repoint(TEST_SERVICE, OLD, (0, CELL, GEN)), "tid 0");
        assert_eq!(lookup_bound(TEST_SERVICE), Some((OLD, CELL, GEN)));

        assert!(repoint(TEST_SERVICE, OLD, (NEW, CELL + 1, GEN + 1)));
        assert_eq!(lookup_bound(TEST_SERVICE), Some((NEW, CELL + 1, GEN + 1)));
        clear_tid(OLD);
        assert_eq!(
            lookup(TEST_SERVICE),
            Some(NEW),
            "the old tid's teardown misses it"
        );
        clear_tid(NEW);
    }

    #[test]
    fn register_if_absent_never_replaces_a_row() {
        const TEST_SERVICE: u16 = 60_022;
        const HELD: usize = 0x6_0221;
        const LATE: usize = 0x6_0222;
        assert!(!register_if_absent(TEST_SERVICE, 0, CELL, GEN), "tid 0");
        register(TEST_SERVICE, HELD, CELL, GEN);
        assert!(!register_if_absent(TEST_SERVICE, LATE, CELL, GEN));
        assert_eq!(lookup_bound(TEST_SERVICE), Some((HELD, CELL, GEN)));
        assert!(pause(TEST_SERVICE, HELD));
        assert!(!register_if_absent(TEST_SERVICE, LATE, CELL, GEN));
        assert!(paused_matches(TEST_SERVICE, HELD), "paused row kept");
        clear_tid(HELD);
        assert!(register_if_absent(TEST_SERVICE, LATE, CELL, GEN + 1));
        assert_eq!(lookup_bound(TEST_SERVICE), Some((LATE, CELL, GEN + 1)));
        clear_tid(LATE);
    }

    /// A paused row is mid hot-swap: neither re-pointed nor removed by the
    /// NIC route paths, so `commit_paused` still finds it at the source; its
    /// provider's death still removes it.
    #[test]
    fn a_paused_row_is_neither_repointed_nor_removed() {
        const TEST_SERVICE: u16 = 60_021;
        const OLD: usize = 0x6_0211;
        const NEW: usize = 0x6_0212;
        const SWAP: usize = 0x6_0213;
        register(TEST_SERVICE, OLD, CELL, GEN);
        assert!(pause(TEST_SERVICE, OLD));
        assert!(!repoint(TEST_SERVICE, OLD, (NEW, CELL, GEN)));
        assert!(!remove_active(TEST_SERVICE, OLD));
        assert!(paused_matches(TEST_SERVICE, OLD), "still paused at OLD");
        assert!(commit_paused(TEST_SERVICE, OLD, SWAP, CELL, GEN + 1));
        assert_eq!(lookup_bound(TEST_SERVICE), Some((SWAP, CELL, GEN + 1)));

        assert!(!remove_active(TEST_SERVICE, OLD), "another provider");
        assert!(remove_active(TEST_SERVICE, SWAP));
        assert_eq!(lookup(TEST_SERVICE), None);

        register(TEST_SERVICE, OLD, CELL, GEN);
        assert!(pause(TEST_SERVICE, OLD));
        clear_tid(OLD);
        assert!(!paused_matches(TEST_SERVICE, OLD), "death removes it");
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

    /// Review finding (Phase 13): a `SupervisorCap` holder must not hot-swap
    /// the machine-pin provider — no pause, no republish, and its tid is
    /// recognised for the freeze and replacement-spawn refusals.
    #[test]
    fn machine_pin_is_never_paused_or_republished() {
        const PIN: u16 = abi::syscall::service::MACHINE_PIN;
        const PROVIDER: usize = 9_101;
        const SUCCESSOR: usize = 9_102;
        assert!(swap_refused(PIN));
        assert!(!swap_refused(abi::syscall::service::CONFIG));
        register(PIN, PROVIDER, CELL, GEN);
        assert!(is_swap_refused_tid(PROVIDER));
        assert!(!is_swap_refused_tid(SUCCESSOR));

        assert!(!pause(PIN, PROVIDER));
        assert!(!is_paused_tid(PROVIDER));
        assert_eq!(lookup_bound(PIN), Some((PROVIDER, CELL, GEN)));
        assert!(!commit_paused(PIN, PROVIDER, SUCCESSOR, CELL + 1, GEN));
        assert_eq!(lookup_bound(PIN), Some((PROVIDER, CELL, GEN)));

        clear_tid(PROVIDER);
        assert!(!is_swap_refused_tid(PROVIDER));
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
        use abi::syscall::service::{KMS, SUPERVISOR};
        const HIJACKER: usize = 0x5_11A0;
        for id in [SUPERVISOR, KMS] {
            assert_eq!(
                register_as(Registrar::Spawner(HIJACKER), id, HIJACKER, CELL, GEN, 0),
                Err(Refusal::NotOwner)
            );
            assert_ne!(lookup(id), Some(HIJACKER));
        }
        assert_eq!(
            register_as(Registrar::Namespace, KMS, 0x5_11A1, CELL, GEN, 0),
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

        assert_eq!(
            register_as(spawner(OWNER), ID, 0x5_11C0, CELL, GEN, 0),
            Ok(())
        );
        assert_eq!(
            register_as(spawner(OWNER), ID, 0x5_11C1, CELL, GEN + 1, 0),
            Ok(())
        );
        assert_eq!(lookup(ID), Some(0x5_11C1));
        assert_eq!(
            register_as(spawner(OTHER), ID, OTHER, CELL, GEN, 0),
            Err(Refusal::NotOwner)
        );
        assert_eq!(lookup(ID), Some(0x5_11C1));
        assert_eq!(
            register_as(spawner(OWNER), ID, 0, CELL, GEN, 0),
            Err(Refusal::Rejected)
        );

        clear_tid(OWNER);
        assert_eq!(
            register_as(spawner(SUCCESSOR), ID, 0x5_11C2, CELL, GEN, 0),
            Ok(())
        );
        assert_eq!(
            register_as(spawner(OWNER), ID, 0x5_11C3, CELL, GEN, 0),
            Err(Refusal::NotOwner)
        );
        clear_tid(SUCCESSOR);
        clear_tid(0x5_11C2);
    }

    /// D1: flags come only from the namespace owner, an undefined bit is refused,
    /// RETAINS_OUTER_CALLER holds through a quiesce while OWNER_WATCH does not,
    /// and a hot-swap commit hands both to the replacement.
    #[test]
    fn registration_flags_follow_the_row() {
        use abi::syscall::service_flags::{ALL, OWNER_WATCH, RETAINS_OUTER_CALLER};
        const ID: u16 = 0x7A0C;
        const OLD: usize = 0x5_11D0;
        const NEW: usize = 0x5_11D1;
        const NEW_CELL: u64 = CELL + 1;

        assert_eq!(
            register_as(Registrar::Namespace, ID, OLD, CELL, GEN, ALL + 1),
            Err(Refusal::Rejected)
        );
        assert_eq!(
            register_as(Registrar::Spawner(OLD), ID, OLD, CELL, GEN, OWNER_WATCH),
            Err(Refusal::NotOwner)
        );
        assert_eq!(lookup(ID), None);

        assert_eq!(
            register_as(Registrar::Namespace, ID, OLD, CELL, GEN, ALL),
            Ok(())
        );
        assert!(retains_outer_caller(CELL, GEN) && grants_owner_watch(CELL, GEN));
        assert!(!retains_outer_caller(CELL, GEN + 1) && !grants_owner_watch(CELL, GEN + 1));

        assert!(pause(ID, OLD));
        assert!(retains_outer_caller(CELL, GEN));
        assert!(!grants_owner_watch(CELL, GEN));

        assert!(commit_paused(ID, OLD, NEW, NEW_CELL, GEN));
        assert!(!retains_outer_caller(CELL, GEN));
        assert!(retains_outer_caller(NEW_CELL, GEN) && grants_owner_watch(NEW_CELL, GEN));

        // A plain re-registration states its own flags.
        assert!(register(ID, NEW, NEW_CELL, GEN));
        assert!(!retains_outer_caller(NEW_CELL, GEN) && !grants_owner_watch(NEW_CELL, GEN));
        assert_eq!(
            register_as(
                Registrar::Namespace,
                ID,
                NEW,
                NEW_CELL,
                GEN,
                RETAINS_OUTER_CALLER
            ),
            Ok(())
        );
        clear_tid(NEW);
        assert!(!retains_outer_caller(NEW_CELL, GEN));
    }
}
