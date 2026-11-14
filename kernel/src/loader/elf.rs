//! ELF Parsing Logic
use super::{ElfHeader, ElfParser};
use types::*;
use xmas_elf::ElfFile;

/// Placement policy for every PT_LOAD (slot confinement, null page, stack
/// guards); see the module docs.
#[path = "image-va.rs"]
pub(crate) mod image_va;

pub struct ElfLoader;

/// One 4 KiB page of a loaded cell image.
///
/// `final_flags` is the permission set the page must carry once the loader is
/// done — the ELF's `p_flags`, OR-ed across every PT_LOAD that touches the page.
/// It is deliberately NOT the set the page is mapped with during loading: see
/// [`crate::loader::wx`] for why the two differ and when they converge.
#[derive(Debug, Clone, Copy)]
pub struct LoadedPage {
    /// Page-aligned virtual address in the cell's address space.
    pub va: VAddr,
    /// Backing physical frame, reclaimed by `CellSegments` when the cell dies.
    pub frame: PhysAddr,
    /// Post-relocation permissions, applied by `wx::enforce`.
    pub final_flags: crate::memory::paging::Flags,
}

impl ElfLoader {
    /// Validate every PT_LOAD extent against the slot this spawn owns before
    /// any mapping or page-metadata allocation (see [`image_va`]). A large BSS
    /// must never spill into the next cell's still-unmapped slot, and no image
    /// may cover the null page or a stack guard page.
    ///
    /// `load_base` is added to every `p_vaddr` (PIE base, or `0` for a fixed-VA
    /// image); `slot_base` is the slot `va_alloc` handed this spawn.
    pub(super) fn preflight_pages(
        elf: &ElfFile<'_>,
        load_base: usize,
        slot_base: usize,
    ) -> ViResult<usize> {
        let slot = image_va::ImageSlot::of(slot_base).ok_or(ViError::InvalidInput)?;
        let mut pages = 0usize;
        for ph in elf.program_iter() {
            if !matches!(ph.get_type(), Ok(xmas_elf::program::Type::Load)) {
                continue;
            }
            let relative = usize::try_from(ph.virtual_addr()).map_err(|_| ViError::InvalidInput)?;
            let size = usize::try_from(ph.mem_size()).map_err(|_| ViError::InvalidInput)?;
            let start = load_base
                .checked_add(relative)
                .ok_or(ViError::InvalidInput)?;
            let end = start.checked_add(size).ok_or(ViError::InvalidInput)?;
            let segment_pages = image_va::admit_segment(
                start,
                end,
                slot,
                crate::task::stack::overlaps_stack_reservation,
            )?;
            pages = pages
                .checked_add(segment_pages)
                .ok_or(ViError::InvalidInput)?;
        }
        Ok(pages)
    }

    /// The slot a fixed-VA (ET_EXEC) image is linked into: the one containing
    /// its lowest PT_LOAD. The caller must claim it with
    /// `va_alloc::reserve_cell_va` before loading; `preflight_pages` then holds
    /// every other PT_LOAD to the same slot.
    pub fn fixed_image_slot(&self, data: &[u8]) -> ViResult<usize> {
        let elf = ElfFile::new(data).map_err(|_| ViError::InvalidInput)?;
        let mut lowest: Option<usize> = None;
        for ph in elf.program_iter() {
            if matches!(ph.get_type(), Ok(xmas_elf::program::Type::Load)) {
                let vaddr =
                    usize::try_from(ph.virtual_addr()).map_err(|_| ViError::InvalidInput)?;
                lowest = Some(lowest.map_or(vaddr, |low| low.min(vaddr)));
            }
        }
        image_va::fixed_image_slot(lowest.ok_or(ViError::InvalidInput)?)
    }

