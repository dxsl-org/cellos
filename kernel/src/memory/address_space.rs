//! Private Sv39 roots for native RV64 domains; a root remains private until its builder publishes it.
use super::{
    frame::{phys_to_virt, OwnedFrame},
    paging::{Flags, PAGE_SIZE},
};
use crate::{sync::Spinlock, PhysAddr, VAddr};
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use hal::PageTableTrait;
const USER_LIMIT: usize = 1usize << 38;
static NEXT_DOMAIN: AtomicU64 = AtomicU64::new(1);

/// Frames whose invalidation could not be confirmed. They never return to the
/// allocator: a hart that still resolves the retired translation could walk into
/// them, and "leaked" is the only safe end state for that. Drained only by an
/// explicit audit, and counted so the leak is visible rather than silent.
static QUARANTINED_FRAMES: Spinlock<Vec<OwnedFrame>> = Spinlock::new(Vec::new());

/// Retain frames whose invalidation was not acknowledged.
fn quarantine_frames(frames: Vec<OwnedFrame>, reason: &str) {
    let count = frames.len();
    if count == 0 {
        return;
    }
    log::error!(
        "[aspace] quarantining {} frame(s): {} — they will not be reused",
        count,
        reason
    );
    QUARANTINED_FRAMES.lock().extend(frames);
}

/// Test-hooks view of the quarantine: frames retained after an unacknowledged
/// invalidation. A non-zero value is a leak by design, never a silent one.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn quarantined_frame_count() -> usize {
    QUARANTINED_FRAMES.lock().len()
}

/// Width of the architectural tag a root register can carry on this target.
///
/// RV64 `satp` and AArch64 `TTBR0` carry 16-bit ASIDs; x86 PCID is 12-bit. A tag
/// is only ever programmed after the backend has decided the register is usable
/// (x86 needs CPUID `PCID`/`INVPCID` and `CR4.PCIDE`, and a root that cannot carry
/// a tag must use tag 0 with a full flush — never a nonzero value the hardware
/// would ignore or alias).
#[inline]
pub(crate) const fn asid_width() -> usize {
    if cfg!(target_arch = "x86_64") {
        12
    } else {
        16
    }
}

/// Test-hooks view of how many domain identities have been issued. The admission
/// selftest uses it to prove a refused launch creates no domain at all.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) fn domain_identity_counter() -> u64 {
    NEXT_DOMAIN.load(Ordering::Relaxed)
}
#[cfg(feature = "test-hooks")]
static FAIL_ALLOCATION_AFTER: AtomicUsize = AtomicUsize::new(usize::MAX);
#[cfg(feature = "test-hooks")]
static FAIL_NEXT_MAP: AtomicU8 = AtomicU8::new(0);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainId(u64);
impl DomainId {
    /// Stable per-domain identity for hart-local bookkeeping; it grants no mapping authority.
    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingKind {
    Private,
    ImmutableImage,
    SharedAbi,
    Grant,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressSpaceState {
    Live = 1,
    Dying = 2,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressSpaceError {
    OutOfMemory,
    InvalidMapping,
    InvalidHart,
    WriteExecute,
    NotFound,
    Dying,
    InvalidationUnacknowledged,
}
/// A copy lease counted until its guard is dropped; Phase 03 will wait for these before revoking pages.
pub struct CopyReader<'a> {
    address_space: &'a AddressSpace,
}
impl Drop for CopyReader<'_> {
    fn drop(&mut self) {
        self.address_space
            .copy_readers
            .fetch_sub(1, Ordering::Release);
    }
}
/// A mapping is auditable without consulting global SAS state.
#[derive(Clone, Copy, Debug)]
pub struct MappingEntry {
    pub virtual_address: VAddr,
    pub physical_address: PhysAddr,
    pub kind: MappingKind,
    pub flags: Flags,
}
/// A supervisor page can only be supplied by kernel code as part of its narrow map list.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct SupervisorMapping {
    virtual_address: VAddr,
    physical_address: PhysAddr,
    flags: Flags,
}
impl SupervisorMapping {
    #[cfg(all(
        feature = "native-domains",
        feature = "test-hooks",
        target_arch = "riscv64"
    ))]
    pub(crate) fn identity_page(
        physical_address: PhysAddr,
        flags: Flags,
    ) -> Result<Self, AddressSpaceError> {
        if !physical_address.is_multiple_of(PAGE_SIZE) || flags.bits() & Flags::USER != 0 {
            return Err(AddressSpaceError::InvalidMapping);
        }
        Ok(Self {
            virtual_address: physical_address,
            physical_address,
            flags,
        })
    }
}
#[derive(Clone, Copy)]
pub(crate) struct ExistingUserMapping {
    virtual_address: VAddr,
    physical_address: PhysAddr,
    kind: MappingKind,
    flags: Flags,
}
fn allocate_owned_frame() -> Result<OwnedFrame, AddressSpaceError> {
    #[cfg(feature = "test-hooks")]
    {
        let remaining = FAIL_ALLOCATION_AFTER.load(Ordering::Acquire);
        if remaining == 0 {
            return Err(AddressSpaceError::OutOfMemory);
        }
        if remaining != usize::MAX {
            FAIL_ALLOCATION_AFTER.store(remaining - 1, Ordering::Release);
        }
    }
    let frame = OwnedFrame::allocate().ok_or(AddressSpaceError::OutOfMemory)?;
    // SAFETY: the newly allocated frame is exclusively owned by this value.
    unsafe {
        core::ptr::write_bytes(
            phys_to_virt(frame.physical_address()) as *mut u8,
            0,
            PAGE_SIZE,
        );
    }
    Ok(frame)
}
#[derive(Clone, Copy)]
struct RequestedMapping {
    virtual_address: VAddr,
    kind: MappingKind,
    flags: Flags,
}

