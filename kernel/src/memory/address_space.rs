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
const ASID_MASK: usize = 0xffff;
static NEXT_DOMAIN: AtomicU64 = AtomicU64::new(1);
static NEXT_ASID: AtomicUsize = AtomicUsize::new(1);
static ASID_EPOCH: AtomicU64 = AtomicU64::new(0);
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
pub(crate) struct SupervisorMapping {
    virtual_address: VAddr,
    physical_address: PhysAddr,
    flags: Flags,
}
impl SupervisorMapping {
    #[cfg(feature = "test-hooks")]
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
            supervisor,
            requests,
            existing_user,
        } = self;
        let root = allocate_owned_frame()?;
        // SAFETY: root is a zeroed private page frame and PageTable has the same page layout.
        unsafe {
            core::ptr::write(
                phys_to_virt(root.physical_address()) as *mut hal::PageTable,
                hal::PageTable::empty(),
            );
        }
        let mut table_frames = Vec::new();
        let mut frames = Vec::new();
        let mut ledger = Vec::new();
        for mapping in supervisor {
            map_page(
                root.physical_address(),
                &mut table_frames,
                mapping.virtual_address,
                mapping.physical_address,
                mapping.flags,
            )?;
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
            asid: AsidLease::acquire(),
            state: AtomicU8::new(AddressSpaceState::Live as u8),
            ledger: Spinlock::new(ledger),
            copy_readers: AtomicUsize::new(0),
            current_harts: AtomicUsize::new(0),
            table_frames: Spinlock::new(table_frames),
            frames: Spinlock::new(frames),
            #[cfg(feature = "test-hooks")]
            supervisor_registrations,
            root,
        }))
    }
}