    /// Load loadable segments into memory.
    ///
    /// `load_base` is added to every segment's `p_vaddr` before mapping:
    /// - For fixed-VA cells (ET_EXEC, non-PIE): pass `0` — uses p_vaddr directly.
    /// - For PIE cells (ET_DYN): pass the VA base allocated by `va_alloc::alloc_cell_va`.
    ///
    /// `slot_base` is the `va_alloc` slot this spawn owns (the PIE base, or the
    /// slot claimed by `va_alloc::reserve_cell_va` for a fixed-VA image); every
    /// page must lie inside it.
    ///
    /// `guarded_code` is the image's BTI decision ([`super::gnu_property::arm`]):
    /// when set, every executable page's target flags carry `GUARDED`.
    ///
    /// Returns one [`LoadedPage`] per mapped page so the caller can (a) hand the
    /// frames to `CellSegments` for reclamation and (b) run the W^X lowering pass
    /// once relocations are applied.
    ///
    /// # Errors
    /// - `ViError::InvalidInput` — malformed headers (file_size > mem_size, ranges
    ///   outside the ELF buffer, arithmetic overflow) or `slot_base` not a slot.
    /// - `ViError::PermissionDenied` — a page outside the owned slot, on the null
    ///   page or a stack run, a VA already owned by another live cell or kernel
    ///   MMIO, or a PT_LOAD declaring W+X.
    /// - `ViError::OutOfMemory` — no frame available, or the mapping failed.
    ///
    /// Every invalidation the mapping needs, and on failure every page and
    /// table the unwind retires, goes to `batch`. The caller holds
    /// `FRAME_ALLOCATOR` (the `frame_allocator` borrow) and must finish `batch`
    /// once it is released, before relocating through the image or returning
    /// the spawn.
    pub fn load_segments(
        &self,
        data: &[u8],
        frame_allocator: &mut crate::memory::frame::FrameAllocator,
        batch: &mut crate::memory::frame_run::Batch,
        load_base: usize,
        slot_base: usize,
        guarded_code: bool,
    ) -> ViResult<alloc::vec::Vec<LoadedPage>> {
        let elf = ElfFile::new(data).map_err(|_| ViError::InvalidInput)?;
        // Record each mapped page so the cell's segment frames can be reclaimed
        // when it dies (see task::stack::CellSegments) — otherwise they leak — and
        // so the post-relocation W^X pass knows each page's target permissions.
        //
        // Reserve the whole bound before mapping anything: pushing grows through
        // the infallible allocator, and a growth that cannot be satisfied halts
        // the kernel instead of failing the spawn (measured 2026-10-03: the growth
        // to 512 entries / 12 288 bytes is what ended the capacity sweep). One
        // entry per mapped page; summing page-aligned PT_LOAD spans is an upper bound.
        let page_bound = Self::preflight_pages(&elf, load_base, slot_base)?;
        let mut mapped: alloc::vec::Vec<LoadedPage> = alloc::vec::Vec::new();
        mapped
            .try_reserve_exact(page_bound)
            .map_err(|_| ViError::OutOfMemory)?;

        for (seg_index, ph) in elf.program_iter().enumerate() {
            if let Ok(xmas_elf::program::Type::Load) = ph.get_type() {
                let file_offset = usize::try_from(ph.offset())
                    .map_err(|_| ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?;
                // For PIE cells load_base relocates every segment; for fixed-VA
                // cells load_base == 0 so p_vaddr is used verbatim.
                let segment_vaddr = usize::try_from(ph.virtual_addr())
                    .map_err(|_| ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?;
                let vaddr = segment_vaddr
                    .checked_add(load_base)
                    .ok_or(ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?;
                let mem_size = usize::try_from(ph.mem_size())
                    .map_err(|_| ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?;
                let file_size = usize::try_from(ph.file_size())
                    .map_err(|_| ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?;
                let ph_flags = ph.flags();

                // --- Header sanity checks ---
                // W+X is refused before a single frame is allocated: a cell must
                // not be able to opt out of W^X by declaring writable code.
                super::wx::reject_wx_segment(
                    seg_index,
                    vaddr,
                    ph_flags.is_write(),
                    ph_flags.is_execute(),
                )
                .inspect_err(|_| Self::unwind(&mapped, batch))?;

                // file_size MUST NOT exceed mem_size (the rest is BSS).
                if file_size > mem_size {
                    log::error!(
                        "ELF: rejecting segment with file_size={} > mem_size={}",
                        file_size,
                        mem_size
                    );
                    Self::unwind(&mapped, batch);
                    return Err(ViError::InvalidInput);
                }
                // file_offset + file_size must fit inside the ELF buffer.
                let file_end = file_offset
                    .checked_add(file_size)
                    .ok_or(ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?;
                if file_end > data.len() {
                    log::error!(
                        "ELF: segment file range {}..{} exceeds buffer len {}",
                        file_offset,
                        file_end,
                        data.len()
                    );
                    Self::unwind(&mapped, batch);
                    return Err(ViError::InvalidInput);
                }
                // vaddr + mem_size must not overflow; preflight already held the
                // extent to the owned slot, which lies below `USER_VADDR_MAX`.
                let end_addr = vaddr
                    .checked_add(mem_size)
                    .ok_or(ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?;

                let start_addr = vaddr;

                // Align start/end to page boundaries
                let start_page = start_addr & !(4096 - 1);
                let end_page = end_addr
                    .checked_add(4095)
                    .ok_or(ViError::InvalidInput)
                    .inspect_err(|_| Self::unwind(&mapped, batch))?
                    & !(4096 - 1);

                // --- Translate ELF p_flags to page-table flags ---
                // p_flags bits: 0x1=X, 0x2=W, 0x4=R.
                //
                // TWO flag sets per segment, and the difference is the whole point:
                //
                //   final_flags — what the ELF actually asks for. Recorded per page,
                //                 OR-ed across PT_LOADs that share a boundary page,
                //                 and applied by `wx::enforce` after relocation.
                //   load_flags  — final_flags plus WRITE. Used only while the loader
                //                 runs, because `.rela.dyn` patching writes through
                //                 the cell's own VA on riscv64/x86_64.
                //
                // The WRITE bit therefore lives for exactly the load window. After
                // `wx::enforce` a cell's `.text`/`.rodata` are hardware read-only,
                // which is what stops one cell rewriting another's code in the SAS.
                use crate::memory::paging::Flags;
                let final_flags =
                    super::wx::page_flags(ph_flags.is_write(), ph_flags.is_execute(), guarded_code);
                let flags = Flags::from_bits(final_flags.bits() | Flags::WRITE);

                // Map pages. The invalidation is batched (phase 07b R1): every
                // leaf of this segment is written through `map_page_deferred`
                // under the caller's FRAME_ALLOCATOR guard, and
                // `flush_deferred_maps` adds [start_page, end_page) — every page
                // the loop touches, the merged boundary pages included — to the
                // caller's batch, whose one shared flush runs after the guard is
                // released and before relocation (which writes through the
                // cell's VA) or the spawn's return. An early return goes through
                // `unwind`, which unmaps every recorded page and retires its
                // frame into the batch, freed only after that flush confirms.
                let mut current_page = start_page;
                while current_page < end_page {
                    // Overwrite guard: reject VA collision with kernel MMIO or a
                    // *different* live cell. Allow shared pages from *this* cell's own
                    // adjacent PT_LOAD segments (already_ours == true).
                    //
                    // PIE linker scripts may place adjacent sections (e.g. .text R-X
                    // and .rodata R--) at non-page-aligned offsets so the two segments
                    // share the same physical page.  In that case already_ours==true and
                    // we reuse the frame, merging the new segment's flags into the page
                    // rather than allocating a fresh one or blindly overwriting flags.
                    let already_ours = mapped.iter().any(|p| p.va == current_page);
                    if !already_ours && crate::memory::paging::virt_to_phys(current_page).is_some()
                    {
                        log::error!(
                            "ELF: load VA 0x{:X} already mapped — rejecting spawn (VA collision with a live cell or kernel MMIO; fix the cell's linker script)",
                            current_page
                        );
                        Self::unwind(&mapped, batch);
                        return Err(ViError::PermissionDenied);
                    }

                    // Get the backing frame's kernel-accessible VA for copying.
                    let frame_virt = if already_ours {
                        // Shared page from an earlier LOAD segment (e.g. .rodata R-- and
                        // .data/RELRO RW- sharing a boundary page).  Reuse the existing
                        // frame but OR the new segment's permission bits in so the page
                        // satisfies both segments' access requirements.
                        let phys = crate::memory::paging::virt_to_phys(current_page)
                            .expect("already_ours but virt_to_phys returned None");
                        if let Some(entry) = mapped.iter_mut().find(|p| p.va == current_page) {
                            // Merge the TARGET flags — this is the value `wx::enforce`
                            // will apply, so the OR must happen here, on the page, and
                            // never per-segment after the fact.
                            let merged =
                                Flags::from_bits(entry.final_flags.bits() | final_flags.bits());
                            if merged != entry.final_flags {
                                entry.final_flags = merged;
                                let _ = crate::memory::paging::map_page_deferred(
                                    frame_allocator,
                                    batch,
                                    current_page,
                                    phys,
                                    Flags::from_bits(merged.bits() | Flags::WRITE),
                                );
                            }
                        }
                        crate::memory::frame::phys_to_virt(phys)
                    } else {
                        let buf_frame = match frame_allocator.allocate_frame() {
                            Some(f) => f,
                            None => {
                                log::warn!(
                                    "[loader] segment frame allocation failed: va={:#x} mapped_pages={}",
                                    current_page,
                                    mapped.len()
                                );
                                Self::unwind(&mapped, batch);
                                return Err(ViError::OutOfMemory);
                            }
                        };

                        if crate::memory::paging::map_page_deferred(
                            frame_allocator,
                            batch,
                            current_page,
                            buf_frame,
                            flags,
                        )
                        .is_err()
                        {
                            log::warn!(
                                "[loader] segment page-table allocation failed: va={:#x} mapped_pages={}",
                                current_page,
                                mapped.len()
                            );
                            // buf_frame is not in `mapped` yet — free it separately.
                            frame_allocator.deallocate_frame(buf_frame);
                            Self::unwind(&mapped, batch);
                            return Err(ViError::OutOfMemory);
                        }

                        // Track for reclamation on cell death.
                        mapped.push(LoadedPage {
                            va: current_page,
                            frame: buf_frame,
                            final_flags,
                        });

                        // Zero the frame first (simplifies BSS and padding, and
                        // prevents info-leak from previous frame owner).
                        // Use phys_to_virt: on RISC-V it's a no-op (identity map);
                        // on x86_64 physical RAM is only accessible via HHDM_BASE+phys.
                        let fv = crate::memory::frame::phys_to_virt(buf_frame);
                        unsafe {
                            core::ptr::write_bytes(fv as *mut u8, 0, 4096);
                        }
                        fv
                    };

                    // Intersection of [page, page+4096) AND [vaddr, vaddr+file_size)
                    let page_start_vaz = current_page;
                    let page_end_vaz = current_page + 4096;
                    let copy_start_v = core::cmp::max(page_start_vaz, vaddr);
                    let copy_end_v = core::cmp::min(page_end_vaz, vaddr + file_size);

                    if copy_start_v < copy_end_v {
                        let len = copy_end_v - copy_start_v;
                        let dst_offset = copy_start_v - page_start_vaz;
                        let src_offset_in_file = file_offset + (copy_start_v - vaddr);
                        // file_end was already validated above; this guards
                        // arithmetic on `len` from any rounding surprise.
                        let src_end = src_offset_in_file
                            .checked_add(len)
                            .ok_or(ViError::InvalidInput)
                            .inspect_err(|_| Self::unwind(&mapped, batch))?;
                        if src_end <= data.len() {
                            let src = &data[src_offset_in_file..src_end];
                            unsafe {
                                let dst = (frame_virt as *mut u8).add(dst_offset);
                                core::ptr::copy_nonoverlapping(src.as_ptr(), dst, len);
                            }
                        }
                    }

                    #[cfg(feature = "test-hooks")]
                    crate::loader::atomic_publication_tests::observe_unpublished_pages(&mapped);

                    if let Err(error) = crate::loader::atomic_checkpoint("AP-02") {
                        Self::unwind(&mapped, batch);
                        return Err(error);
                    }

                    current_page += 4096;
                }
                crate::memory::paging::flush_deferred_maps(batch, start_page, end_page);

                log::info!(
                    "ELF LOAD: 0x{:X}-0x{:X} flags={}{}{}",
                    start_addr,
                    end_addr,
                    if ph_flags.is_read() { 'R' } else { '-' },
                    if ph_flags.is_write() { 'W' } else { '-' },
                    if ph_flags.is_execute() { 'X' } else { '-' },
                );
            }
        }
        Ok(mapped)
    }

    /// Undo a partially loaded image: unmap every page recorded so far and
    /// retire its frame, and every table it emptied, into `batch`.
    ///
    /// Every early return inside `load_segments` must call this. Without it a
    /// rejected ELF leaves its already-mapped pages resident, which both leaks
    /// the frames and poisons the VA-collision guard for the next spawn at the
    /// same base. The batch frees them only after one shared flush covering
    /// the leaves and the detached tables confirms (census P10: the tables'
    /// remote paging-structure caches are covered too, not only this hart's).
    pub(crate) fn unwind(mapped: &[LoadedPage], batch: &mut crate::memory::frame_run::Batch) {
        use crate::memory::frame_run::{FrameRun, KernelAlias, RunKind, RunOwner};
        for page in mapped {
            let _ = crate::memory::paging::unmap_page(batch, page.va);
            batch.invalidate(page.va);
        }
        for page in mapped {
            crate::memory::paging::prune_empty_tables(page.va).retire_into(batch, page.va);
            batch.retire(FrameRun::unpublished(
                page.frame,
                1,
                RunOwner::Kernel,
                RunKind::Leaf,
                KernelAlias::Intact,
            ));
        }
    }
}

impl ElfParser for ElfLoader {
    fn parse_header(&self, data: &[u8]) -> ViResult<ElfHeader> {
        let elf = ElfFile::new(data).map_err(|_| ViError::InvalidInput)?;

        // Verify architecture (RISC-V 64)
        // Header check is implicit in successful new(), but specific machine check?
        // elf.header.pt2.machine() == xmas_elf::header::Machine::RISC_V

        Ok(ElfHeader {
            entry: elf.header.pt2.entry_point() as usize,
            shoff: elf.header.pt2.sh_offset() as usize,
        })
    }

    fn get_section<'a>(&self, data: &'a [u8], name: &str) -> ViResult<&'a [u8]> {
        let elf = ElfFile::new(data).map_err(|_| ViError::InvalidInput)?;
        match elf.find_section_by_name(name) {
            Some(section) => Ok(section.raw_data(&elf)),
            None => Err(ViError::NotFound),
        }
    }
}