/// The uncommitted transaction. Dropping it returns roots, intermediate tables, and pages.
pub struct AddressSpaceBuilder {
    identity: DomainId,
    supervisor: Vec<SupervisorMapping>,
    requests: Vec<RequestedMapping>,
    existing_user: Vec<ExistingUserMapping>,
}
impl Default for AddressSpaceBuilder {
    fn default() -> Self {
        Self::new()
    }
}
impl AddressSpaceBuilder {
    pub fn new() -> Self {
        Self {
            identity: DomainId(NEXT_DOMAIN.fetch_add(1, Ordering::Relaxed)),
            supervisor: Vec::new(),
            requests: Vec::new(),
            existing_user: Vec::new(),
        }
    }

    pub(crate) fn allow_supervisor(&mut self, mapping: SupervisorMapping) {
        self.supervisor.push(mapping);
    }

    pub(crate) fn map_registered_execution(&mut self, kernel_stack: &crate::task::stack::Stack) {
        use crate::memory::domain_supervisor_registry::{shared_snapshot, SupervisorRangeKind};

        for range in shared_snapshot() {
            let flags = match range.kind {
                SupervisorRangeKind::StaticText => {
                    Flags::from_bits(Flags::VALID | Flags::READ | Flags::EXECUTE | Flags::ACCESSED)
                }
                SupervisorRangeKind::StaticReadOnly => {
                    Flags::from_bits(Flags::VALID | Flags::READ | Flags::ACCESSED)
                }
                SupervisorRangeKind::StaticWritable
                | SupervisorRangeKind::KernelHeap
                | SupervisorRangeKind::KernelStack
                | SupervisorRangeKind::PrivatePageTable => Flags::from_bits(
                    Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
                ),
                SupervisorRangeKind::DeviceMmio => Flags::from_bits(
                    Flags::VALID
                        | Flags::READ
                        | Flags::WRITE
                        | Flags::DEVICE
                        | Flags::ACCESSED
                        | Flags::DIRTY,
                ),
            };
            for address in (range.start..range.end).step_by(PAGE_SIZE) {
                self.allow_supervisor(SupervisorMapping {
                    virtual_address: address,
                    physical_address: address,
                    flags,
                });
            }
        }
        let flags = Flags::from_bits(
            Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
        );
        for address in (kernel_stack.usable_start()..kernel_stack.top).step_by(PAGE_SIZE) {
            self.allow_supervisor(SupervisorMapping {
                virtual_address: address,
                physical_address: address,
                flags,
            });
        }
    }

    pub fn map_existing_user_page(
        &mut self,
        virtual_address: VAddr,
        physical_address: PhysAddr,
        kind: MappingKind,
        flags: Flags,
    ) -> Result<(), AddressSpaceError> {
        validate_user_mapping(virtual_address, flags)?;
        self.existing_user.push(ExistingUserMapping {
            virtual_address,
            physical_address,
            kind,
            flags,
        });
        Ok(())
    }

    pub fn map_user_page(
        &mut self,
        virtual_address: VAddr,
        kind: MappingKind,
        flags: Flags,
    ) -> Result<(), AddressSpaceError> {
        validate_user_mapping(virtual_address, flags)?;
        self.requests.push(RequestedMapping {
            virtual_address,
            kind,
            flags,
        });
        Ok(())
    }
    pub fn build(self) -> Result<Arc<AddressSpace>, AddressSpaceError> {
        let AddressSpaceBuilder {
            identity,
            #[allow(unused_variables)]
            supervisor,
            requests,
            existing_user,
        } = self;
        // Claim the architectural tag before anything is allocated: when every tag
        // is held by a live root the builder must refuse the domain (no reuse of a
        // live tag, no SAS fallback), and a refused build leaves no frame behind.
        let asid = AsidLease::acquire(identity.raw()).ok_or(AddressSpaceError::OutOfMemory)?;
        let root = allocate_owned_frame()?;
        // SAFETY: root is a zeroed private page frame and PageTable has the same page layout.
        unsafe {
            core::ptr::write(
                phys_to_virt(root.physical_address()) as *mut hal::PageTable,
                hal::PageTable::empty(),
            );
        }
        #[cfg(target_arch = "x86_64")]
        if let Some(kernel_root_phys) = *crate::memory::paging::KERNEL_ROOT.lock() {
            let kernel_pml4 =
                unsafe { &*(phys_to_virt(kernel_root_phys) as *const hal::PageTable) };
            let new_pml4 =
                unsafe { &mut *(phys_to_virt(root.physical_address()) as *mut hal::PageTable) };
            new_pml4.copy_kernel_higher_half(kernel_pml4);
        }
        let mut table_frames = Vec::new();
        let mut frames = Vec::new();
        let mut ledger = Vec::new();
        for mapping in supervisor {
            if mapping.virtual_address < 0x0000_8000_0000_0000 {
                map_page(
                    root.physical_address(),
                    &mut table_frames,
                    mapping.virtual_address,
                    mapping.physical_address,
                    mapping.flags,
                )?;
            }
        }
        for request in requests {
            let page = allocate_owned_frame()?;
            map_page(
                root.physical_address(),
                &mut table_frames,
                request.virtual_address,
                page.physical_address(),
                user_flags(request.flags),
            )?;
            ledger.push(MappingEntry {
                virtual_address: request.virtual_address,
                physical_address: page.physical_address(),
                kind: request.kind,
                flags: user_flags(request.flags),
            });
            frames.push(page);
        }
        for mapping in existing_user {
            map_page(
                root.physical_address(),
                &mut table_frames,
                mapping.virtual_address,
                mapping.physical_address,
                user_flags(mapping.flags),
            )?;
            ledger.push(MappingEntry {
                virtual_address: mapping.virtual_address,
                physical_address: mapping.physical_address,
                kind: mapping.kind,
                flags: user_flags(mapping.flags),
            });
        }
        #[cfg(feature = "test-hooks")]
        let supervisor_registrations =
            register_private_table_frames(&root, &table_frames, identity.raw())?;
        Ok(Arc::new(AddressSpace {
            identity,
            generation: NEXT_DOMAIN.fetch_add(1, Ordering::Relaxed),
            asid,
            state: AtomicU8::new(AddressSpaceState::Live as u8),
            invalidation_pending: Spinlock::new(Vec::new()),
            ledger: Spinlock::new(ledger),
            copy_readers: AtomicUsize::new(0),
            current_harts: AtomicUsize::new(0),
            table_frames: Spinlock::new(table_frames),
            frames: Spinlock::new(frames),
            #[cfg(feature = "test-hooks")]
            supervisor_registrations,
            root: core::mem::ManuallyDrop::new(root),
        }))
    }
}

