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

fn cluster_mode(elf: &[u8]) -> u8 {
    super::ElfLoader
        .get_section(elf, "__ViCell_cluster")
        .ok()
        .filter(|bytes| bytes.len() >= 16)
        .map_or(0, |bytes| bytes[0])
}

/// The task that asked for this launch, for audit records; the kernel for boot
/// and trusted-init launches (no caller).
fn spawner_subject(request: &super::SpawnRequest) -> crate::audit::Subject {
    request
        .caller
        .as_ref()
        .map_or(crate::audit::Subject::KERNEL, |caller| {
            crate::audit::Subject {
                cell_id: caller.cell_id,
                generation: caller.generation,
                tid: caller.tid as u64,
            }
        })
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
                &crate::audit::encode_subject(spawner_subject(&request), 1, 0),
            );
            super::atomic_checkpoint("AP-00")?;
            return Err(ViError::PermissionDenied);
        }
    };

    // Signature extraction, byte coverage, and verification stay exactly at the
    // established boundary: structural classification first, task creation later.
    let signature = crate::signing::extract_sig(elf);
    let is_domain = match signature {
        Some(sig) => match crate::signing::verify_cell(elf, &sig) {
            Ok(true) => {
                crate::audit::log_event(
                    crate::audit::AuditEvent::CellSignatureVerified,
                    &crate::audit::encode_u32x2(0, 0),
                );
                manifest.as_ref().is_some_and(|m| {
                    m.protection_class() == abi::manifest::PROTECTION_CLASS_FFI
                        || m.protection_class() == abi::manifest::PROTECTION_CLASS_UNTRUSTED
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
                    &crate::audit::encode_subject(spawner_subject(&request), 3, 0),
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
                // Release posture: every cell is signed; the manifest picks the tier.
                if !crate::signing::unsigned_admitted() {
                    crate::audit::log_event(
                        crate::audit::AuditEvent::CellSpawnDenied,
                        &crate::audit::encode_subject(spawner_subject(&request), 4, 0),
                    );
                    return Err(ViError::PermissionDenied);
                }
                // Unsigned artifacts remain Tier 2 in the development posture.
                true
            }
        }
    };
    // Past this point a present signature has verified (every other outcome
    // returned above).
    let image = super::image_name_section::measured_identity(elf, elf_digest, signature.is_some());

    // Common owner admission gate: verify owner authorization for this ELF
    // before capability derivation or task creation.
    if let Err(err) = crate::admission::evaluate_owner_admission(&elf_digest) {
        crate::audit::log_event(
            crate::audit::AuditEvent::CellSpawnDenied,
            &crate::audit::encode_subject(spawner_subject(&request), 2, 0),
        );
        return Err(err);
    }
    if let Some(manifest) = manifest.as_ref() {
        if !path.starts_with("/bin/") && manifest.declares_any_privilege() {
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSpawnDenied,
                &crate::audit::encode_subject(spawner_subject(&request), 0, manifest.flags as u32),
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
    let cluster_mode = cluster_mode(elf);
    if request.priority > abi::TaskPriority::RealTime as u8
        || (request.priority == abi::TaskPriority::RealTime as u8 && cluster_mode != 0)
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
        .unwrap_or(abi::manifest::PROTECTION_CLASS_LEGACY);
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
    #[cfg(feature = "p01-trace")]
    let _heap_origin = crate::memory::heap::p01_owner::scope(request.trace);
    #[cfg(feature = "p01-trace")]
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
        // Kernel-resolved path: a `/mem/` label matches no row and gets 0.
        crate::catalog::find(path).map_or(0, |row| row.guest_memory),
        allowlist,
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
            image,
            launch_named: request.kernel_resolved_path
                && crate::catalog::names_launch_identity(path),
        }),
        is_domain,
    );
    #[cfg(feature = "p01-trace")]
    let state = {
        let mut state = state;
        state.trace = request.trace;
        state
    };
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
    pub(crate) fn prepare_session_path(
        self,
        path: &str,
    ) -> ViResult<(crate::task::PreparedElfTask, crate::task::TaskLaunchState)> {
        session_path_valid(path)?;
        let bytes = super::early::EarlyLoader::read_file(path)?;
        prepare_gated(&bytes, path, self)
    }

    /// Route 2: the same admission over caller-supplied bytes (`path` is the
    /// advisory edge/measurement name). Signature, manifest, measurement and
    /// domain classification are exactly route 1's; the edge was authorized as
    /// `LaunchRoute::Elf` by the caller of this function.
    pub(crate) fn prepare_session_elf(
        self,
        elf: &[u8],
        path: &str,
    ) -> ViResult<(crate::task::PreparedElfTask, crate::task::TaskLaunchState)> {
        session_path_valid(path)?;
        prepare_gated(elf, path, self)
    }
}

fn session_path_valid(path: &str) -> ViResult<()> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.len() > 4096
        || path.as_bytes().contains(&0)
    {
        return Err(ViError::InvalidInput);
    }
    Ok(())
}

pub(super) fn spawn_trusted_init(elf: &[u8]) -> ViResult<usize> {
    spawn_init(
        elf,
        #[cfg(feature = "p01-trace")]
        false,
    )
}

#[cfg(feature = "p01-trace")]
pub(super) fn spawn_bootstrap_init(elf: &[u8]) -> ViResult<usize> {
    spawn_init(elf, true)
}

fn spawn_init(elf: &[u8], #[cfg(feature = "p01-trace")] bootstrap_init: bool) -> ViResult<usize> {
    let granted = super::boot_ceiling::boot_ceiling("/bin/init");
    let prepared = crate::task::prepare_elf_task(elf, "init", CellId(0), alloc::vec::Vec::new())?;
    let init_digest = crate::sha256::sha256(elf);
    let state = crate::task::TaskLaunchState::complete(
        None,
        granted,
        None,
        None,
        crate::memory::cell_quota::default_quota_bytes(),
        0,
        u64::MAX,
        abi::TaskPriority::Normal as u8,
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
            digest: init_digest,
            // Trusted init is embedded in the kernel image, not admitted
            // through signature verification, so it states a digest only.
            image: abi::cell_image::ImageIdentity::unsigned(init_digest),
            // Init is named by the trusted-init latch, not by an image.
            launch_named: false,
        }),
        false,
    );
    let mut state = state;
    state.trusted_init = true;
    #[cfg(feature = "p01-trace")]
    let state = {
        let mut state = state;
        state.bootstrap_init = bootstrap_init;
        state
    };
    let tid = crate::task::publish_prepared(prepared, state).map(|(tid, _)| tid)?;
    Ok(tid)
}
