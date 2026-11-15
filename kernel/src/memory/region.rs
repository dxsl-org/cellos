//! One record behind the five region tables (architecture remediation
//! Phase 16, census §7.7).
//!
//! `PAGE_GRANT_TABLE` (`GrantAlloc`), `REG_GRANT_TABLE` (`GrantRegister`), the
//! SHM handles (`ShmAlloc`), the DMA export leases (`EXPORTS`) and the domain
//! grant records (carried by grant rows) each kept their own row type, with its
//! own owner, rights and receiver fields and its own idea of who frees the
//! frames. Each table now stores one [`Region`]: who owns it, what it allows,
//! who receives it, what it holds in the pin registry, and its frames — a
//! [`FrameRun`] the region owns until its release takes it, or the range of
//! another region's frames it locks (a DMA export). Every table keeps its
//! lock, its rank and its key, so the syscall-visible ids (a grant's or an SHM
//! frame's base address, an export id) do not change.
//!
//! A region's frames leave it only through [`Region::into_run`], which consumes
//! the region: whoever took the row out of its table is the one releaser (the
//! single-free rule, census risk 1). A region dropped while it still owns a run
//! never frees it: the run is leaked, counted and logged (`FrameRun`'s drop).

use crate::memory::frame_run::FrameRun;
use crate::memory::pin::ReleaseKey;
use types::{CellId, GrantPerm};

/// The lifecycle record a domain-backed grant row carries (owner and receiver
/// private roots, `Live → Revoking → Revoked`), on targets with the
/// private-root grant lifecycle.
#[cfg(cellos_domain_grants)]
pub(crate) type DomainRecord = alloc::sync::Arc<crate::task::domain_grant::DomainGrant>;

/// Elsewhere no row can carry a lifecycle record: the slot is always `None`.
#[cfg(not(cellos_domain_grants))]
pub(crate) type DomainRecord = ();

/// Who owns a region: a task and the cell incarnation it ran as.
///
/// `tid` is 0 for a region an incarnation owns rather than a task (SHM), and
/// for a region nobody owns any more (an orphaned registered grant, or a
/// domain row left ownerless while its revoke waits): no caller has tid 0, so
/// neither matches an owner check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegionOwner {
    pub(crate) tid: usize,
    pub(crate) cell: CellId,
    pub(crate) generation: u64,
}

impl RegionOwner {
    /// Nobody.
    pub(crate) const NONE: Self = Self {
        tid: 0,
        cell: CellId(0),
        generation: 0,
    };

    /// Task `tid` of incarnation `(cell, generation)`.
    pub(crate) const fn task(tid: usize, cell: CellId, generation: u64) -> Self {
        Self {
            tid,
            cell,
            generation,
        }
    }

    /// Incarnation `(cell, generation)` itself, no task.
    pub(crate) const fn incarnation(cell: u64, generation: u64) -> Self {
        Self {
            tid: 0,
            cell: CellId(cell),
            generation,
        }
    }

    /// `(cell, generation)`.
    pub(crate) const fn binding(&self) -> (CellId, u64) {
        (self.cell, self.generation)
    }
}

/// Who receives a region; one receiver at most in every table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Receiver {
    None,
    /// The task a grant is shared with (`GrantShare`), with its permission.
    Task {
        tid: usize,
        perm: GrantPerm,
    },
    /// The cell an SHM frame was handed to (`ShmMap`'s target).
    Cell(u64),
    /// The provider incarnation that holds a DMA export.
    Holder {
        cell: u64,
        generation: u64,
    },
}

impl Receiver {
    /// The grantee task and its permission, if this is a grant receiver.
    pub(crate) const fn task(&self) -> Option<(usize, GrantPerm)> {
        match *self {
            Receiver::Task { tid, perm } => Some((tid, perm)),
            _ => None,
        }
    }
}

/// What a region itself holds in the pin registry (`memory::pin`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PinState {
    /// No range lock of its own. An in-flight operation may still pin part of
    /// the region (`pin::holder_of`); its release then withholds the run under
    /// that operation's key (`pin::withhold_pinned_run`).
    Unlocked,
    /// The region is a range lock over frames another region owns, under
    /// `key`, the key its quarantine holds are released by (a DMA export,
    /// `pin::lock_dma_export`).
    Locked(ReleaseKey),
}