/// A published private root. Its root frame is manually released only after
/// the tag's completion boundary; an unacknowledged teardown quarantines it.
pub struct AddressSpace {
    identity: DomainId,
    generation: u64,
    asid: AsidLease,
    state: AtomicU8,
    /// VAs removed from the ledger but not yet safe to remap under this tag.
    invalidation_pending: Spinlock<Vec<VAddr>>,
    ledger: Spinlock<Vec<MappingEntry>>,
    copy_readers: AtomicUsize,
    current_harts: AtomicUsize,
    frames: Spinlock<Vec<OwnedFrame>>,
    /// Intermediate Sv39 page-table frames, separate from user mappings.
    table_frames: Spinlock<Vec<OwnedFrame>>,
    /// Registry tokens retire before the root/page tables return to the allocator.
    #[cfg(feature = "test-hooks")]
    supervisor_registrations: Vec<crate::memory::domain_supervisor_registry::SupervisorRangeId>,
    root: core::mem::ManuallyDrop<OwnedFrame>,
}

impl AddressSpace {
    pub fn identity(&self) -> DomainId {
        self.identity
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn ledger(&self) -> alloc::vec::Vec<MappingEntry> {
        self.ledger.lock().clone()
    }
    pub fn root_ppn(&self) -> usize {
        self.root.physical_address() >> 12
    }
    pub fn asid(&self) -> usize {
        self.asid.value
    }

    /// Reports whether new scheduling work may still target this private root.
    #[inline]
    pub fn is_live(&self) -> bool {
        self.state.load(Ordering::Acquire) == AddressSpaceState::Live as u8
    }
    /// Acquires a copy lease only while the root is live. A concurrent retirement makes the lease unavailable.
    pub fn acquire_copy_reader(&self) -> Result<CopyReader<'_>, AddressSpaceError> {
        if self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8 {
            return Err(AddressSpaceError::Dying);
        }
        self.copy_readers.fetch_add(1, Ordering::AcqRel);
        if self.state.load(Ordering::Acquire) == AddressSpaceState::Live as u8 {
            Ok(CopyReader {
                address_space: self,
            })
        } else {
            self.copy_readers.fetch_sub(1, Ordering::Release);
            Err(AddressSpaceError::Dying)
        }
    }
    pub fn copy_reader_count(&self) -> usize {
        self.copy_readers.load(Ordering::Acquire)
    }
    /// Ledger record (flags + physical frame) for one mapped page base,
    /// without cloning the whole ledger. Used by the user-copy probe pass;
    /// the caller still confirms the live PTE before moving bytes.
    #[allow(dead_code)]
    pub(crate) fn page_proof_for(&self, virtual_address: VAddr) -> Option<(Flags, PhysAddr)> {
        self.ledger.lock().iter().find_map(|entry| {
            (entry.virtual_address == virtual_address)
                .then_some((entry.flags, entry.physical_address))
        })
    }
    /// Records a hart executing this root; clearing remains available after retirement for Phase 06 acknowledgement.
    pub fn set_current_hart(&self, hart: usize, current: bool) -> Result<(), AddressSpaceError> {
        if hart >= usize::BITS as usize {
            return Err(AddressSpaceError::InvalidHart);
        }
        if current && self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8 {
            return Err(AddressSpaceError::Dying);
        }
        let bit = 1usize << hart;
        if current {
            self.current_harts.fetch_or(bit, Ordering::AcqRel);
        } else {
            self.current_harts.fetch_and(!bit, Ordering::AcqRel);
        }
        Ok(())
    }
    /// Pins this hart as an executor of this root, failing closed against the
    /// selection-time TOCTOU where retirement lands between the Live check and
    /// the pin. Mirrors `acquire_copy_reader`'s double-check: a retirement that
    /// races the pin makes it unavailable, while retirement AFTER a successful
    /// pin is legal — the set bit pins the space against teardown drain until
    /// the owning hart clears it on its next transition away from this root.
    pub fn begin_execution(&self, hart: usize) -> Result<(), AddressSpaceError> {
        if hart >= usize::BITS as usize {
            return Err(AddressSpaceError::InvalidHart);
        }
        if self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8 {
            return Err(AddressSpaceError::Dying);
        }
        let bit = 1usize << hart;
        let prior = self.current_harts.fetch_or(bit, Ordering::AcqRel);
        if self.state.load(Ordering::Acquire) == AddressSpaceState::Live as u8 {
            Ok(())
        } else if prior & bit == 0 {
            // Roll back only our own pin: the bit may have been set before we
            // raced (same-domain reselection already executing this root), and
            // erasing a pre-existing pin would drop the hart out of the drain
            // set while it still executes on this root.
            self.current_harts.fetch_and(!bit, Ordering::Release);
            Err(AddressSpaceError::Dying)
        } else {
            Err(AddressSpaceError::Dying)
        }
    }
    pub fn current_harts(&self) -> usize {
        self.current_harts.load(Ordering::Acquire)
    }
    /// Reserve a retired VA until the old translation has been invalidated.
    /// Lock order for mutations is pending -> ledger -> table_frames -> frames;
    /// copy proofs use only ledger, so draining readers never holds that lock.
    fn begin_unmap(
        &self,
        virtual_address: VAddr,
        kind: Option<MappingKind>,
    ) -> Result<MappingEntry, AddressSpaceError> {
        let mut pending = self.invalidation_pending.lock();
        if pending.contains(&virtual_address) {
            return Err(AddressSpaceError::NotFound);
        }
        let mut ledger = self.ledger.lock();
        let position = ledger
            .iter()
            .position(|entry| {
                entry.virtual_address == virtual_address
                    && kind.is_none_or(|kind| entry.kind == kind)
            })
            .ok_or(AddressSpaceError::NotFound)?;
        pending.push(virtual_address);
        Ok(ledger.remove(position))
    }

