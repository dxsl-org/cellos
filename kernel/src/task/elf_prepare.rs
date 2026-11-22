//! Fallible ELF construction owned independently of scheduler publication.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use types::{CellId, ViError};

/// Every allocation and mapping needed by an unpublished ELF task.
pub struct PreparedElfTask {
    name: String,
    requested_cell_id: CellId,
    allowed_drivers: Vec<usize>,
    kstack: super::stack::Stack,
    ustack: super::stack::Stack,
    segments: crate::memory::cell_segments::CellSegments,
    entry: usize,
    load_base: usize,
    /// Entry stack pointer: the Cell initial stack block (CP1 AX-2) on 64-bit
    /// targets, the bare user stack top elsewhere.
    user_sp: usize,
    /// Initial `IA32_U_CET` (x86_64 CET-IBT opt-in); 0 everywhere else.
    user_cet: u64,
}

impl PreparedElfTask {
    /// The task of incarnation `generation`, minted before its quota row.
    pub(super) fn into_task(
        self,
        id: usize,
        cell_id: CellId,
        generation: u64,
    ) -> Result<(alloc::boxed::Box<super::Task>, usize), ViError> {
        let mut task = crate::memory::heap::try_box(super::Task::of_incarnation(
            id,
            cell_id,
            generation,
            &self.name,
            self.allowed_drivers,
        ))?;
        task.kernel_stack = Some(self.kstack);
        task.user_stack = Some(self.ustack);
        task.segment_mem = Some(self.segments);
        task.user_cet = self.user_cet;
        super::prime_user_mode_entry_at(&mut task, self.entry, 0, self.user_sp);
        Ok((task, self.load_base))
    }

    /// Where the image and the user stack landed: (PIE load base, `0` for a
    /// fixed-VA image; user-stack slot base).
    pub(crate) fn placement(&self) -> (usize, usize) {
        (self.load_base, self.ustack.base)
    }

    pub(super) fn requested_cell_id(&self) -> CellId {
        self.requested_cell_id
    }
}

/// `Iterator::collect` allocates through the infallible allocator: a full heap
/// halts the kernel instead of refusing the spawn. Every vector here is sized by
/// the ELF's page count (4 KiB for `/bin/bench-probe`), so reserve fallibly and
/// fill — the spawn then returns `ViError::OutOfMemory` like any other capacity
/// refusal. Measured 2026-10-03: these collects were what ended the capacity
/// sweep once the spawn path's larger transients stopped binding.
fn collect_reserved<T>(iter: impl ExactSizeIterator<Item = T>) -> Result<Vec<T>, ViError> {
    let mut out = Vec::new();
    out.try_reserve_exact(iter.len())
        .map_err(|_| ViError::OutOfMemory)?;
    for item in iter {
        out.push(item);
    }
    Ok(out)
}

/// Own the image's VA slot before any subsequent fallible preparation step.
/// An unwind hands the slot to its last retired run instead
/// ([`Self::hold_in`]), so a VA a remote hart may still translate is not
/// handed out again before the unwind's flush confirms (census risk 2).
struct PreparationVa(core::cell::Cell<usize>);
impl PreparationVa {
    /// Keep the slot reserved until the runs `batch` retired are free; the
    /// guard no longer releases it then.
    fn hold_in(&self, batch: &mut crate::memory::frame_run::Batch) {
        let slot = self.0.get();
        if slot != 0
            && batch.hold_va(crate::memory::frame_run::VaHold {
                release: crate::memory::cell_segments::release_cell_va,
                start: slot,
                end: slot,
            })
        {
            self.0.set(0);
        }
    }
}
impl Drop for PreparationVa {
    fn drop(&mut self) {
        let slot = self.0.get();
        if slot != 0 {
            crate::loader::va_alloc::free_cell_va(slot);
        }
    }
}