/// A published private root. The root field is declared last so table/page frames drop first.
pub struct AddressSpace {
    identity: DomainId,
    generation: u64,
    asid: AsidLease,
    state: AtomicU8,
    ledger: Spinlock<Vec<MappingEntry>>,
    copy_readers: AtomicUsize,
    current_harts: AtomicUsize,
    frames: Spinlock<Vec<OwnedFrame>>,
    /// Intermediate Sv39 page-table frames, separate from user mappings.
    table_frames: Spinlock<Vec<OwnedFrame>>,
    /// Registry tokens retire before the root/page tables return to the allocator.
    #[cfg(feature = "test-hooks")]
    supervisor_registrations: Vec<crate::memory::domain_supervisor_registry::SupervisorRangeId>,
    root: OwnedFrame,
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
    pub fn map_private_page(
        &self,
        virtual_address: VAddr,
        kind: MappingKind,
        flags: Flags,
    ) -> Result<(), AddressSpaceError> {
        validate_user_mapping(virtual_address, flags)?;
        let mut ledger = self.ledger.lock();
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
        map_page(
            self.root.physical_address(),
            &mut table_frames,
            virtual_address,
            page.physical_address(),
            user_flags(flags),
        )?;
        ledger.push(MappingEntry {
            virtual_address,
            physical_address: page.physical_address(),
            kind,
            flags: user_flags(flags),
        });
        frames.push(page);
        Ok(())
    }
    pub fn unmap_private_page(&self, virtual_address: VAddr) -> Result<(), AddressSpaceError> {
        let entry = {
            let mut ledger = self.ledger.lock();
            let position = ledger
                .iter()
                .position(|entry| entry.virtual_address == virtual_address)
                .ok_or(AddressSpaceError::NotFound)?;
            ledger.remove(position)
        };
        // Wait for all in-flight copy readers to drain before unmapping PTE and reclaiming frame.
        while self.copy_readers.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }
        let mut table_frames = self.table_frames.lock();
        let mut frames = self.frames.lock();
        // SAFETY: only this address space owns and mutates its root.
        let table =
            unsafe { &mut *(phys_to_virt(self.root.physical_address()) as *mut hal::PageTable) };
        table
            .unmap(virtual_address)
            .map_err(|_| AddressSpaceError::NotFound)?;
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
        #[cfg(target_arch = "riscv64")]
        // SAFETY: sfence.vma on a single virtual address is a pure TLB
        // invalidation from S-mode.
        unsafe {
            core::arch::asm!("sfence.vma zero, {va}", va = in(reg) virtual_address);
        }
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
        let mut ledger = self.ledger.lock();
        if self.state.load(Ordering::Acquire) != AddressSpaceState::Live as u8
            || ledger
                .iter()
                .any(|entry| entry.virtual_address == virtual_address)
        {
            return Err(AddressSpaceError::Dying);
        }
        let mut table_frames = self.table_frames.lock();
        map_page(
            self.root.physical_address(),
            &mut table_frames,
            virtual_address,
            physical_address,
            user_flags(flags),
        )?;
        ledger.push(MappingEntry {
            virtual_address,
            physical_address,
            kind: MappingKind::Grant,
            flags: user_flags(flags),
        });
        Ok(())
    }

    pub fn unmap_grant_page(&self, virtual_address: VAddr) -> Result<(), AddressSpaceError> {
        let entry = {
            let mut ledger = self.ledger.lock();
            let position = ledger
                .iter()
                .position(|entry| {
                    entry.virtual_address == virtual_address && entry.kind == MappingKind::Grant
                })
                .ok_or(AddressSpaceError::NotFound)?;
            ledger.remove(position)
        };
        while self.copy_readers.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }
        let mut table_frames = self.table_frames.lock();
        let table =
            unsafe { &mut *(phys_to_virt(self.root.physical_address()) as *mut hal::PageTable) };
        table
            .unmap(virtual_address)
            .map_err(|_| AddressSpaceError::NotFound)?;
        table.prune_empty(virtual_address, &mut |physical_address| {
            if let Some(index) = table_frames
                .iter()
                .position(|frame| frame.physical_address() == physical_address)
            {
                table_frames.remove(index);
            }
        });
        drop(table_frames);
        crate::memory::tlb_shootdown::flush_page(virtual_address);
        let _ = entry;
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
        let flags = if is_write {
            Flags::from_bits(Flags::READ | Flags::WRITE)
        } else {
            Flags::from_bits(Flags::READ | Flags::EXECUTE)
        };
        let phys =
            crate::memory::paging::virt_to_phys(va).ok_or(AddressSpaceError::InvalidMapping)?;
        builder.map_existing_user_page(va, phys, MappingKind::Private, flags)?;
    }
    builder.build()
}

#[cfg(feature = "test-hooks")]
impl Drop for AddressSpace {
    fn drop(&mut self) {
        for id in self.supervisor_registrations.drain(..) {
            let unregistered = crate::memory::domain_supervisor_registry::unregister(id);
            assert!(unregistered);
        }
    }
}

struct AsidLease {
    value: usize,
    _epoch: u64,
}
impl AsidLease {
    fn acquire() -> Self {
        let next = NEXT_ASID.fetch_add(1, Ordering::AcqRel) & ASID_MASK;
        if next == 0 {
            hal::domain::flush_all();
            let _ = hal::domain::flush_asid_remote(usize::MAX, 0);
            return Self {
                value: 1,
                _epoch: ASID_EPOCH.fetch_add(1, Ordering::AcqRel) + 1,
            };
        }
        Self {
            value: next,
            _epoch: ASID_EPOCH.load(Ordering::Acquire),
        }
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
                table_frames.remove(index);
            }
        });
        return Err(AddressSpaceError::OutOfMemory);
    }
    Ok(())
}

#[cfg(feature = "test-hooks")]
pub(crate) fn fail_allocation_after(count: usize) {
    FAIL_ALLOCATION_AFTER.store(count, Ordering::Release);
}
#[cfg(feature = "test-hooks")]
pub(crate) fn fail_next_map() {
    FAIL_NEXT_MAP.store(1, Ordering::Release);
}
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[path = "address_space_tests.rs"]
pub(crate) mod address_space_tests;