    fn abort_unmap(&self, entry: MappingEntry) {
        let mut pending = self.invalidation_pending.lock();
        self.ledger.lock().push(entry);
        pending.retain(|address| *address != entry.virtual_address);
    }

    fn finish_unmap(&self, virtual_address: VAddr) {
        self.invalidation_pending
            .lock()
            .retain(|address| *address != virtual_address);
    }

    pub fn map_private_page(
        &self,
        virtual_address: VAddr,
        kind: MappingKind,
        flags: Flags,
    ) -> Result<(), AddressSpaceError> {
        validate_user_mapping(virtual_address, flags)?;
        let pending = self.invalidation_pending.lock();
        if pending.contains(&virtual_address) {
            return Err(AddressSpaceError::InvalidMapping);
        }
        let mut ledger = self.ledger.lock();
        drop(pending);
        if self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8
            || ledger
                .iter()
                .any(|entry| entry.virtual_address == virtual_address)
        {
            return Err(AddressSpaceError::Dying);
        }
        let page = allocate_owned_frame()?;
        let mut table_frames = self.table_frames.lock();
        let mut frames = self.frames.lock();
        let mut pruned_tables = Vec::new();
        let result = map_page_retaining_pruned_tables(
            self.root.physical_address(),
            &mut table_frames,
            &mut pruned_tables,
            virtual_address,
            page.physical_address(),
            user_flags(flags),
        );
        if let Err(error) = result {
            // Allocation can publish an intermediate table before failing.
            if crate::memory::tlb_shootdown::flush_asid_and_await(self.asid()).is_err() {
                quarantine_frames(pruned_tables, "private map rollback invalidation unacknowledged");
                quarantine_frames(alloc::vec![page], "private map rollback leaf retained");
                return Err(AddressSpaceError::InvalidationUnacknowledged);
            }
            return Err(error);
        }
        drop(pruned_tables);
        ledger.push(MappingEntry {
            virtual_address,
            physical_address: page.physical_address(),
            kind,
            flags: user_flags(flags),
        });
        frames.push(page);
        Ok(())
    }

    /// Map a newly allocated task's existing stack frames into this live domain
    /// before that task becomes runnable. Kernel stacks remain supervisor-only;
    /// user stacks receive the normal private writable user mapping.
    pub(crate) fn map_existing_task_stacks(
        &self,
        kernel_stack: &crate::task::stack::Stack,
        user_stack: &crate::task::stack::Stack,
    ) -> Result<(), AddressSpaceError> {
        if self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8 {
            return Err(AddressSpaceError::Dying);
        }

        let supervisor_flags = Flags::from_bits(
            Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
        );
        let user_stack_flags = Flags::from_bits(
            Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
        );
        let mut mapped = Vec::new();
        mapped
            .try_reserve_exact(kernel_stack.pages + user_stack.pages)
            .map_err(|_| AddressSpaceError::OutOfMemory)?;

        let pending = self.invalidation_pending.lock();
        let mut ledger = self.ledger.lock();
        if pending.iter().any(|address| {
            (kernel_stack.usable_start()..kernel_stack.top).contains(address)
                || (user_stack.usable_start()..user_stack.top).contains(address)
        }) {
            return Err(AddressSpaceError::InvalidMapping);
        }
        drop(pending);
        if self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8 {
            return Err(AddressSpaceError::Dying);
        }
        if (user_stack.usable_start()..user_stack.top)
            .step_by(PAGE_SIZE)
            .any(|address| ledger.iter().any(|entry| entry.virtual_address == address))
        {
            return Err(AddressSpaceError::InvalidMapping);
        }

        let mut table_frames = self.table_frames.lock();
        let mut pruned_table_frames = Vec::new();
        let mut unacked = false;
        let result = (|| {
            for address in (kernel_stack.usable_start()..kernel_stack.top).step_by(PAGE_SIZE) {
                let physical_address = crate::memory::paging::virt_to_phys(address)
                    .ok_or(AddressSpaceError::InvalidMapping)?;
                map_page_retaining_pruned_tables(
                    self.root.physical_address(),
                    &mut table_frames,
                    &mut pruned_table_frames,
                    address,
                    physical_address,
                    supervisor_flags,
                )?;
                mapped.push(address);
            }
            for address in (user_stack.usable_start()..user_stack.top).step_by(PAGE_SIZE) {
                let physical_address = crate::memory::paging::virt_to_phys(address)
                    .ok_or(AddressSpaceError::InvalidMapping)?;
                map_page_retaining_pruned_tables(
                    self.root.physical_address(),
                    &mut table_frames,
                    &mut pruned_table_frames,
                    address,
                    physical_address,
                    user_flags(user_stack_flags),
                )?;
                ledger.push(MappingEntry {
                    virtual_address: address,
                    physical_address,
                    kind: MappingKind::Private,
                    flags: user_flags(user_stack_flags),
                });
                mapped.push(address);
            }
            Ok(())
        })();
        if result.is_err() {
            for address in mapped.into_iter().rev() {
                unmap_existing_page(
                    self.root.physical_address(),
                    &mut table_frames,
                    &mut pruned_table_frames,
                    address,
                );
            }
            ledger.retain(|entry| {
                entry.virtual_address < user_stack.usable_start()
                    || entry.virtual_address >= user_stack.top
            });
            // Both stack ranges belong to this private tag. A current-root
            // page flush cannot invalidate an inactive private root.
            if crate::memory::tlb_shootdown::flush_asid_and_await(self.asid()).is_err() {
                unacked = true;
            }
        }
        // The stack backing belongs to the caller and would be freed after an
        // ordinary Err. Without an ack there is no safe return to that caller.
        if unacked {
            quarantine_frames(pruned_table_frames, "stack unmap invalidation unacknowledged");
            panic!("[aspace] stack map rollback could not invalidate its private tag");
        }
        drop(pruned_table_frames);
        result
    }

