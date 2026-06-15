//! Cell loader — ELF parsing, relocation, and path-based spawning.

use core::sync::atomic::{AtomicBool, Ordering};
use types::*;

/// Tracks whether a block-I/O cell has registered the VFS fast-IPC handler pointer.
/// Set to `true` on first registration; subsequent registrations (hot-swap path) log
/// a warning and re-point the handler.  Never reset — warm boot / snapshot restore
/// skips `spawn_from_path`, so re-registration never fires spuriously.
static BLOCK_IO_REGISTERED: AtomicBool = AtomicBool::new(false);

pub mod disk_layout;
pub mod early;
pub mod elf;
pub mod elf_tests;
pub mod reloc;
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

/// Spawn a cell by reading its ELF from a filesystem path.
///
/// Resolution order:
/// 1. If the early-boot cell table has been probed (via `early::EarlyLoader::probe`),
///    reads the ELF directly from the block device at the known LBA.
/// 2. Otherwise returns `ViError::NotFound` — the caller must ensure the early
///    table is probed before calling `spawn_from_path` during bootstrapping.
///
/// After the ELF is loaded into memory, relocations are applied and the cell is
/// enqueued via `crate::task::spawn_from_mem`.
///
/// # Errors
/// - `ViError::NotFound` — path absent from the bootstrap table.
/// - `ViError::InvalidInput` — malformed ELF or unsupported relocation.
/// - `ViError::OutOfMemory` — cannot allocate frames for segments.
pub fn spawn_from_path(path: &str) -> ViResult<usize> {
    // Validate path: non-empty, leading slash, bounded length, no traversal sequences.
    // Reject '..' and '//' to prevent a future VFS-backed spawn from escaping /bin/
    // via a /bin/-prefixed traversal path (defense-in-depth; currently harmless since
    // the early loader uses exact-match, but cheap to enforce here unconditionally).
    if path.is_empty()
        || !path.starts_with('/')
        || path.len() > disk_layout::MAX_CELL_PATH
        || path.contains("..")
        || path.contains("//")
    {
        log::error!("[loader] invalid path {:?}", path);
        return Err(ViError::InvalidInput);
    }

    log::info!("[loader] SpawnFromPath: {}", path);

    // Read ELF bytes from the early bootstrap table.
    let elf_bytes = early::EarlyLoader::read_file(path)?;

    // Apply relocations (base = 0 for fixed-VA cells; non-zero for PIE cells).
    // For cells with no .rela.dyn section, get_section returns NotFound — skip.
    let base: VAddr = 0; // fixed-VA cells compiled with shell.ld; PIE support is future work
    let elf_loader = ElfLoader;
    if let Ok(rela_section) = elf_loader.get_section(&elf_bytes, ".rela.dyn") {
        reloc::apply_relocations(base, rela_section)?;
    }

    // Read capability manifest from `__ViCell_manifest` ELF section.
    // Absent or malformed → None (falls back to legacy hardcoded path grants).
    let manifest_opt: Option<api::manifest::CellManifest> =
        match elf_loader.get_section(&elf_bytes, "__ViCell_manifest") {
            Ok(bytes) => api::manifest::CellManifest::from_bytes(bytes),
            Err(_)    => None,
        };

    // Privilege gate: a user Cell (path NOT under /bin/) may NOT declare any
    // privileged capability.  Runs BEFORE spawn_from_mem — no task is created
    // for a rejected Cell.
    if let Some(ref m) = manifest_opt {
        if !path.starts_with("/bin/") && m.declares_any_privilege() {
            log::error!(
                "[loader] DENY spawn {:?}: user cell over-declares caps (flags={:#04x})",
                path, m.flags
            );
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSpawnDenied,
                &crate::audit::encode_u32x2(m.flags as u32, 0u32),
            );
            return Err(ViError::PermissionDenied);
        }
    }

    // Extract cell name from the last path component (e.g. "/bin/shell" → "shell").
    let name = path.rsplit('/').next().unwrap_or(path);

    // Spawn via the existing in-memory spawn path (ELF parse + segment map).
    let tid = crate::task::spawn_from_mem(&elf_bytes, name, CellId(0), alloc::vec::Vec::new())
        .map_err(|_| ViError::OutOfMemory)?;

    // Assign a unique CellId based on the task ID so per-cell quota and
    // capability checks are correctly scoped.  `spawn_from_mem` defaults to
    // CellId(0) (kernel), which would make every path-spawned cell share the
    // kernel's quota slot (charge() short-circuits for cell_id == 0).
    let cell_id = CellId(tid as u64);
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            task.cell_id = cell_id;
        }
    }

    crate::audit::log_event(
        crate::audit::AuditEvent::CellSpawn,
        &crate::audit::encode_u32x2(tid as u32, 0u32),
    );

    // Read per-Cell syscall allowlist from ELF section __ViCell_syscalls.
    // The section (if present) contains a u64 LE bitset; absent = permit-all.
    {
        let allowlist = match ElfLoader.get_section(&elf_bytes, "__ViCell_syscalls") {
            Ok(bytes) if bytes.len() >= 8 => {
                // SAFETY: bytes slice is valid data from the loaded ELF.
                u64::from_le_bytes(bytes[..8].try_into().expect("8-byte __ViCell_syscalls section"))
            }
            _ => u64::MAX, // no section → permit-all (backwards compatible)
        };
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            if let Some(task) = sched.tasks.get_mut(&tid) {
                task.syscall_allowlist = allowlist;
            }
        }
    }

    // Register per-cell memory quota (4 MiB default) using the real CellId.
    crate::memory::cell_quota::register(cell_id, crate::memory::cell_quota::DEFAULT_QUOTA_BYTES);

    // Grant ZST capability tokens.
    // Manifest present → grant from declared flags; absent → legacy hardcoded path grants.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            match manifest_opt {
                Some(m) => {
                    if m.has_block_io() {
                        task.block_io_cap = Some(crate::task::cap::BlockIoCap::new());
                        // Partition range grants (P03): the manifest scopes WHICH
                        // LBA ranges the raw block syscalls may touch. A manifest
                        // cell that declares block_io but no PART_* bit gets cap
                        // without ranges — every access denied (deny-by-default).
                        // Bit 2 (SRV/P5) co-granted with bit 1 (LFS/P4): both belong
                        // exclusively to the VFS service.  When manifest flags expand
                        // to u16, split into a dedicated MANIFEST_FLAG_PART_SRV bit.
                        task.block_regions = (m.has_part_data() as u8)
                                           | ((m.has_part_lfs() as u8) << 1)
                                           | ((m.has_part_lfs() as u8) << 2);
                        // Re-registration is valid on VFS hot-swap; just update the handler pointer.
                        // Using swap to track whether this is a first-boot registration or a re-swap.
                        let already = BLOCK_IO_REGISTERED.swap(true, Ordering::SeqCst);
                        if already {
                            log::warn!("[loader] block_io re-registration — VFS hot-swap or second block_io cell");
                        }
                        crate::fast_ipc::set_vfs_handler_cell(cell_id.0 as usize);
                    }
                    if m.has_network() {
                        task.network_cap = Some(crate::task::cap::NetworkCap::new());
                    }
                    if m.has_spawn() {
                        task.spawn_cap = Some(crate::task::cap::SpawnCap::new());
                    }
                    if m.has_gpio() || m.has_uart() {
                        task.mmio_cap = true;
                    }
                    if m.has_hypervisor()
                        && (crate::cpu_features::has_h_ext()
                            || crate::cpu_features::has_el2()) {
                        task.hypervisor_cap = Some(crate::task::cap::HypervisorCap::new());
                    }
                }
                None => {
                    // Legacy hardcoded path grants for cells without a manifest.
                    // Outer starts_with guard prevents suffix-only matches from
                    // non-/bin/ paths (e.g., /data/bin/vfs) gaining privileged caps.
                    if path.starts_with("/bin/") {
                        if path.ends_with("/bin/vfs") {
                            task.block_io_cap = Some(crate::task::cap::BlockIoCap::new());
                            // Legacy grant matches the pre-P03 behavior: VFS may
                            // address both grantable partitions (P1 + P4).
                            task.block_regions = 0b11;
                            let already = BLOCK_IO_REGISTERED.swap(true, Ordering::SeqCst);
                            if already {
                                log::warn!("[loader] block_io re-registration (legacy) — VFS hot-swap");
                            }
                            crate::fast_ipc::set_vfs_handler_cell(cell_id.0 as usize);
                        }
                        if path.ends_with("/bin/net") {
                            task.network_cap = Some(crate::task::cap::NetworkCap::new());
                        }
                        if path.ends_with("/bin/shell") || path.ends_with("/bin/init") {
                            task.spawn_cap = Some(crate::task::cap::SpawnCap::new());
                        }
                    }
                }
            }
        }
    }
    // Register input service endpoint regardless of manifest presence.
    if path.ends_with("/bin/input") {
        crate::task::drivers::virtio_input::set_input_cell(tid);
    }
    Ok(tid)
}

/// Linker trait (reserved for future dynamic-linking support).
#[allow(dead_code)] // reason: trait body used by future Cell hot-swap (Phase 20)
pub trait Linker {
    fn load_cell(&mut self, data: &[u8]) -> ViResult<CellId>;
    fn resolve_symbol(&self, name: &str) -> ViResult<VAddr>;
    fn unload_cell(&mut self, id: CellId) -> ViResult<()>;
}