/// LoadedPage is metadata, so bridge its mappings into CellSegments ownership
/// without another allocation. This guard must be declared after the page Vec
/// and the VA guard.
struct PreparationMappings<'a>(
    Option<&'a [crate::loader::elf::LoadedPage]>,
    &'a PreparationVa,
);
impl Drop for PreparationMappings<'_> {
    fn drop(&mut self) {
        if let Some(pages) = self.0.take() {
            let mut batch = crate::memory::frame_run::Batch::new();
            crate::loader::ElfLoader::unwind(pages, &mut batch);
            self.1.hold_in(&mut batch);
            let _ = batch.complete();
        }
    }
}

/// Parse, map, relocate, protect, and stack an ELF without touching scheduler state.
/// The root user stack has the kernel stack's size, [`super::stack_pages_for`].
pub fn prepare_elf_task(
    data: &[u8],
    name: &str,
    requested_cell_id: CellId,
    allowed_drivers: Vec<usize>,
) -> Result<PreparedElfTask, ViError> {
    prepare_elf_task_with_user_stack(
        data,
        name,
        requested_cell_id,
        allowed_drivers,
        super::stack_pages_for(name),
    )
}

/// [`prepare_elf_task`] with a root user stack of `user_stack_pages` usable
/// pages (the caller's [`super::user_stack_pages_for`] decision).
pub fn prepare_elf_task_with_user_stack(
    data: &[u8],
    name: &str,
    requested_cell_id: CellId,
    allowed_drivers: Vec<usize>,
    user_stack_pages: usize,
) -> Result<PreparedElfTask, ViError> {
    use crate::loader::{ElfLoader, ElfParser};

    if data.len() < 4 || &data[..4] != b"\x7fELF" {
        return Err(ViError::InvalidInput);
    }
    let aligned = crate::loader::aligned_elf::bytes(data)?;
    let elf_data = aligned.as_ref();

    let loader = ElfLoader;
    let header = loader.parse_header(elf_data)?;
    // Forward-edge CFI opt-in, read from the admitted bytes before any mapping
    // so a malformed property refuses the spawn with nothing to unwind.
    let cfi = crate::loader::gnu_property::arm(
        crate::loader::gnu_property::forward_edge_cfi(elf_data)?,
        name,
    );
    // Every image owns exactly one VA slot and may map only inside it: a PIE
    // takes any free slot; a fixed-VA image claims the slot it is linked into
    // and is refused when another spawn holds it, so no image page can land
    // on the null page, a stack guard or a foreign slot.
    let (load_base, slot_base) = if super::elf_is_pie(elf_data) {
        let base = crate::loader::va_alloc::alloc_cell_va().ok_or(ViError::OutOfMemory)?;
        (base, base)
    } else {
        let slot = loader.fixed_image_slot(elf_data)?;
        if !crate::loader::va_alloc::reserve_cell_va(slot) {
            log::error!("ELF: fixed-VA image slot {slot:#x} is owned by another cell");
            return Err(ViError::PermissionDenied);
        }
        (0, slot)
    };
    let va_rollback = PreparationVa(core::cell::Cell::new(slot_base));
    crate::loader::atomic_checkpoint("AP-01")?;

    let mut load_batch = crate::memory::frame_run::Batch::new();
    let loaded = crate::memory::frame::with(|allocator| {
        loader.load_segments(
            elf_data,
            allocator,
            &mut load_batch,
            load_base,
            slot_base,
            cfi.guarded_code,
        )
    });
    // A failed load unwound its pages into the batch: they hold the slot.
    if !matches!(loaded, Some(Ok(_))) {
        va_rollback.hold_in(&mut load_batch);
    }
    // One shared flush for the whole image (or for its unwind), after the frame
    // allocator is released and before anything relocates through it.
    let flushed = load_batch.complete();
    let seg_pages = loaded.ok_or(ViError::OutOfMemory)??;
    let mut mapping_rollback = PreparationMappings(Some(&seg_pages), &va_rollback);
    if !flushed.is_confirmed() {
        log::error!("ELF: image mapping invalidation unconfirmed — refusing the spawn");
        return Err(ViError::OutOfMemory);
    }
    #[cfg(feature = "test-hooks")]
    crate::loader::atomic_publication_tests::observe_unpublished_pages(&seg_pages);
    let final_flags = crate::loader::atomic_checkpoint("AP-RF")
        .map_err(|_| ViError::OutOfMemory)
        .and_then(|_| collect_reserved(seg_pages.iter().map(|p| (p.va, p.final_flags))))
        .inspect_err(|_| {
            #[cfg(feature = "p01-trace")]
            p01_post_load_failure(&seg_pages, load_base, 2);
        })?;
    let mapped_pages = crate::loader::atomic_checkpoint("AP-RM")
        .map_err(|_| ViError::OutOfMemory)
        .and_then(|_| collect_reserved(seg_pages.iter().map(|p| (p.va, p.frame))))
        .inspect_err(|_| {
            #[cfg(feature = "p01-trace")]
            p01_post_load_failure(&seg_pages, load_base, 3);
        })?;
    let writable_count = seg_pages
        .iter()
        .filter(|p| p.final_flags.bits() & crate::memory::paging::Flags::WRITE != 0)
        .count();
    let mut writable_pages = Vec::new();
    crate::loader::atomic_checkpoint("AP-RW")
        .map_err(|_| ViError::OutOfMemory)
        .and_then(|_| {
            writable_pages
                .try_reserve_exact(writable_count)
                .map_err(|_| ViError::OutOfMemory)
        })
        .map_err(|_| {
            #[cfg(feature = "p01-trace")]
            p01_post_load_failure(&seg_pages, load_base, 4);
            ViError::OutOfMemory
        })?;
    for page in seg_pages.iter() {
        if page.final_flags.bits() & crate::memory::paging::Flags::WRITE != 0 {
            writable_pages.push(page.va);
        }
    }
    #[cfg(feature = "p01-trace")]
    {
        // These exclusive backing arrays survive in CellSegments. Other loader
        // scratch/shared-helper allocations retain origin-only classification.
        crate::memory::heap::p01_owner::adopt_vec(&mapped_pages);
        crate::memory::heap::p01_owner::adopt_vec(&writable_pages);
    }
    let segments = crate::memory::cell_segments::CellSegments::with_writable_pages(
        mapped_pages,
        writable_pages,
        slot_base,
    )
    .with_guarded_code(cfi.guarded_code);
    // Constructor is infallible: the segment owner now exclusively owns both
    // frame reclamation and PIE release, including every later failure edge.
    mapping_rollback.0 = None;
    va_rollback.0.set(0);
    #[cfg(feature = "test-hooks")]
    crate::loader::atomic_publication_tests::observe_unpublished_segments(&segments);
    crate::loader::atomic_checkpoint("AP-02")?;

    if load_base != 0 {
        if let Ok(rela) = loader.get_section(elf_data, ".rela.dyn") {
            crate::loader::reloc::apply_relocations(load_base, &seg_pages, rela)?;
        }
    }
    crate::loader::atomic_checkpoint("AP-03")?;
    crate::loader::wx::enforce(&final_flags, name)?;

    let pages = super::stack_pages_for(name);
    let kstack = super::stack::Stack::new_kernel(pages).map_err(|_| ViError::OutOfMemory)?;
    kstack.zero_usable();
    #[cfg(feature = "test-hooks")]
    kstack.test_hook_prime_watermark();
    crate::loader::atomic_checkpoint("AP-04")?;
    let ustack =
        super::stack::Stack::new_user(user_stack_pages).map_err(|_| ViError::OutOfMemory)?;
    // Frames come back from the allocator holding their last owner's bytes, and
    // the initial-stack writer fills only the top of the range: zero all of it so
    // a new cell can never read another cell's data off its own stack. Through
    // the frames' kernel alias, never the user stack's USER VA (census G12).
    ustack.zero_usable();
    #[cfg(feature = "test-hooks")]
    ustack.test_hook_prime_watermark();
    let entry = header.entry.wrapping_add(load_base);
    #[cfg(target_pointer_width = "64")]
    let user_sp = write_initial_stack(elf_data, &ustack, load_base, entry, name)?;
    #[cfg(not(target_pointer_width = "64"))]
    let user_sp = ustack.top;

    Ok(PreparedElfTask {
        name: name.to_string(),
        requested_cell_id,
        allowed_drivers,
        kstack,
        ustack,
        segments,
        entry,
        load_base,
        user_sp,
        user_cet: cfi.user_cet,
    })
}