    /// Remove a reaped worker's stack mappings before its frames return to the
    /// global allocator.
    ///
    /// Close new user-copy proofs for a reaped worker's stack mappings, drain
    /// proofs already in flight, then remove the PTEs before the frames return
    /// to the global allocator.
    pub(crate) fn unmap_existing_task_stacks(
        &self,
        kernel_stack: &crate::task::stack::Stack,
        user_stack: &crate::task::stack::Stack,
    ) {
        // User-copy takes its mapping proof from this ledger while holding a
        // CopyReader. Removing the entries first prevents a reader admitted
        // after this point from reaching the PTE; a reader that already proved
        // one keeps its lease and must drain before unmapping can begin.
        self.ledger.lock().retain(|entry| {
            entry.virtual_address < user_stack.usable_start()
                || entry.virtual_address >= user_stack.top
        });
        while self.copy_readers.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }

        let mut table_frames = self.table_frames.lock();
        let mut pruned_table_frames = Vec::new();
        for address in (kernel_stack.usable_start()..kernel_stack.top).step_by(PAGE_SIZE) {
            unmap_existing_page(
                self.root.physical_address(),
                &mut table_frames,
                &mut pruned_table_frames,
                address,
            );
        }
        // Keep detached tables until both ranges have been removed.
        for address in (user_stack.usable_start()..user_stack.top).step_by(PAGE_SIZE) {
            unmap_existing_page(
                self.root.physical_address(),
                &mut table_frames,
                &mut pruned_table_frames,
                address,
            );
        }
        drop(table_frames);
        if crate::memory::tlb_shootdown::flush_asid_and_await(self.asid()).is_ok() {
            drop(pruned_table_frames);
        } else {
            quarantine_frames(
                pruned_table_frames,
                "task-stack unmap invalidation unacknowledged",
            );
            // This API cannot take ownership of the external stack backing
            // frames. Prevent the caller from freeing them after a missing ack.
            panic!("[aspace] task stack teardown could not invalidate its private tag");
        }
    }

    pub fn unmap_private_page(&self, virtual_address: VAddr) -> Result<(), AddressSpaceError> {
        let entry = self.begin_unmap(virtual_address, None)?;
        // Wait for all in-flight copy readers to drain before unmapping PTE and reclaiming frame.
        while self.copy_readers.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }
        let mut table_frames = self.table_frames.lock();
        let mut frames = self.frames.lock();
        // An existing (non-owned) page must not be torn down by this API.
        let Some(index) = frames
            .iter()
            .position(|frame| frame.physical_address() == entry.physical_address)
        else {
            drop(frames);
            drop(table_frames);
            self.abort_unmap(entry);
            return Err(AddressSpaceError::NotFound);
        };
        // Detached frames and the leaf stay owned until the tag flush completes.
        let mut detached_tables = Vec::new();
        // SAFETY: the table_frames lock serializes mutations of this private root.
        let table =
            unsafe { &mut *(phys_to_virt(self.root.physical_address()) as *mut hal::PageTable) };
        if table.unmap(virtual_address).is_err() {
            drop(frames);
            drop(table_frames);
            self.abort_unmap(entry);
            return Err(AddressSpaceError::NotFound);
        }
        table.prune_empty(virtual_address, &mut |physical_address| {
            if let Some(index) = table_frames
                .iter()
                .position(|frame| frame.physical_address() == physical_address)
            {
                detached_tables.push(table_frames.remove(index));
            }
        });
        let leaf = frames.remove(index);
        drop(frames);
        drop(table_frames);
        if crate::memory::tlb_shootdown::flush_asid_and_await(self.asid()).is_err() {
            // Keep the VA reserved: remapping it under this tag would revive a
            // stale translation even though its original frames are retained.
            quarantine_frames(detached_tables, "private-page unmap invalidation unacknowledged");
            quarantine_frames(
                alloc::vec![leaf],
                "private-page leaf invalidation unacknowledged",
            );
            return Err(AddressSpaceError::InvalidationUnacknowledged);
        }
        drop(detached_tables);
        drop(leaf);
        self.finish_unmap(virtual_address);
        Ok(())
    }
    /// TEST-ONLY protocol-violation injection for the user-copy fixtures.
    ///
    /// Performs exactly what [`Self::unmap_private_page`] does — ledger
    /// removal, PTE teardown, table pruning, frame release — but skips the
    /// copy-reader drain spin and flushes the local TLB entry so an in-flight
    /// guarded copy is genuinely left with a dangling mapping. The public API
    /// can never produce this interleaving; that is precisely what the
    /// forced-fault fixture proves.
    #[cfg(feature = "test-hooks")]
    pub fn force_unmap_without_drain_for_test(
        &self,
        virtual_address: VAddr,
    ) -> Result<(), AddressSpaceError> {
        let entry = {
            let mut ledger = self.ledger.lock();
            let position = ledger
                .iter()
                .position(|entry| entry.virtual_address == virtual_address)
                .ok_or(AddressSpaceError::NotFound)?;
            ledger.remove(position)
        };
        let mut table_frames = self.table_frames.lock();
        let mut frames = self.frames.lock();
        // SAFETY: only this address space owns and mutates its root.
        let table =
            unsafe { &mut *(phys_to_virt(self.root.physical_address()) as *mut hal::PageTable) };
        table
            .unmap(virtual_address)
            .map_err(|_| AddressSpaceError::NotFound)?;
        crate::hal::paging::flush_tlb_page(virtual_address);
        table.prune_empty(virtual_address, &mut |physical_address| {
            if let Some(index) = table_frames
                .iter()
                .position(|frame| frame.physical_address() == physical_address)
            {
                table_frames.remove(index);
            }
        });
        let index = frames
            .iter()
            .position(|frame| frame.physical_address() == entry.physical_address)
            .ok_or(AddressSpaceError::NotFound)?;
        frames.remove(index);
        Ok(())
    }
    pub fn map_grant_page(
        &self,
        virtual_address: VAddr,
        physical_address: PhysAddr,
        flags: Flags,
    ) -> Result<(), AddressSpaceError> {
        validate_user_mapping(virtual_address, flags)?;
        let pending = self.invalidation_pending.lock();
        if pending.contains(&virtual_address) {
            return Err(AddressSpaceError::InvalidMapping);
        }
        let mut ledger = self.ledger.lock();
        drop(pending);
        if self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8
            || ledger
                .iter()
                .any(|entry| entry.virtual_address == virtual_address)
        {
            return Err(AddressSpaceError::Dying);
        }
        let mut table_frames = self.table_frames.lock();
        let mut pruned_tables = Vec::new();
        let result = map_page_retaining_pruned_tables(
            self.root.physical_address(),
            &mut table_frames,
            &mut pruned_tables,
            virtual_address,
            physical_address,
            user_flags(flags),
        );
        if let Err(error) = result {
            if crate::memory::tlb_shootdown::flush_asid_and_await(self.asid()).is_err() {
                quarantine_frames(pruned_tables, "grant map rollback invalidation unacknowledged");
                return Err(AddressSpaceError::InvalidationUnacknowledged);
            }
            return Err(error);
        }
        drop(pruned_tables);
        ledger.push(MappingEntry {
            virtual_address,
            physical_address,
            kind: MappingKind::Grant,
            flags: user_flags(flags),
        });
        Ok(())
    }

    pub fn unmap_grant_page(&self, virtual_address: VAddr) -> Result<(), AddressSpaceError> {
        let entry = self.begin_unmap(virtual_address, Some(MappingKind::Grant))?;
        while self.copy_readers.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }
        let mut table_frames = self.table_frames.lock();
        // Pruned tables are detached, not freed: the invalidate below has to
        // complete before a table frame can be handed to another root, or a remote
        // walker could read a table that has already been reused.
        let mut detached_tables = Vec::new();
        let table =
            unsafe { &mut *(phys_to_virt(self.root.physical_address()) as *mut hal::PageTable) };
        if table.unmap(virtual_address).is_err() {
            drop(table_frames);
            self.abort_unmap(entry);
            return Err(AddressSpaceError::NotFound);
        }
        table.prune_empty(virtual_address, &mut |physical_address| {
            if let Some(index) = table_frames
                .iter()
                .position(|frame| frame.physical_address() == physical_address)
            {
                detached_tables.push(table_frames.remove(index));
            }
        });
        drop(table_frames);
        if crate::memory::tlb_shootdown::flush_asid_and_await(self.asid()).is_err() {
            quarantine_frames(detached_tables, "grant-page unmap invalidation unacknowledged");
            return Err(AddressSpaceError::InvalidationUnacknowledged);
        }
        drop(detached_tables);
        self.finish_unmap(virtual_address);
        Ok(())
    }

    pub fn retire(&self) {
        self.state
            .store(AddressSpaceState::Dying as u8, Ordering::Release);
    }
}

