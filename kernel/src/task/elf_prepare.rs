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
    ) -> (alloc::boxed::Box<super::Task>, usize) {
        let mut task = alloc::boxed::Box::new(super::Task::new(
            id,
            cell_id,
            &self.name,
            self.allowed_drivers,
        ));
        task.kernel_stack = Some(self.kstack);
        task.user_stack = Some(self.ustack);
        task.segment_mem = Some(self.segments);
        super::prime_user_mode_entry(&mut task, self.entry, 0);
        (task, self.load_base)
    }

    pub(super) fn requested_cell_id(&self) -> CellId {
        self.requested_cell_id
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
    let aligned = crate::loader::aligned_elf::bytes(data);
    let elf_data = aligned.as_ref();

    let loader = ElfLoader;
    let header = loader.parse_header(elf_data)?;
    let load_base = if super::elf_is_pie(elf_data) {
        crate::loader::va_alloc::alloc_cell_va().ok_or(ViError::OutOfMemory)?
    } else {
        0
    };
    if let Err(error) = crate::loader::atomic_checkpoint("AP-01") {
        if load_base != 0 {
            crate::loader::va_alloc::free_cell_va(load_base);
        }
        return Err(error);
    }

    let seg_pages = {
        let mut frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = frames.as_mut().ok_or(ViError::OutOfMemory)?;
        match loader.load_segments(elf_data, allocator, load_base) {
            Ok(pages) => pages,
            Err(error) => {
                if load_base != 0 {
                    crate::loader::va_alloc::free_cell_va(load_base);
                }
                return Err(error);
            }
        }
    };
    let final_flags = seg_pages
        .iter()
        .map(|p| (p.va, p.final_flags))
        .collect::<Vec<_>>();
    let segments = super::stack::CellSegments::with_writable_pages(
        seg_pages.iter().map(|p| (p.va, p.frame)).collect(),
        seg_pages
            .iter()
            .filter(|p| p.final_flags.bits() & crate::memory::paging::Flags::WRITE != 0)
            .map(|p| p.va)
            .collect(),
        load_base,
    );
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
