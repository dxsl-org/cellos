//! Immutable governed preflight followed by owned preparation and one commit.

use super::ElfParser;
use alloc::string::ToString;
use types::{CellId, ViError, ViResult};

fn next_tid_hint() -> usize {
    crate::task::SCHEDULER
        .lock()
        .as_ref()
        .map_or(0, |sched| sched.next_task_id)
}

fn section_u64(elf: &[u8], name: &str) -> u64 {
    super::ElfLoader
        .get_section(elf, name)
        .ok()
        .and_then(|bytes| bytes.get(..8))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(u64::MAX)
}

fn cluster(elf: &[u8]) -> (u8, u64) {
    super::ElfLoader
        .get_section(elf, "__ViCell_cluster")
        .ok()
        .filter(|bytes| bytes.len() >= 16)
        .map(|bytes| {
            let id = u64::from_le_bytes(bytes[8..16].try_into().expect("cluster id"));
            (bytes[0], id)
        })
        .unwrap_or((0, 0))
}

fn prepare_gated(
    elf: &[u8],
    path: &str,
    mut request: super::SpawnRequest,
) -> ViResult<(crate::task::PreparedElfTask, crate::task::TaskLaunchState)> {
    use crate::task::cap::{CapSet, Spawner};
    let aligned = super::aligned_elf::bytes(elf)?;
    let elf = aligned.as_ref();
    let elf_digest = crate::sha256::sha256(elf);
    let manifest = match super::manifest_section::classify(elf) {
        super::manifest_section::ManifestSection::Absent => None,
        super::manifest_section::ManifestSection::Valid { manifest, .. } => Some(manifest),
        super::manifest_section::ManifestSection::Malformed => {
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSpawnDenied,
                &crate::audit::encode_u32x2(0, 1),
            );
            super::atomic_checkpoint("AP-00")?;
            return Err(ViError::PermissionDenied);
        }
    };

    // Signature extraction, byte coverage, and verification stay exactly at the
    // established boundary: structural classification first, task creation later.
    let is_domain = match crate::signing::extract_sig(elf) {
        Some(sig) => match crate::signing::verify_cell(elf, &sig) {
            Ok(true) => {
                crate::audit::log_event(
                    crate::audit::AuditEvent::CellSignatureVerified,
                    &crate::audit::encode_u32x2(0, 0),
                );
                manifest.as_ref().is_some_and(|m| {
                    m.protection_class() == api::manifest::PROTECTION_CLASS_FFI
                        || m.protection_class() == api::manifest::PROTECTION_CLASS_UNTRUSTED
                })
            }
            Ok(false) => {
                crate::audit::log_event(
                    crate::audit::AuditEvent::CellSignatureFailed,
                    &crate::audit::encode_u32x2(0, 0),
                );
                return Err(ViError::PermissionDenied);
            }
            // Verification could not run (no room for the signed payload): refuse the
            // cell and report capacity. An unperformed check must not read as a
            // signature mismatch, and it must not halt the kernel either.
            Err(err) => {
                crate::audit::log_event(
                    crate::audit::AuditEvent::CellSpawnDenied,
                    &crate::audit::encode_u32x2(0, 3),
                );
                return Err(err);
            }
        },
        None => {
            // This explicitly selected image runs only signed browser engines;
            // an advisory launch path must never authorize unsigned ELF bytes.
            #[cfg(feature = "browser-engine-profile")]
            return Err(ViError::PermissionDenied);
            #[cfg(not(feature = "browser-engine-profile"))]
            {
                // Unsigned artifacts remain Tier 2 in the ordinary development profile.
                true
            }
        }
    };

    // Common owner admission gate: verify owner authorization for this ELF
    // before capability derivation or task creation.
    if let Err(err) = crate::admission::evaluate_owner_admission(&elf_digest) {
        crate::audit::log_event(
            crate::audit::AuditEvent::CellSpawnDenied,
            &crate::audit::encode_u32x2(0, 2),
        );
        return Err(err);
    }
    if let Some(manifest) = manifest.as_ref() {
        if !path.starts_with("/bin/") && manifest.declares_any_privilege() {
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSpawnDenied,
                &crate::audit::encode_u32x2(manifest.flags as u32, 0),
            );
            return Err(ViError::PermissionDenied);
        }
    }

    let requested = manifest
        .as_ref()
        .map(CapSet::from_manifest)
        .unwrap_or_else(|| super::legacy_path_caps(path))
        .with_path_caps(path);
    let bounded = match request.spawner {
        Spawner::Root => {
            let ceiling = super::boot_ceiling::boot_ceiling(path);
            let bounded = requested.intersect(ceiling);
            if bounded != requested {
                super::boot_ceiling::log_refusal(path, requested, ceiling, bounded);
            }
            bounded
        }
        Spawner::Ceiling(ceiling) => requested.intersect(ceiling),
        Spawner::User(tid) => {
            let ceiling = crate::task::SCHEDULER
                .lock()
                .as_ref()
                .and_then(|sched| sched.tasks.get(&tid))
                .map(|task| CapSet::of_task(task))
                .unwrap_or(CapSet::EMPTY);
            requested.intersect(ceiling)
        }
    };
    let granted = match request.spawner {
        Spawner::Root if !crate::policy::is_resolved() => bounded,
        _ => crate::policy::apply(path, next_tid_hint(), bounded),
    };
    if path == "/bin/vfs" {
        // Test hooks inject at the mandatory-region admission boundary even when
        // the active policy grants every VFS region.
        super::atomic_checkpoint("AP-09")?;
        if granted.block_regions != 0b1111 {
            return Err(ViError::PermissionDenied);
        }
    }

    let allowlist = section_u64(elf, "__ViCell_syscalls");
    let (cluster_mode, cluster_id) = cluster(elf);
    if request.priority > api::TaskPriority::RealTime as u8
        || (request.priority == api::TaskPriority::RealTime as u8 && cluster_mode != 0)
    {
        super::atomic_checkpoint("AP-10")?;
        return Err(ViError::PermissionDenied);
    }
    let platform = if granted.platform {
        super::atomic_checkpoint("AP-05")?;
        let reservation = crate::task::cap::reserve_platform()?;
        super::atomic_checkpoint("AP-06")?;
        Some(reservation)
    } else {
        None
    };
    #[cfg(target_arch = "x86_64")]
    let requested_class = manifest
        .as_ref()
        .map(|m| m.protection_class())
        .unwrap_or(api::manifest::PROTECTION_CLASS_LEGACY);
    #[cfg(target_arch = "x86_64")]
    let (pku_key, pku_value) = {
        let key = crate::task::cap::granted_protection_class(&granted, requested_class);
        (key, crate::hal::pku::pkru_for_key(key))
    };
    #[cfg(not(target_arch = "x86_64"))]
    let (pku_key, pku_value) = (0, 0);

    let name = path.rsplit('/').next().unwrap_or(path);
    // This private origin predates the child's CellId/generation. Quota routing
    // remains unchanged; publication binds the token only after exact identity.
    #[cfg(cellos_p01_trace)]
    let _heap_origin = crate::memory::heap::p01_owner::scope(request.trace);
    #[cfg(cellos_p01_trace)]
    if let Some(argv) = request.argv.as_ref() {
        crate::memory::heap::p01_owner::adopt_vec(argv);
    }
    let prepared = crate::task::prepare_elf_task(elf, name, CellId(0), alloc::vec::Vec::new())?;
    let state = crate::task::TaskLaunchState::complete(
        request.caller.take(),
        granted,
        platform,
        request.replacement.take(),
        crate::memory::cell_quota::default_quota_bytes(),
        allowlist,
        cluster_mode,
        cluster_id,
        request.priority,
        pku_key,
        pku_value,
        false,
        request.inherit_from,
        request.argv.take(),
        crate::task::LaunchRoutes {
            block_io: granted.block_io,
            input: path.ends_with("/bin/input"),
            #[cfg(feature = "test-hooks")]
            development_silo: path == "/bin/silo" && granted.hypervisor,
        },
        Some(crate::task::StagedMeasurement {
            path: path.to_string(),
            digest: elf_digest,
        }),
        is_domain,
    );
    #[cfg(cellos_p01_trace)]
    let state = { let mut state = state; state.trace = request.trace; state };
    Ok((prepared, state))
}