/// Build a Tier 2 domain address space covering the kernel supervisor mapping,
/// the cell's own kernel stack, its user stack, and its loaded ELF segments.
pub fn create_cell_domain(
    kstack: &crate::task::stack::Stack,
    ustack: &crate::task::stack::Stack,
    segments: &crate::task::stack::CellSegments,
) -> Result<Arc<AddressSpace>, AddressSpaceError> {
    let mut builder = AddressSpaceBuilder::new();
    builder.map_registered_execution(kstack);

    // Map user stack with User Read+Write permissions
    let ustack_flags = Flags::from_bits(Flags::READ | Flags::WRITE);
    for addr in (ustack.usable_start()..ustack.top).step_by(PAGE_SIZE) {
        let phys =
            crate::memory::paging::virt_to_phys(addr).ok_or(AddressSpaceError::InvalidMapping)?;
        builder.map_existing_user_page(addr, phys, MappingKind::Private, ustack_flags)?;
    }

    // Map cell ELF segments with User permissions
    for &(va, _frame) in segments.pages() {
        let is_write = segments.is_writable(va);
        let (kind, flags) = if is_write {
            (
                MappingKind::Private,
                Flags::from_bits(Flags::READ | Flags::WRITE),
            )
        } else {
            (
                MappingKind::ImmutableImage,
                Flags::from_bits(Flags::READ | Flags::EXECUTE),
            )
        };
        let phys =
            crate::memory::paging::virt_to_phys(va).ok_or(AddressSpaceError::InvalidMapping)?;
        builder.map_existing_user_page(va, phys, kind, flags)?;
    }
    builder.build()
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        let acked = self.asid.release().is_ok();
        #[cfg(feature = "test-hooks")]
        for id in self.supervisor_registrations.drain(..) {
            let unregistered = crate::memory::domain_supervisor_registry::unregister(id);
            assert!(unregistered);
        }
        // SAFETY: Drop runs once and ManuallyDrop suppresses the field destructor.
        let root = unsafe { core::mem::ManuallyDrop::take(&mut self.root) };
        if acked {
            drop(root);
        } else {
            quarantine_frames(self.frames.lock().drain(..).collect(), "root teardown leaves");
            quarantine_frames(
                self.table_frames.lock().drain(..).collect(),
                "root teardown tables",
            );
            quarantine_frames(alloc::vec![root], "root teardown root");
        }
    }
}

