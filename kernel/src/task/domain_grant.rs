//! CPU-only domain grants between two private RV64 roots.
//!
//! One kernel-owned record per grant carries the owner root, the receiver root
//! (`Arc<AddressSpace>`, retained while the receiver PTE is published), the
//! exact owner/receiver identity and virtual range, the rights copied into the
//! receiver PTE, and a `Live → Revoking → Revoked` state. The record never
//! transfers frame ownership: revocation removes the receiver PTE, performs the
//! synchronous CPU shootdown, removes the owner PTE, and only then may the
//! frames leave the grant table.
//!
//! A grant that cannot observe safe-root quiescence (the receiver root still
//! current on a hart, or the SBI remote-fence transport unacknowledged) stays
//! `Revoking`: its frames are retained and no entry point may publish a new
//! mapping over the old one.
//!
//! The record is owned by the `PAGE_GRANT_TABLE`/`REG_GRANT_TABLE` rows in
//! `super::syscall`; this module is the state machine they embed.

use super::domain_switch::DomainRef;
use crate::memory::address_space::{AddressSpace, AddressSpaceError};
use crate::memory::paging::{Flags, PAGE_SIZE};
use crate::sync::Spinlock;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use types::{CellId, GrantPerm, PhysAddr, VAddr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainGrantState {
    Live = 1,
    Revoking = 2,
    Revoked = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainGrantError {
    /// The requested mapping cannot be represented (unaligned, overlapping an
    /// existing receiver mapping, or naming a non-live root).
    InvalidMapping,
    /// The record is not `Live`; a `Revoking` record refuses new slices/shares.
    NotLive,
    /// The receiver PTE could not be published or removed.
    Mapping(AddressSpaceError),
    /// Unmap succeeded but the root/tag invalidation is not acknowledged. The
    /// record stays `Revoking` with its frames retained for an idempotent retry.
    AwaitingSafeRoot,
    /// The requested rights are not enforceable on an ordinary domain page.
    UnsupportedRights,
}

/// The receiver half of a domain grant.
pub(crate) struct DomainReceiver {
    pub(crate) root: Arc<AddressSpace>,
    pub(crate) cell: CellId,
    pub(crate) generation: u64,
    pub(crate) base: VAddr,
    pub(crate) size: usize,
    pub(crate) rights: Flags,
    /// Set once the receiver PTE is gone and its invalidation acknowledged.
    drained: bool,
}

/// Resolve the exact PTE rights for a domain pair, or `None` when the requested
/// permission has no enforceable page representation.
///
/// `ReadOnly` → R+NX, `ReadWrite` → RW+NX. A genuine write-only page is not
/// representable on any supported target, so it is refused rather than widened
/// to RW.
pub(crate) fn rights_for(perm: GrantPerm) -> Option<Flags> {
    match perm {
        GrantPerm::ReadOnly => Some(Flags::from_bits(Flags::READ)),
        GrantPerm::ReadWrite => Some(Flags::from_bits(Flags::READ | Flags::WRITE)),
        GrantPerm::WriteOnly => None,
    }
}

/// One owner page range mapped into exactly one live receiver root.
pub(crate) struct DomainGrant {
    owner: DomainRef,
    owner_base: PhysAddr,
    owner_size: usize,
    owner_drained: AtomicBool,
    receiver: Spinlock<Option<DomainReceiver>>,
    state: AtomicU8,
}

impl DomainGrant {
    /// Record an owner mapping this grant's allocation just published.
    pub(crate) fn new(owner: &Arc<AddressSpace>, base: PhysAddr, size: usize) -> Self {
        Self {
            owner: DomainRef::from_address_space(owner),
            owner_base: base,
            owner_size: size,
            owner_drained: AtomicBool::new(false),
            receiver: Spinlock::new(None),
            state: AtomicU8::new(DomainGrantState::Live as u8),
        }
    }

    /// The owner root must still accept new scheduling work for a grant to be
    /// sliceable: a dying owner's frames are on their way back to the allocator.
    pub(crate) fn owner_is_live(&self) -> bool {
        self.owner.address_space().is_live()
    }

    pub(crate) fn state(&self) -> DomainGrantState {
        match self.state.load(Ordering::Acquire) {
            1 => DomainGrantState::Live,
            2 => DomainGrantState::Revoking,
            _ => DomainGrantState::Revoked,
        }
    }

    /// Is the receiver mapping already exactly `(cell, generation, root, rights)`?
    /// A redundant re-share must not tear down a live mapping.
    pub(crate) fn receiver_matches(
        &self,
        cell: CellId,
        generation: u64,
        root: &Arc<AddressSpace>,
        rights: Flags,
    ) -> bool {
        self.receiver.lock().as_ref().is_some_and(|receiver| {
            !receiver.drained
                && receiver.cell == cell
                && receiver.generation == generation
                && Arc::ptr_eq(&receiver.root, root)
                && receiver.rights.bits() == rights.bits()
        })
    }

    /// Publish `rights` for `root` at the grant's physical base, replacing any
    /// previous receiver mapping.
    ///
    /// The tuple `(cell, generation, root, rights)` is compared first: only a
    /// change revokes the old PTE. Every page is mapped transactionally; on any
    /// failure the pages already mapped are unmapped (with acknowledgement) and
    /// nothing is published.
    pub(crate) fn publish(
        &self,
        root: &Arc<AddressSpace>,
        cell: CellId,
        generation: u64,
        rights: Flags,
    ) -> Result<(), DomainGrantError> {
        if self.state() != DomainGrantState::Live {
            return Err(DomainGrantError::NotLive);
        }
        if rights.bits() & Flags::EXECUTE != 0 || rights.bits() & Flags::READ == 0 {
            return Err(DomainGrantError::UnsupportedRights);
        }
        if !root.is_live() {
            return Err(DomainGrantError::InvalidMapping);
        }
        if self.receiver_matches(cell, generation, root, rights) {
            return Ok(());
        }
        // A different tuple (re-share, target change or same-recipient
        // downgrade) revokes the old receiver PTE and shoots it down before the
        // new one is published.
        self.drain_receiver()?;

        let base = self.owner_base;
        let n_pages = self.owner_size.div_ceil(PAGE_SIZE).max(1);
        let mut mapped = 0usize;
        for index in 0..n_pages {
            let va = base + index * PAGE_SIZE;
            match root.map_grant_page(va, va, rights) {
                Ok(()) => {
                    // The hart that writes a PTE must invalidate before any
                    // translation of it is relied on; the receiver root is
                    // inactive on this hart, so the local page flush is the
                    // whole requirement here.
                    crate::hal::paging::flush_tlb_page(va);
                    mapped += 1;
                }
                Err(error) => {
                    for undo in 0..mapped {
                        let _ = root.unmap_grant_page(base + undo * PAGE_SIZE);
                    }
                    let _ = crate::memory::tlb_shootdown::flush_asid_and_await(root.asid());
                    return Err(DomainGrantError::Mapping(error));
                }
            }
        }
        *self.receiver.lock() = Some(DomainReceiver {
            root: Arc::clone(root),
            cell,
            generation,
            base,
            size: self.owner_size,
            rights,
            drained: false,
        });
        Ok(())
    }

    /// Remove the receiver PTE and confirm the invalidation. `Ok(())` means the
    /// receiver half is drained and its retained root may be dropped.
    pub(crate) fn drain_receiver(&self) -> Result<(), DomainGrantError> {
        let Some((root, base, size)) = ({
            let guard = self.receiver.lock();
            guard.as_ref().filter(|receiver| !receiver.drained).map(|receiver| {
                (
                    Arc::clone(&receiver.root),
                    receiver.base,
                    receiver.size,
                )
            })
        }) else {
            return Ok(());
        };
        let n_pages = size.div_ceil(PAGE_SIZE).max(1);
        for index in 0..n_pages {
            match root.unmap_grant_page(base + index * PAGE_SIZE) {
                Ok(()) | Err(AddressSpaceError::NotFound) => {}
                Err(_) => return Err(DomainGrantError::AwaitingSafeRoot),
            }
        }
        if crate::memory::tlb_shootdown::flush_asid_and_await(root.asid()).is_err() {
            return Err(DomainGrantError::AwaitingSafeRoot);
        }
        if let Some(receiver) = self.receiver.lock().as_mut() {
            receiver.drained = true;
        }
        Ok(())
    }

    /// Remove the owner PTE and confirm the invalidation.
    fn drain_owner(&self) -> Result<(), DomainGrantError> {
        if self.owner_drained.load(Ordering::Acquire) {
            return Ok(());
        }
        let root = Arc::clone(self.owner.address_space());
        let n_pages = self.owner_size.div_ceil(PAGE_SIZE).max(1);
        for index in 0..n_pages {
            match root.unmap_grant_page(self.owner_base + index * PAGE_SIZE) {
                Ok(()) | Err(AddressSpaceError::NotFound) => {}
                Err(_) => return Err(DomainGrantError::AwaitingSafeRoot),
            }
        }
        if crate::memory::tlb_shootdown::flush_asid_and_await(root.asid()).is_err() {
            return Err(DomainGrantError::AwaitingSafeRoot);
        }
        self.owner_drained.store(true, Ordering::Release);
        Ok(())
    }

    /// Linearize revocation before PTE removal and drive both halves to
    /// quiescence. Idempotent: a record left `Revoking` by an unacknowledged
    /// invalidation is retried with the same effect.
    pub(crate) fn revoke(&self) -> Result<(), DomainGrantError> {
        match self.state() {
            DomainGrantState::Revoked => return Ok(()),
            DomainGrantState::Live => {
                let _ = self.state.compare_exchange(
                    DomainGrantState::Live as u8,
                    DomainGrantState::Revoking as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            }
            DomainGrantState::Revoking => {}
        }
        if self.state() == DomainGrantState::Revoked {
            return Ok(());
        }
        self.drain_receiver()?;
        self.drain_owner()?;
        self.state
            .store(DomainGrantState::Revoked as u8, Ordering::Release);
        Ok(())
    }
}

/// Boot self-test: the table path is the single implementation, so this drives
/// the production `handle_syscall` entry points with two real
/// `TaskAddressSpace::Domain` tasks and observes the ABI result of each step.
///
/// Emits the established `S22-RV64-GRANT-REVOKE` terminal plus a
/// property marker per assertion. Every synthetic task and grant is torn down
/// before it returns.
#[cfg(all(
    feature = "test-hooks",
    feature = "native-domains",
    target_arch = "riscv64"
))]
pub(crate) fn run_selftest() {
    use super::syscall::{handle_syscall, Syscall};
    use super::thread_cap_selftest::{insert, mk_task, remove};
    use super::tcb::TaskAddressSpace;
    use crate::memory::address_space::{AddressSpaceBuilder, MappingKind};

    const OWNER_TID: usize = 9601;
    const RECEIVER_TID: usize = 9602;
    const DEAD_TID: usize = 9603;
    const OWNER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 20) as u64;
    const RECEIVER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 21) as u64;
    const PAGE: usize = PAGE_SIZE;
    const GRANT_SIZE: usize = PAGE;
    /// A private-root VA for each fixture task, clear of the SAS identity window.
    const OWNER_VA: usize = 0x0400_0000;
    const RECEIVER_VA: usize = 0x0800_0000;

    fn space(va: usize) -> Option<Arc<AddressSpace>> {
        let mut builder = AddressSpaceBuilder::new();
        builder
            .map_user_page(
                va,
                MappingKind::Private,
                Flags::from_bits(Flags::READ | Flags::WRITE),
            )
            .ok()?;
        builder.build().ok()
    }

    fn grant_alloc_size(tid: usize, size: usize) -> Option<usize> {
        match handle_syscall(tid, Syscall::GrantAlloc { size }) {
            Ok(base) if base != 0 => Some(base),
            _ => None,
        }
    }

    fn grant_alloc(tid: usize) -> Option<usize> {
        grant_alloc_size(tid, GRANT_SIZE)
    }

    fn grant_slice(tid: usize, grant_id: usize) -> Option<usize> {
        match handle_syscall(
            tid,
            Syscall::GrantSlice {
                grant_id,
                size_out_ptr: 0,
            },
        ) {
            Ok(base) if base != usize::MAX => Some(base),
            _ => None,
        }
    }

    let (Some(owner_space), Some(receiver_space)) = (space(OWNER_VA), space(RECEIVER_VA)) else {
        log::error!("S22-RV64-GRANT-REVOKE: FAIL fixture-setup");
        return;
    };

    let mut owner_task = mk_task(OWNER_TID, OWNER_CELL);
    owner_task.address_space = TaskAddressSpace::Domain(Arc::clone(&owner_space));
    insert(owner_task);
    let mut receiver_task = mk_task(RECEIVER_TID, RECEIVER_CELL);
    receiver_task.address_space = TaskAddressSpace::Domain(Arc::clone(&receiver_space));
    insert(receiver_task);

    let mut ok = true;
    let mut report = |marker: &str, property: bool| {
        if property {
            log::info!("S22-RV64-GRANT-REVOKE-{marker}: PASS");
        } else {
            log::error!("S22-RV64-GRANT-REVOKE-{marker}: FAIL");
            ok = false;
        }
    };

    // 1. Owner allocation: the pointer is published in the owner's private root
    //    and the SAS/global root must stay supervisor-only.
    let allocated = grant_alloc(OWNER_TID);
    let owner_mapped = allocated.is_some_and(|base| {
        let (present, user) = crate::memory::paging::mapping_state(base);
        let ledger_owner = owner_space.ledger().into_iter().any(|entry| {
            entry.virtual_address == base
                && entry.kind == MappingKind::Grant
                && entry.flags.bits() & Flags::WRITE != 0
                && entry.flags.bits() & Flags::EXECUTE == 0
        });
        present && !user && ledger_owner
    });
    report("OWNER-MAPPED", owner_mapped);

    // 2. The owner resolves its own grant without a second mapping.
    let owner_slice = allocated.is_some_and(|base| grant_slice(OWNER_TID, base) == Some(base));
    report("OWNER-SLICE", owner_slice);

    // 3. Read-only slice: R+NX in the receiver root, no write bit.
    let ro_readable = allocated.is_some_and(|base| {
        let shared = handle_syscall(
            OWNER_TID,
            Syscall::GrantShare {
                grant_id: base,
                target_cell: RECEIVER_TID,
                perm: GrantPerm::ReadOnly as usize,
            },
        )
        .is_ok();
        let sliced = grant_slice(RECEIVER_TID, base) == Some(base);
        let ro_pte = receiver_space.ledger().into_iter().any(|entry| {
            entry.virtual_address == base
                && entry.kind == MappingKind::Grant
                && entry.flags.bits() & Flags::READ != 0
                && entry.flags.bits() & Flags::WRITE == 0
                && entry.flags.bits() & Flags::EXECUTE == 0
        });
        shared && sliced && ro_pte
    });
    report("SLICE-RO", ro_readable);

    // 4. Same-recipient upgrade RW→RO must republish: the new PTE is RW+NX and
    //    the old read-only PTE is gone.
    let rw_writable = allocated.is_some_and(|base| {
        handle_syscall(
            OWNER_TID,
            Syscall::GrantShare {
                grant_id: base,
                target_cell: RECEIVER_TID,
                perm: GrantPerm::ReadWrite as usize,
            },
        )
        .is_ok()
            && grant_slice(RECEIVER_TID, base) == Some(base)
            && receiver_space.ledger().into_iter().any(|entry| {
                entry.virtual_address == base
                    && entry.kind == MappingKind::Grant
                    && entry.flags.bits() & Flags::WRITE != 0
                    && entry.flags.bits() & Flags::EXECUTE == 0
            })
    });
    report("SLICE-RW", rw_writable);

    // 5. Write-only has no ordinary-page representation: refused, never widened.
    let write_only_refused = allocated.is_some_and(|base| {
        handle_syscall(
            OWNER_TID,
            Syscall::GrantShare {
                grant_id: base,
                target_cell: RECEIVER_TID,
                perm: GrantPerm::WriteOnly as usize,
            },
        )
        .is_err()
    });
    report("WO-REFUSED", write_only_refused);

    // 6. A non-domain peer cannot be named as a domain receiver.
    let foreign_peer_refused = allocated.is_some_and(|base| {
        handle_syscall(
            OWNER_TID,
            Syscall::GrantShare {
                grant_id: base,
                target_cell: 0,
                perm: GrantPerm::ReadOnly as usize,
            },
        )
        .is_err()
    });
    report("FOREIGN-PEER", foreign_peer_refused);

    // 7. Revoke through GrantFree: both PTEs disappear, the receiver root no
    //    longer resolves the address, and a re-slice is refused.
    let revoked = allocated.is_some_and(|base| {
        let freed = handle_syscall(OWNER_TID, Syscall::GrantFree { grant_id: base }).is_ok();
        let receiver_gone = !receiver_space
            .ledger()
            .into_iter()
            .any(|entry| entry.virtual_address == base);
        let owner_gone = !owner_space
            .ledger()
            .into_iter()
            .any(|entry| entry.virtual_address == base);
        freed && receiver_gone && owner_gone && grant_slice(RECEIVER_TID, base).is_none()
    });
    report("REVOKE", revoked);

    // 8. Frame reuse: an allocation handed the same frame must be zeroed before
    //    it can be published, so no byte of the revoked grant survives.
    let reuse_private = grant_alloc(OWNER_TID).is_some_and(|base| {
        // SAFETY: the frame is identity-mapped supervisor in the current root.
        let zeroed = (0..GRANT_SIZE).all(|offset| {
            (unsafe { core::ptr::read_volatile((base + offset) as *const u8) }) == 0
        });
        let _ = handle_syscall(OWNER_TID, Syscall::GrantFree { grant_id: base });
        zeroed
    });
    report("FRAME-REUSE", reuse_private);

    // 9. A multi-page receiver publish that fails on its second page must undo
    //    the first: the owner's grant is untouched, no half-mapping is visible in
    //    the receiver root, and the pre-existing private mapping survives.
    let partial_map = grant_alloc_size(OWNER_TID, 2 * PAGE).is_some_and(|base| {
        let occupied = receiver_space
            .map_private_page(
                base + PAGE,
                MappingKind::Private,
                Flags::from_bits(Flags::READ | Flags::WRITE),
            )
            .is_ok();
        let shared = handle_syscall(
            OWNER_TID,
            Syscall::GrantShare {
                grant_id: base,
                target_cell: RECEIVER_TID,
                perm: GrantPerm::ReadWrite as usize,
            },
        )
        .is_err();
        let no_partial = !receiver_space
            .ledger()
            .into_iter()
            .any(|entry| entry.virtual_address == base && entry.kind == MappingKind::Grant);
        let private_intact = receiver_space.ledger().into_iter().any(|entry| {
            entry.virtual_address == base + PAGE && entry.kind == MappingKind::Private
        });
        let _ = handle_syscall(OWNER_TID, Syscall::GrantFree { grant_id: base });
        occupied && shared && no_partial && private_intact
    });
    report("PARTIAL-MAP", partial_map);

    // 10. A retired private root is not a capability: its owner is refused with
    //     the alloc-safe sentinel, and a receiver naming it is refused.
    let mut dead_task = mk_task(DEAD_TID, RECEIVER_CELL + 1);
    dead_task.address_space = TaskAddressSpace::Domain(Arc::clone(&receiver_space));
    insert(dead_task);
    receiver_space.retire();
    let dead_refused = grant_alloc(DEAD_TID).is_none()
        && handle_syscall(
            DEAD_TID,
            Syscall::GrantSlice {
                grant_id: 0,
                size_out_ptr: 0,
            },
        )
        .is_ok_and(|result| result == usize::MAX);
    remove(DEAD_TID);
    report("DEAD-ROOT", dead_refused);

    // Teardown: the owner-side reaper drains and releases whatever the probe
    // left behind, then the synthetic tasks go away.
    super::syscall::reclaim_owned_grants(OWNER_TID);
    super::syscall::reclaim_owned_grants(RECEIVER_TID);
    remove(OWNER_TID);
    remove(RECEIVER_TID);

    if ok {
        log::info!("S22-RV64-GRANT-REVOKE: PASS");
        log::info!("S22-RV64-DMA-QUARANTINE: DENY");
    } else {
        log::error!("S22-RV64-GRANT-REVOKE: FAIL");
    }
}
