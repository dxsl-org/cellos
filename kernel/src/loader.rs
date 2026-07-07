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
pub mod va_alloc;
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
/// Legacy hardcoded path grants for cells lacking a `__ViCell_manifest`.
/// Mirrors the pre-manifest behavior; only `/bin/` paths gain privilege. The
/// returned set is still subject to spawner intersection in `spawn_from_path`.
fn legacy_path_caps(path: &str) -> crate::task::cap::CapSet {
    let mut c = crate::task::cap::CapSet::EMPTY;
    if path.starts_with("/bin/") {
        if path.ends_with("/bin/vfs") {
            c.block_io = true;
            c.block_regions = 0b11; // legacy: P1 + P4 (pre-P03, no SRV bit)
        }
        if path.ends_with("/bin/net") { c.network = true; }
        if path.ends_with("/bin/shell") || path.ends_with("/bin/init") { c.spawn = true; }
    }
    c
}

/// Spawn a cell from a filesystem path. `spawner` sets the capability ceiling:
/// `Root` (boot/init) grants the full manifest; `User(tid)`/`Ceiling(caps)`
/// intersect the manifest with the spawner's caps (P2 monotonic downgrade).
pub fn spawn_from_path(path: &str, spawner: crate::task::cap::Spawner) -> ViResult<usize> {
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

    // ── Binary signature gate ─────────────────────────────────────────────────
    // Verify the Ed25519 signature in __ViCell_sig before any ELF parsing or
    // task creation. With `signing-required`, an absent signature is treated
    // the same as an invalid one (fail-closed). In dev mode (default), an
    // absent signature is permitted so unsigned dev cells keep working.
    match crate::signing::extract_sig(&elf_bytes) {
        Some(sig) => {
            if !crate::signing::verify_cell(&elf_bytes, &sig) {
                log::warn!("[loader] DENY {:?}: cell signature INVALID", path);
                crate::audit::log_event(
                    crate::audit::AuditEvent::CellSignatureFailed,
                    &crate::audit::encode_u32x2(0, 0),
                );
                return Err(ViError::PermissionDenied);
            }
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSignatureVerified,
                &crate::audit::encode_u32x2(0, 0),
            );
        }
        None if crate::signing::signing_required() => {
            log::warn!("[loader] DENY {:?}: no __ViCell_sig (signing-required)", path);
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSignatureFailed,
                &crate::audit::encode_u32x2(0, 0),
            );
            return Err(ViError::PermissionDenied);
        }
        None => {
            // Dev mode: unsigned cell permitted.
        }
    }

    let elf_loader = ElfLoader;

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

    // Spawn via the in-memory path.  spawn_from_mem now applies .rela.dyn
    // relocations internally, so no separate apply_relocations call is needed.
    let (tid, _load_base) = crate::task::spawn_from_mem(&elf_bytes, name, CellId(0), alloc::vec::Vec::new())
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

    // Integrity measurement (IMA-style): hash the ELF image and record it in the
    // append-only measurement log BEFORE the cell is scheduled. Evidence for
    // future DICE/EAT attestation — orthogonal to (and complements) Cell signing.
    crate::measurement_log::measure(tid, path, &elf_bytes);

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

    // Read cluster membership from ELF section __ViCell_cluster.
    // Layout: u8 mode, u8 pad[7], u64 cluster_id (LE) = 16 bytes.
    // Absent section → Isolated mode (mode=0, cluster_id=0); backwards compatible.
    {
        let (mode, cid) = match ElfLoader.get_section(&elf_bytes, "__ViCell_cluster") {
            Ok(bytes) if bytes.len() >= 16 => {
                // SAFETY: bytes slice is valid data from the loaded ELF.
                let mode = bytes[0];
                let cid  = u64::from_le_bytes(bytes[8..16].try_into().expect("8-byte cluster_id"));
                (mode, cid)
            }
            _ => (0u8, 0u64), // no section → Isolated, no cluster
        };
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            if let Some(task) = sched.tasks.get_mut(&tid) {
                task.cluster_mode = mode;
                task.cluster_id   = cid;
            }
        }
    }

    // Register per-cell memory quota (4 MiB default) using the real CellId.
    crate::memory::cell_quota::register(cell_id, crate::memory::cell_quota::DEFAULT_QUOTA_BYTES);

    // ── Capability grant (P2 — spawn-time monotonic downgrade) ───────────────
    // 1. `requested` = caps the manifest declares (absent → legacy path grants).
    // 2. `granted`   = requested ∩ spawner-ceiling — a cell cannot hand a child a
    //    cap it does not itself hold. `init` (Spawner::Root) is the root authority
    //    and is exempt; HotSwap passes the replaced cell's caps as the ceiling.
    use crate::task::cap::{CapSet, Spawner};
    let requested: CapSet = match manifest_opt {
        Some(ref m) => CapSet::from_manifest(m),
        None        => legacy_path_caps(path),
    };
    // Snapshot the spawner's caps in its OWN lock scope; the guard is DROPPED
    // before the child-mutation lock below (the Spinlock is non-reentrant).
    let after_spawner: CapSet = match spawner {
        Spawner::Root          => requested,
        Spawner::Ceiling(ceil) => requested.intersect(ceil),
        Spawner::User(stid)    => {
            let ceil = crate::task::SCHEDULER.lock().as_ref()
                .and_then(|s| s.tasks.get(&stid))
                .map(|t| CapSet::of_task(t))
                .unwrap_or(CapSet::EMPTY); // unknown spawner → no caps (fail-safe)
            requested.intersect(ceil)
        }
    };
    // 3. Operator policy (P5/Phase 04): `granted = after_spawner ∩ policy(path)`,
    //    with trusted-core recovery + fail-safe. `policy::apply` takes the POLICY
    //    lock internally — called OUTSIDE the SCHEDULER guard above to avoid lock
    //    nesting. `init` (Root) is exempt (it is the loader OF the policy).
    let granted: CapSet = match spawner {
        Spawner::Root => after_spawner,
        _ => crate::policy::apply(path, tid, after_spawner),
    };

    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            granted.apply_to(task);

            // x86_64 PKU: derive the protection-key domain from the granted caps.
            // Trusted-core cells (block_io / network / spawn / hypervisor) get key 0
            // (all-access); standard cells get key 1. Key 2 is reserved for Tier-1b
            // C/FFI cells and will be assigned once the manifest carries a tier field.
            // On non-x86_64 targets these fields default to 0 and are never consulted.
            #[cfg(target_arch = "x86_64")]
            {
                let is_trusted = granted.block_io
                    || granted.network
                    || granted.spawn
                    || granted.hypervisor;
                // TODO(pku-ffi): derive key 2 from a future manifest tier field.
                // Until then, all non-trusted cells use key 1 (standard domain).
                let key: u8 = if is_trusted { 0 } else { 1 };
                task.pku_key   = key;
                task.pku_value = crate::hal::pku::pkru_for_key(key);
            }

            // Path-based non-manifest caps — granted here because all 8 manifest flag
            // bits are occupied (v1 manifest is full; v2 requires a Law-1 bump).
            //
            // PcieDriverCap: Driver Cells that own PCIe BAR MMIO + DMA grants.
            // SupervisorCap: single Supervisor Cell that orchestrates live hotswap.
            //   Mirrored in the init grant path (kernel/src/main.rs) so that if the
            //   Supervisor Cell crashes and init needs to unfreeze its frozen targets,
            //   init retains the authority to do so.
            if path == "/bin/nvme" || path == "/bin/e1000" || path == "/bin/virtio-net"
                || path == "/bin/block"
                || path == "/bin/input" || path == "/bin/virtio-gpu" {
                task.pcie_driver_cap = Some(crate::task::cap::PcieDriverCap::new());
            }
            if path == "/bin/platform" {
                match crate::task::cap::try_grant_platform() {
                    Some(cap) => task.platform_cap = Some(cap),
                    None => {
                        log::error!("[loader] PlatformCap already granted — refusing 2nd /bin/platform spawn");
                        return Err(types::ViError::PermissionDenied);
                    }
                }
            }
            if path == "/bin/supervisor" {
                task.supervisor_cap = Some(crate::task::cap::SupervisorCap::new());
            }
        }
    }

    // Side effect keyed off the GRANTED (not requested) block-io bit: the VFS
    // fast-IPC handler must point at whoever actually received block_io.
    if granted.block_io {
        // Re-registration is valid on VFS hot-swap; just re-point the handler.
        let already = BLOCK_IO_REGISTERED.swap(true, Ordering::SeqCst);
        if already {
            log::warn!("[loader] block_io re-registration — VFS hot-swap or second block_io cell");
        }
        crate::fast_ipc::set_vfs_handler_cell(cell_id.0 as usize);
    }
    // Register the input service endpoint so console_drv can route UART bytes to it.
    // (Service-registry registration is done by init via sys_register_service.)
    if path.ends_with("/bin/input") {
        crate::task::drivers::driver_cell::set_input_cell(tid);
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