/// A region's frames.
enum Backing {
    /// Owned: `Mapped` until the region's release takes the run.
    Run(FrameRun),
    /// `[base, base + size)` of frames another region owns.
    Borrowed { base: usize },
}

/// One row of a region table. `X` is the table's own state, if it has any
/// (a DMA export's lifecycle and device window).
pub(crate) struct Region<X = ()> {
    pub(crate) owner: RegionOwner,
    /// Bytes the region was asked for (and charged): a grant's or an export's
    /// frames cover them rounded up to whole pages; an SHM handle is one frame
    /// whatever it was charged.
    pub(crate) size: usize,
    /// What the region allows: its owner (a grant or SHM frame is `ReadWrite`)
    /// or its device holder (a DMA export).
    pub(crate) rights: GrantPerm,
    pub(crate) receiver: Receiver,
    pub(crate) pin: PinState,
    /// The private-root lifecycle record of a domain-backed grant.
    pub(crate) domain: Option<DomainRecord>,
    backing: Backing,
    pub(crate) ext: X,
}

impl Region {
    /// A `GrantAlloc` or `GrantRegister` row owning `run`, not yet shared.
    pub(crate) fn grant(
        owner: RegionOwner,
        run: FrameRun,
        size: usize,
        domain: Option<DomainRecord>,
    ) -> Self {
        Self {
            owner,
            size,
            rights: GrantPerm::ReadWrite,
            receiver: Receiver::None,
            pin: PinState::Unlocked,
            domain,
            backing: Backing::Run(run),
            ext: (),
        }
    }

    /// An `ShmAlloc` frame owned by an incarnation, not yet handed to a cell.
    pub(crate) fn shm(owner: RegionOwner, run: FrameRun, size: usize) -> Self {
        Self {
            owner,
            size,
            rights: GrantPerm::ReadWrite,
            receiver: Receiver::None,
            pin: PinState::Unlocked,
            domain: None,
            backing: Backing::Run(run),
            ext: (),
        }
    }
}

impl<X> Region<X> {
    /// A DMA export: a range lock (`key`) over `[base, base + size)` of a
    /// grant `owner` owns, held by `holder`.
    pub(crate) fn export(
        owner: RegionOwner,
        base: usize,
        size: usize,
        rights: GrantPerm,
        holder: Receiver,
        key: ReleaseKey,
        ext: X,
    ) -> Self {
        Self {
            owner,
            size,
            rights,
            receiver: holder,
            pin: PinState::Locked(key),
            domain: None,
            backing: Backing::Borrowed { base },
            ext,
        }
    }

    /// The first address of the region's frames.
    pub(crate) fn base(&self) -> usize {
        match &self.backing {
            Backing::Run(run) => run.base(),
            Backing::Borrowed { base } => *base,
        }
    }

    /// The grantee and its permission (`GrantShare`), if any.
    pub(crate) const fn shared_to(&self) -> Option<(usize, GrantPerm)> {
        self.receiver.task()
    }

    /// Share with `grantee` (`Some`) or with nobody.
    pub(crate) fn set_shared_to(&mut self, grantee: Option<(usize, GrantPerm)>) {
        self.receiver = match grantee {
            Some((tid, perm)) => Receiver::Task { tid, perm },
            None => Receiver::None,
        };
    }

    /// Leave the region with neither owner nor receiver: no caller can act on
    /// it any more, only the reaper that releases it.
    pub(crate) fn orphan(&mut self) {
        self.owner = RegionOwner::NONE;
        self.receiver = Receiver::None;
    }

    /// The region's own frames, for the one caller whose removal took the row
    /// to release; `None` for a region that only locks another's frames.
    pub(crate) fn into_run(self) -> Option<FrameRun> {
        match self.backing {
            Backing::Run(run) => Some(run),
            Backing::Borrowed { .. } => None,
        }
    }

    /// Host tests: drop a row whose run names synthetic frames (no allocator
    /// owns them) without counting a leak.
    #[cfg(test)]
    pub(crate) fn discard_synthetic(self) {
        if let Some(run) = self.into_run() {
            core::mem::forget(run);
        }
    }
}
