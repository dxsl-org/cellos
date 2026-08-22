//! Cell loader — ELF parsing, relocation, and path-based spawning.

use core::sync::atomic::{AtomicBool, Ordering};
use types::*;

/// Tracks whether a block-I/O cell has registered the VFS fast-IPC handler pointer.
/// Set to `true` on first registration; subsequent registrations (hot-swap path) log
/// a warning and re-point the handler.  Never reset — warm boot / snapshot restore
/// skips `spawn_from_path`, so re-registration never fires spuriously.
static BLOCK_IO_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Per-path capability ceiling for the cells named in the boot manifest.
#[cfg(feature = "test-hooks")]
pub mod atomic_publication_tests;
pub(crate) mod aligned_elf;
pub mod boot_ceiling;
pub mod disk_layout;
pub mod early;
pub mod elf;
pub mod elf_tests;
mod governed_spawn;
pub mod launch_profile;
/// Admission of caller-supplied in-memory ELF images (`Syscall::SpawnFromMem`).
pub mod mem_spawn_gate;
mod manifest_section;
mod manifest_section_tests;
pub mod reloc;
mod spawn_request;
pub use spawn_request::SpawnRequest;
pub mod va_alloc;
/// W^X: lower cell pages to their ELF `p_flags` once relocation has finished.
pub mod wx;
pub use elf::ElfLoader;

/// ELF parser trait.
pub trait ElfParser {
    /// Parse ELF header, returning entry point and section-header offset.
    fn parse_header(&self, data: &[u8]) -> ViResult<ElfHeader>;

    /// Return the raw bytes of a named section, or `ViError::NotFound`.
    fn get_section<'a>(&self, data: &'a [u8], name: &str) -> ViResult<&'a [u8]>;
}

/// Parsed ELF header fields needed by the spawner.
pub struct ElfHeader {
    /// Entry point virtual address.
    pub entry: VAddr,
    /// Section header table file offset (used for relocation lookups).
    pub shoff: usize,
}

/// Legacy hardcoded path grants for cells lacking a `__ViCell_manifest`.
///
/// Only `/bin/` paths gain privilege, still bounded by the launch request.
fn legacy_path_caps(path: &str) -> crate::task::cap::CapSet {
    let mut c = crate::task::cap::CapSet::EMPTY;
    if path.starts_with("/bin/") {
        if path.ends_with("/bin/vfs") {
            c.block_io = true;
            c.block_regions = 0b111; // legacy manifest-less fallback: P1 + P4 + SRV
        }
        if path.ends_with("/bin/net") {
            c.network = true;
        }
        if path.ends_with("/bin/shell") || path.ends_with("/bin/init") {
            c.spawn = true;
        }
    }
    c
}

/// Spawn a governed cell from the early filesystem with all route decisions
/// already represented by `request`.
pub fn spawn_from_path(path: &str, request: SpawnRequest) -> ViResult<usize> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.len() > disk_layout::MAX_CELL_PATH
        || path.contains("..")
        || path.contains("//")
    {
        return Err(ViError::InvalidInput);
    }
    let elf = early::EarlyLoader::read_file(path)?;
    spawn_gated(&elf, path, request)
}

/// Govern and atomically publish resident ELF bytes.
pub fn spawn_gated(elf: &[u8], path: &str, request: SpawnRequest) -> ViResult<usize> {
    #[cfg(feature = "test-hooks")]
    atomic_publication_tests::begin_governed_attempt();
    let result = governed_spawn::spawn_gated(elf, path, request);
    #[cfg(feature = "test-hooks")]
    atomic_publication_tests::finish_governed_attempt(&result);
    result
}

/// Explicit embedded-init path. This deliberately bypasses manifest/signature
/// admission but still publishes a fully configured root task in one commit.
pub fn spawn_trusted_init(elf: &[u8]) -> ViResult<usize> {
    governed_spawn::spawn_trusted_init(elf)
}

pub(crate) fn commit_launch_routes(
    tid: usize,
    cell_id: CellId,
    routes: crate::task::LaunchRoutes,
) {
    if routes.block_io {
        let already = BLOCK_IO_REGISTERED.swap(true, Ordering::SeqCst);
        if already {
            log::warn!("[loader] block_io route replaced by task {}", tid);
        }
        crate::fast_ipc::set_vfs_handler_cell(cell_id.0 as usize);
    }
    if routes.input {
        crate::task::drivers::driver_cell::set_input_cell(tid);
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn block_io_registered_snapshot() -> bool {
    BLOCK_IO_REGISTERED.load(Ordering::Acquire)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn restore_block_io_registration_for_test(registered: bool) {
    BLOCK_IO_REGISTERED.store(registered, Ordering::Release);
}

#[cfg(feature = "test-hooks")]
pub(crate) fn atomic_checkpoint(case: &'static str) -> ViResult<()> {
    atomic_publication_tests::checkpoint(case)
}

#[cfg(not(feature = "test-hooks"))]
pub(crate) fn atomic_checkpoint(_case: &'static str) -> ViResult<()> {
    Ok(())
}

#[cfg(feature = "test-hooks")]
pub(crate) fn observe_pre_ready(sched: &crate::task::scheduler::Scheduler, tid: usize) {
    atomic_publication_tests::observe_complete(sched, tid);
}

#[cfg(not(feature = "test-hooks"))]
pub(crate) fn observe_pre_ready(_sched: &crate::task::scheduler::Scheduler, _tid: usize) {}

/// Linker trait (reserved for future dynamic-linking support).
#[allow(dead_code)] // reason: trait body used by future Cell hot-swap (Phase 20)
pub trait Linker {
    fn load_cell(&mut self, data: &[u8]) -> ViResult<CellId>;
    fn resolve_symbol(&self, name: &str) -> ViResult<VAddr>;
    fn unload_cell(&mut self, id: CellId) -> ViResult<()>;
}