/// Largest number of private roots that may hold an architectural tag at once.
///
/// A fixed pool makes exhaustion a bounded, checkable condition instead of a
/// counter wrap that reissues a live tag. Every value handed out is `slot + 1`, so
/// tag 0 stays reserved for "no architectural tag" (full-flush mode) and no value
/// can exceed the narrowest supported width.
const MAX_LIVE_ASIDS: usize = 256;

/// The identity holding one live tag, for diagnostics and for the generation-tagged
/// acknowledgement the non-RV64 switch still owes phase 02.
#[derive(Clone, Copy)]
struct AsidTagOwner {
    domain: u64,
}

static LIVE_ASIDS: Spinlock<[Option<AsidTagOwner>; MAX_LIVE_ASIDS]> =
    Spinlock::new([None; MAX_LIVE_ASIDS]);

/// Test-hooks view: which domain currently holds tag `value`, or `None` when the
/// tag is free. The lease contract test uses it to prove a released tag is
/// reissued to its new owner rather than aliasing the previous one.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) fn live_tag_owner(value: usize) -> Option<u64> {
    if value == 0 || value > MAX_LIVE_ASIDS {
        return None;
    }
    LIVE_ASIDS.lock()[value - 1].map(|owner| owner.domain)
}

/// An architectural tag owned by exactly one live private root.
///
/// The value is never reissued while another root holds it. A published root
/// releases its tag before its frames; a standalone lease releases it on Drop.
/// Both wait for local/remote invalidation, retaining the slot on failure.
struct AsidLease {
    value: usize,
    slot: usize,
    domain: u64,
    released: bool,
}

impl AsidLease {
    fn acquire(domain: u64) -> Option<Self> {
        let mut tags = LIVE_ASIDS.lock();
        let slot = tags.iter().position(Option::is_none)?;
        let value = slot + 1;
        // Never hand out a tag the root register cannot carry. The pool is sized so
        // this cannot trigger today (256 < 2^12), and the guard is what keeps that
        // true if the pool grows: a value outside the width would be truncated or
        // ignored by hardware, which is aliasing under another name.
        if value >= 1usize << asid_width() {
            return None;
        }
        tags[slot] = Some(AsidTagOwner { domain });
        Some(Self {
            value,
            slot,
            domain,
            released: false,
        })
    }
    /// Release the tag once, preserving its reservation on an unacknowledged
    /// flush. AddressSpace calls this before dropping any owned page-table frame;
    /// standalone leases use Drop for the same tag-reuse ordering.
    fn release(&mut self) -> Result<(), crate::memory::tlb_shootdown::FlushAckError> {
        if self.released {
            return Ok(());
        }
        self.released = true;
        if let Err(error) = crate::memory::tlb_shootdown::flush_asid_and_await(self.value) {
            log::error!(
                "[asid] tag {} for domain {} not recycled: invalidation unacknowledged ({:?})",
                self.value,
                self.domain,
                error
            );
            return Err(error);
        }
        let mut tags = LIVE_ASIDS.lock();
        match tags[self.slot] {
            Some(owner) if owner.domain == self.domain => tags[self.slot] = None,
            other => log::warn!(
                "[asid] tag {} slot {} released by domain {} but held by {:?} — slot retained",
                self.value,
                self.slot,
                self.domain,
                other.map(|owner| owner.domain)
            ),
        }
        Ok(())
    }
}

impl Drop for AsidLease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

fn validate_user_mapping(virtual_address: VAddr, flags: Flags) -> Result<(), AddressSpaceError> {
    if virtual_address >= USER_LIMIT || !virtual_address.is_multiple_of(PAGE_SIZE) {
        return Err(AddressSpaceError::InvalidMapping);
    }
    if flags.bits() & Flags::WRITE != 0 && flags.bits() & Flags::EXECUTE != 0 {
        return Err(AddressSpaceError::WriteExecute);
    }
    Ok(())
}
#[cfg(feature = "test-hooks")]
fn register_private_table_frames(
    root: &OwnedFrame,
    table_frames: &[OwnedFrame],
    owner: u64,
) -> Result<Vec<crate::memory::domain_supervisor_registry::SupervisorRangeId>, AddressSpaceError> {
    use crate::memory::domain_supervisor_registry::{
        is_active, register, unregister, SupervisorRangeKind, SupervisorRangeOwner,
    };

    if !is_active() {
        return Ok(Vec::new());
    }
    let mut registrations = Vec::with_capacity(table_frames.len() + 1);
    for physical_address in core::iter::once(root.physical_address())
        .chain(table_frames.iter().map(OwnedFrame::physical_address))
    {
        match register(
            physical_address,
            physical_address + PAGE_SIZE,
            SupervisorRangeKind::PrivatePageTable,
            SupervisorRangeOwner::AddressSpace(owner),
        ) {
            Ok(id) => registrations.push(id),
            Err(()) => {
                for id in registrations.drain(..) {
                    let unregistered = unregister(id);
                    debug_assert!(unregistered);
                }
                return Err(AddressSpaceError::OutOfMemory);
            }
        }
    }
    Ok(registrations)
}