pub(super) fn spawn_gated(elf: &[u8], path: &str, request: super::SpawnRequest) -> ViResult<usize> {
    let (prepared, state) = prepare_gated(elf, path, request)?;
    crate::task::publish_prepared(prepared, state).map(|(tid, _)| tid)
}

impl super::SpawnRequest {
    /// Admit the exact artifact bytes once: the prepared image, digest and
    /// staged measurement all derive from this single read, so commit never
    /// rereads a replaceable path. Nothing is published: no owner, task,
    /// route, measurement, CSpace, or runnable queue entry.
    pub(crate) fn prepare_session_path(self, path: &str)
        -> ViResult<(crate::task::PreparedElfTask, crate::task::TaskLaunchState)> {
        if path.is_empty() || !path.starts_with('/') || path.len() > 4096
            || path.as_bytes().contains(&0) { return Err(ViError::InvalidInput); }
        let bytes = super::early::EarlyLoader::read_file(path)?;
        prepare_gated(&bytes, path, self)
    }
}

pub(super) fn spawn_trusted_init(elf: &[u8]) -> ViResult<usize> {
    spawn_init(elf, #[cfg(cellos_p01_trace)] false)
}

#[cfg(cellos_p01_trace)]
pub(super) fn spawn_bootstrap_init(elf: &[u8]) -> ViResult<usize> {
    spawn_init(elf, true)
}

fn spawn_init(elf: &[u8], #[cfg(cellos_p01_trace)] bootstrap_init: bool) -> ViResult<usize> {
    let granted = super::boot_ceiling::boot_ceiling("/bin/init");
    let prepared = crate::task::prepare_elf_task(elf, "init", CellId(0), alloc::vec::Vec::new())?;
    let state = crate::task::TaskLaunchState::complete(
        None,
        granted,
        None,
        None,
        crate::memory::cell_quota::default_quota_bytes(),
        u64::MAX,
        0,
        0,
        api::TaskPriority::Normal as u8,
        0,
        0,
        true,
        0,
        None,
        crate::task::LaunchRoutes {
            block_io: false,
            input: false,
            #[cfg(feature = "test-hooks")]
            development_silo: false,
        },
        Some(crate::task::StagedMeasurement {
            path: "/bin/init".to_string(),
            digest: crate::sha256::sha256(elf),
        }),
        false,
    );
    #[cfg(cellos_p01_trace)]
    let state = { let mut state = state; state.bootstrap_init = bootstrap_init; state };
    crate::task::publish_prepared(prepared, state).map(|(tid, _)| tid)
}
