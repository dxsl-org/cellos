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

pub(super) fn spawn_gated(
    elf: &[u8],
    path: &str,
    mut request: super::SpawnRequest,
) -> ViResult<usize> {
    use crate::task::cap::{CapSet, Spawner};
    let aligned = super::aligned_elf::bytes(elf);
    let elf = aligned.as_ref();
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
    match crate::signing::extract_sig(elf) {
        Some(sig) if !crate::signing::verify_cell(elf, &sig) => {
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSignatureFailed,
                &crate::audit::encode_u32x2(0, 0),
            );
            return Err(ViError::PermissionDenied);
        }
        Some(_) => crate::audit::log_event(
            crate::audit::AuditEvent::CellSignatureVerified,
            &crate::audit::encode_u32x2(0, 0),
        ),
        None if crate::signing::signing_required() => {
            crate::audit::log_event(
                crate::audit::AuditEvent::CellSignatureFailed,
                &crate::audit::encode_u32x2(0, 0),
            );
            return Err(ViError::PermissionDenied);
        }
        None => {}
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
    let prepared = crate::task::prepare_elf_task(elf, name, CellId(0), alloc::vec::Vec::new())?;
    let state = crate::task::TaskLaunchState::complete(
        request.caller.take(),
        granted,
        platform,
        request.replacement.take(),
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
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
        },
        Some(crate::task::StagedMeasurement {
            path: path.to_string(),
            digest: crate::sha256::sha256(elf),
        }),
    );
    crate::task::publish_prepared(prepared, state).map(|(tid, _)| tid)
}

pub(super) fn spawn_trusted_init(elf: &[u8]) -> ViResult<usize> {
    let granted = super::boot_ceiling::boot_ceiling("/bin/init");
    let prepared = crate::task::prepare_elf_task(elf, "init", CellId(0), alloc::vec::Vec::new())?;
    let state = crate::task::TaskLaunchState::complete(
        None,
        granted,
        None,
        None,
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
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
        },
        Some(crate::task::StagedMeasurement {
            path: "/bin/init".to_string(),
            digest: crate::sha256::sha256(elf),
        }),
    );
    crate::task::publish_prepared(prepared, state).map(|(tid, _)| tid)
}
