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
    segments: super::stack::CellSegments,
    entry: usize,
    load_base: usize,
}

impl PreparedElfTask {
    pub(super) fn into_task(
        self,
        id: usize,
        cell_id: CellId,
    ) -> Result<(alloc::boxed::Box<super::Task>, usize), ViError> {
        let mut task = crate::memory::heap::try_box(super::Task::new(
            id,
            cell_id,
            &self.name,
            self.allowed_drivers,
        ))?;
        task.kernel_stack = Some(self.kstack);
        task.user_stack = Some(self.ustack);
        task.segment_mem = Some(self.segments);
        super::prime_user_mode_entry(&mut task, self.entry, 0);
        Ok((task, self.load_base))
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

/// Own the PIE reservation before any subsequent fallible preparation step.
struct PreparationVa(usize);
impl Drop for PreparationVa {
    fn drop(&mut self) {
        if self.0 != 0 { crate::loader::va_alloc::free_cell_va(self.0); }
    }
}

/// LoadedPage is metadata, so bridge its mappings into CellSegments ownership
/// without another allocation. This guard must be declared after the page Vec.
struct PreparationMappings<'a>(Option<&'a [crate::loader::elf::LoadedPage]>);
impl Drop for PreparationMappings<'_> {
    fn drop(&mut self) {
        if let Some(pages) = self.0.take() {
            let mut frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
            let allocator = frames.as_mut().expect("loaded ELF lost its frame allocator");
            crate::loader::ElfLoader::unwind(pages, allocator);
        }
    }
}

/// Parse, map, relocate, protect, and stack an ELF without touching scheduler state.
pub fn prepare_elf_task(
    data: &[u8],
    name: &str,
    requested_cell_id: CellId,
    allowed_drivers: Vec<usize>,
) -> Result<PreparedElfTask, ViError> {
    use crate::loader::{ElfLoader, ElfParser};

    if data.len() < 4 || &data[..4] != b"\x7fELF" {
        return Err(ViError::InvalidInput);
    }
    let aligned = crate::loader::aligned_elf::bytes(data)?;
    let elf_data = aligned.as_ref();

    let loader = ElfLoader;
    let header = loader.parse_header(elf_data)?;
    let load_base = if super::elf_is_pie(elf_data) {
        crate::loader::va_alloc::alloc_cell_va().ok_or(ViError::OutOfMemory)?
    } else {
        0
    };
    let mut va_rollback = PreparationVa(load_base);
    crate::loader::atomic_checkpoint("AP-01")?;

    let seg_pages = {
        let mut frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = frames.as_mut().ok_or(ViError::OutOfMemory)?;
        loader.load_segments(elf_data, allocator, load_base)?
    };
    let mut mapping_rollback = PreparationMappings(Some(&seg_pages));
    #[cfg(feature = "test-hooks")]
    crate::loader::atomic_publication_tests::observe_unpublished_pages(&seg_pages);
    let final_flags = crate::loader::atomic_checkpoint("AP-RF")
        .map_err(|_| ViError::OutOfMemory)
        .and_then(|_| collect_reserved(seg_pages.iter().map(|p| (p.va, p.final_flags))))
        .map_err(|error| {
            #[cfg(cellos_p01_trace)]
            p01_post_load_failure(&seg_pages, load_base, 2);
            error
        })?;
    let mapped_pages = crate::loader::atomic_checkpoint("AP-RM")
        .map_err(|_| ViError::OutOfMemory)
        .and_then(|_| collect_reserved(seg_pages.iter().map(|p| (p.va, p.frame))))
        .map_err(|error| {
            #[cfg(cellos_p01_trace)]
            p01_post_load_failure(&seg_pages, load_base, 3);
            error
        })?;
    // The filtered set has at most one entry per page, so the page count is an
    // upper bound: reserving it means no growth, hence no infallible reallocation.
    let mut writable_pages = Vec::new();
    crate::loader::atomic_checkpoint("AP-RW")
        .map_err(|_| ViError::OutOfMemory)
        .and_then(|_| writable_pages.try_reserve_exact(seg_pages.len()).map_err(|_| ViError::OutOfMemory))
        .map_err(|_| {
            #[cfg(cellos_p01_trace)]
            p01_post_load_failure(&seg_pages, load_base, 4);
            ViError::OutOfMemory
        })?;
    for page in seg_pages.iter() {
        if page.final_flags.bits() & crate::memory::paging::Flags::WRITE != 0 {
            writable_pages.push(page.va);
        }
    }
    #[cfg(cellos_p01_trace)]
    {
        // These exclusive backing arrays survive in CellSegments. Other loader
        // scratch/shared-helper allocations retain origin-only classification.
        crate::memory::heap::p01_owner::adopt_vec(&mapped_pages);
        crate::memory::heap::p01_owner::adopt_vec(&writable_pages);
    }
    let segments = super::stack::CellSegments::with_writable_pages(
        mapped_pages,
        writable_pages,
        load_base,
    );
    // Constructor is infallible: the segment owner now exclusively owns both
    // frame reclamation and PIE release, including every later failure edge.
    mapping_rollback.0 = None;
    va_rollback.0 = 0;
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
    // SAFETY: the freshly allocated stack's usable range is exclusively owned.
    unsafe {
        core::ptr::write_bytes(kstack.usable_start() as *mut u8, 0, kstack.usable_bytes());
    }
    #[cfg(feature = "test-hooks")]
    kstack.test_hook_prime_watermark();
    crate::loader::atomic_checkpoint("AP-04")?;
    let ustack = super::stack::Stack::new_user(pages).map_err(|_| ViError::OutOfMemory)?;
    #[cfg(feature = "test-hooks")]
    ustack.test_hook_prime_watermark();

    Ok(PreparedElfTask {
        name: name.to_string(),
        requested_cell_id,
        allowed_drivers,
        kstack,
        ustack,
        segments,
        entry: header.entry.wrapping_add(load_base),
        load_base,
    })
}

/// Observe each exact reserve failure while rollback still owns the live frames.
/// This witness precedes real unwind; it is not a final retained-leak assertion.
#[cfg(cellos_p01_trace)]
fn p01_post_load_failure(pages: &[crate::loader::elf::LoadedPage], load_base: usize, edge: u64) {
    // Source lock only, no scheduler/owner invention. Preserve one coherent
    // physical witness batch while the mapped leaves remain allocator-resident.
    let _frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
    let _physical_write = crate::p01_retained::physical_write();
    for page in pages {
        crate::p01_retained::physical_prepare_failed(page.frame, page.va, load_base, edge);
    }
}
