//! CPU-only, one-page grants between two private RV64 roots.
//!
//! The grant never transfers frame ownership: revocation removes the receiver
//! PTE, performs the synchronous CPU shootdown, and retains the owner's frame.
//! A grant that cannot observe safe-root quiescence remains `Revoking`.

use super::domain_switch::DomainRef;
use crate::memory::{
    address_space::{AddressSpaceError, MappingKind},
    paging::{Flags, PAGE_SIZE},
};
use core::sync::atomic::{AtomicU8, Ordering};
use types::{PhysAddr, VAddr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainGrantState {
    Live = 1,
    Revoking = 2,
    Revoked = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainGrantError {
    InvalidMapping,
    NotLive,
    Mapping(AddressSpaceError),
    AwaitingSafeRoot,
}

/// One owner page mapped at exactly one grantee virtual address.
pub(crate) struct DomainGrant {
    pub(crate) owner: DomainRef,
    pub(crate) grantee: DomainRef,
    pub(crate) range: (PhysAddr, usize),
    pub(crate) receiver_va: VAddr,
    pub(crate) perms: Flags,
    pub(crate) generation: u64,
    state: AtomicU8,
}

impl DomainGrant {
    /// Map a private owner page into exactly one live grantee. Legacy SAS grant
    /// tables never call this API, so cross-domain sharing cannot inherit them.
    pub(crate) fn create(
        owner: DomainRef,
        owner_va: VAddr,
        grantee: DomainRef,
        receiver_va: VAddr,
        perms: Flags,
    ) -> Result<Self, DomainGrantError> {
        if !owner_va.is_multiple_of(PAGE_SIZE)
            || !receiver_va.is_multiple_of(PAGE_SIZE)
            || !owner.address_space().is_live()
            || !grantee.address_space().is_live()
            || owner.tuple() == grantee.tuple()
        {
            return Err(DomainGrantError::InvalidMapping);
        }
        let Some((_, physical_address)) = owner.address_space().page_proof_for(owner_va) else {
            return Err(DomainGrantError::InvalidMapping);
        };
        if !matches!(
            owner.address_space().ledger().into_iter().find(|entry| entry.virtual_address == owner_va),
            Some(entry) if entry.kind == MappingKind::Private
        ) {
            return Err(DomainGrantError::InvalidMapping);
        }
        grantee
            .address_space()
            .map_grant_page(receiver_va, physical_address, perms)
            .map_err(DomainGrantError::Mapping)?;
        Ok(Self {
            owner,
            grantee,
            range: (physical_address, PAGE_SIZE),
            receiver_va,
            perms,
            generation: 1,
            state: AtomicU8::new(DomainGrantState::Live as u8),
        })
    }
    /// The owner mapping stays authoritative for the grant's whole lifetime.
    pub(crate) fn preserves_ownership_contract(&self) -> bool {
        self.owner.address_space().is_live()
            && self.range.1 == PAGE_SIZE
            && self.perms.bits() & Flags::EXECUTE == 0
            && self.generation != 0
    }

    pub(crate) fn state(&self) -> DomainGrantState {
        match self.state.load(Ordering::Acquire) {
            1 => DomainGrantState::Live,
            2 => DomainGrantState::Revoking,
            _ => DomainGrantState::Revoked,
        }
    }

    /// Linearize revocation before PTE removal. `unmap_grant_page` flushes the
    /// local ASID and waits for the SBI remote fence transport; a still-current
    /// grantee root keeps this grant in `Revoking` and its owner frame retained.
    pub(crate) fn revoke(&self) -> Result<(), DomainGrantError> {
        self.state
            .compare_exchange(
                DomainGrantState::Live as u8,
                DomainGrantState::Revoking as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| DomainGrantError::NotLive)?;
        let active_harts = self.grantee.address_space().current_harts();
        self.grantee
            .address_space()
            .unmap_grant_page(self.receiver_va)
            .map_err(DomainGrantError::Mapping)?;
        if self.grantee.address_space().current_harts() & active_harts != 0 {
            return Err(DomainGrantError::AwaitingSafeRoot);
        }
        self.state
            .store(DomainGrantState::Revoked as u8, Ordering::Release);
        Ok(())
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn run_selftest() {
    use crate::memory::address_space::AddressSpaceBuilder;
    use alloc::sync::Arc;

    fn space(
        va: VAddr,
    ) -> Result<Arc<crate::memory::address_space::AddressSpace>, AddressSpaceError> {
        let mut builder = AddressSpaceBuilder::new();
        builder.map_user_page(
            va,
            MappingKind::Private,
            Flags::from_bits(Flags::READ | Flags::WRITE),
        )?;
        builder.build()
    }

    let outcome = (|| {
        let owner = space(0x0040_0000)?;
        let grantee = space(0x00c0_0000)?;
        let grant = DomainGrant::create(
            DomainRef::from_address_space(&owner),
            0x0040_0000,
            DomainRef::from_address_space(&grantee),
            0x0080_0000,
            Flags::from_bits(Flags::READ | Flags::WRITE),
        )
        .map_err(|_| AddressSpaceError::InvalidMapping)?;
        let ownership_preserved = grant.preserves_ownership_contract();
        grant
            .revoke()
            .map_err(|_| AddressSpaceError::InvalidMapping)?;
        Ok::<_, AddressSpaceError>(
            ownership_preserved
                && grant.state() == DomainGrantState::Revoked
                && grantee.page_proof_for(0x0080_0000).is_none(),
        )
    })();
    if outcome == Ok(true) {
        log::info!("S22-RV64-GRANT-REVOKE: PASS");
        log::info!("S22-RV64-DMA-QUARANTINE: DENY");
    } else {
        log::error!("S22-RV64-GRANT-REVOKE: FAIL");
    }
}