/// Write the Cell initial stack (argc, argv, envp, auxv; CP1 AX-2…AX-5) below
/// the top of the root user stack and return the entry `sp`. Runs before the
/// task exists, so the stack is reached through the shared address space where
/// `Stack::new_user` mapped it; a Tier-2 domain maps the same frames later.
#[cfg(target_pointer_width = "64")]
fn write_initial_stack(
    elf: &[u8],
    ustack: &super::stack::Stack,
    load_base: usize,
    entry: usize,
    name: &str,
) -> Result<usize, ViError> {
    use cell_initial_stack::{build, Image, MAX_PHDRS, PHENT};

    let field = |at: usize, width: usize| -> Option<u64> {
        let bytes = elf.get(at..at + width)?;
        let mut word = [0u8; 8];
        word[..width].copy_from_slice(bytes);
        Some(u64::from_le_bytes(word))
    };
    // ELF64 header: e_phoff @32 (8), e_phentsize @54 (2), e_phnum @56 (2).
    let (Some(phoff), Some(phentsize), Some(phnum)) = (field(32, 8), field(54, 2), field(56, 2))
    else {
        return Err(ViError::InvalidInput);
    };
    if phentsize as usize != PHENT || phnum as usize > MAX_PHDRS {
        log::warn!(
            "[loader] {}: {} program headers of {} bytes exceed the initial-stack bound ({} × {})",
            name,
            phnum,
            phentsize,
            MAX_PHDRS,
            PHENT
        );
        return Err(ViError::InvalidInput);
    }
    let start = usize::try_from(phoff).map_err(|_| ViError::InvalidInput)?;
    let phdrs = start
        .checked_add(phnum as usize * PHENT)
        .and_then(|end| elf.get(start..end))
        .ok_or(ViError::InvalidInput)?;

    let mut seed = [0u8; 16];
    let random = (crate::dev::virtio_rng::get_random(&mut seed) == seed.len()).then_some(seed);
    let image = Image {
        phdrs,
        load_base: load_base as u64,
        entry: entry as u64,
        random,
    };
    let block = build(ustack.top as u64, &image).map_err(|_| ViError::InvalidInput)?;
    super::copy_glue::TaskCopyView::sas()
        .write_bytes(block.sp() as usize, block.bytes())
        .map_err(|_| ViError::OutOfMemory)?;
    Ok(block.sp() as usize)
}

/// Observe each exact reserve failure while rollback still owns the live frames.
/// This witness precedes real unwind; it is not a final retained-leak assertion.
#[cfg(feature = "p01-trace")]
fn p01_post_load_failure(pages: &[crate::loader::elf::LoadedPage], load_base: usize, edge: u64) {
    // Source lock only, no scheduler/owner invention. Preserve one coherent
    // physical witness batch while the mapped leaves remain allocator-resident.
    let _ = crate::memory::frame::with(|_frames| {
        let _physical_write = crate::p01_retained::physical_write();
        for page in pages {
            crate::p01_retained::physical_prepare_failed(page.frame, page.va, load_base, edge);
        }
    });
}