fn user_flags(flags: Flags) -> Flags {
    Flags::from_bits(flags.bits() | Flags::VALID | Flags::USER | Flags::ACCESSED | Flags::DIRTY)
}
fn map_page(
    root: PhysAddr,
    table_frames: &mut Vec<OwnedFrame>,
    virtual_address: VAddr,
    physical_address: PhysAddr,
    flags: Flags,
) -> Result<(), AddressSpaceError> {
    map_page_inner(
        root,
        table_frames,
        None,
        virtual_address,
        physical_address,
        flags,
    )
}

fn map_page_retaining_pruned_tables(
    root: PhysAddr,
    table_frames: &mut Vec<OwnedFrame>,
    pruned_table_frames: &mut Vec<OwnedFrame>,
    virtual_address: VAddr,
    physical_address: PhysAddr,
    flags: Flags,
) -> Result<(), AddressSpaceError> {
    map_page_inner(
        root,
        table_frames,
        Some(pruned_table_frames),
        virtual_address,
        physical_address,
        flags,
    )
}

fn map_page_inner(
    root: PhysAddr,
    table_frames: &mut Vec<OwnedFrame>,
    mut pruned_table_frames: Option<&mut Vec<OwnedFrame>>,
    virtual_address: VAddr,
    physical_address: PhysAddr,
    flags: Flags,
) -> Result<(), AddressSpaceError> {
    #[cfg(feature = "test-hooks")]
    if FAIL_NEXT_MAP.swap(0, Ordering::AcqRel) != 0 {
        return Err(AddressSpaceError::OutOfMemory);
    }
    // SAFETY: root is private and its page-table frame remains owned for this call.
    let table = unsafe { &mut *(phys_to_virt(root) as *mut hal::PageTable) };
    let result = {
        let mut allocate_table = || {
            let frame = allocate_owned_frame().ok()?;
            let physical_address = frame.physical_address();
            table_frames.push(frame);
            Some(physical_address)
        };
        table.map(
            virtual_address,
            physical_address,
            flags,
            &mut allocate_table,
        )
    };
    if result.is_err() {
        table.prune_empty(virtual_address, &mut |physical_address| {
            if let Some(index) = table_frames
                .iter()
                .position(|frame| frame.physical_address() == physical_address)
            {
                let frame = table_frames.remove(index);
                if let Some(pruned) = pruned_table_frames.as_deref_mut() {
                    pruned.push(frame);
                }
            }
        });
        return Err(AddressSpaceError::OutOfMemory);
    }
    Ok(())
}

fn unmap_existing_page(
    root: PhysAddr,
    table_frames: &mut Vec<OwnedFrame>,
    pruned_table_frames: &mut Vec<OwnedFrame>,
    virtual_address: VAddr,
) {
    // SAFETY: this address space owns its private root and the caller retains
    // pruned page-table frames until its translation teardown is complete.
    let table = unsafe { &mut *(phys_to_virt(root) as *mut hal::PageTable) };
    let _ = table.unmap(virtual_address);
    table.prune_empty(virtual_address, &mut |physical_address| {
        if let Some(index) = table_frames
            .iter()
            .position(|frame| frame.physical_address() == physical_address)
        {
            pruned_table_frames.push(table_frames.remove(index));
        }
    });
}

#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) fn fail_allocation_after(count: usize) {
    FAIL_ALLOCATION_AFTER.store(count, Ordering::Release);
}
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) fn fail_next_map() {
    FAIL_NEXT_MAP.store(1, Ordering::Release);
}
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[path = "address_space_tests.rs"]
pub(crate) mod address_space_tests;

/// Tag-width and CR3-composition policy. These run in the kernel's host lane on
/// x86_64, where the same `hal::domain::cr3_for` the boot path calls is compiled
/// — so the policy is executed, not merely inspected.
#[cfg(all(test, target_arch = "x86_64"))]
mod tag_policy_tests {
    use super::asid_width;

    const ROOT: usize = 0x1_0000_0000;

    #[test]
    fn x86_tag_width_is_the_pcid_width() {
        assert_eq!(asid_width(), 12);
    }

    #[test]
    fn a_tag_is_dropped_rather_than_faulted_when_pcid_is_unusable() {
        // CR4.PCIDE=0 with a nonzero PCID in CR3 is #GP, so the fail-closed value
        // is the untagged root.
        assert_eq!(hal::domain::cr3_for(ROOT, 0x5A5, false), ROOT);
        assert_eq!(hal::domain::cr3_for(ROOT, 0x5A5, false) & 0xFFF, 0);
    }

    #[test]
    fn a_tag_is_carried_and_masked_when_pcid_is_usable() {
        assert_eq!(hal::domain::cr3_for(ROOT, 7, true), ROOT | 7);
        assert_eq!(hal::domain::cr3_for(ROOT, 0x1234, true), ROOT | 0x234);
    }

    #[test]
    fn root_address_bits_are_preserved_and_low_bits_belong_to_the_tag() {
        assert_eq!(hal::domain::cr3_for(ROOT | 0xABC, 3, true), ROOT | 3);
        assert_eq!(hal::domain::cr3_for(ROOT | 0xABC, 3, false), ROOT);
    }

    #[test]
    fn tag_zero_is_identical_in_both_modes() {
        assert_eq!(
            hal::domain::cr3_for(ROOT, 0, true),
            hal::domain::cr3_for(ROOT, 0, false)
        );
    }
}
