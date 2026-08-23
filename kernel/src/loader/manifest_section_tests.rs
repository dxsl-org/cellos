//! Bare-metal behavioral tests for manifest-section admission.

use super::manifest_section::{classify, ManifestSection, ManifestVersion};
use crate::task::tcb::TaskState;
use api::manifest::{
    MANIFEST_FLAGS_MASK, MANIFEST_MAGIC, MANIFEST_VERSION, PROTECTION_CLASS_STANDARD,
};
use core::sync::atomic::Ordering;
use types::{CellId, ViError};

const NAMES: &[u8] = b"\0.shstrtab\0__ViCell_manifest\0";
const MANIFEST_OFFSET: usize = 96;
const SHOFF: usize = 128;
const SHT_NOBITS: u32 = 8;

pub(super) fn run_all() {
    test_supported_classes_and_absence_policy();
    test_valid_versions_classify_uniquely();
    test_named_nobits_manifest_is_malformed();
    test_malformed_images_deny_before_task_creation();
    log::info!("  [ok] manifest-section admission corpus");
}

fn manifest(version: u8, size: usize) -> alloc::vec::Vec<u8> {
    let mut bytes = alloc::vec![0u8; size];
    if size >= 4 {
        bytes[..4].copy_from_slice(&MANIFEST_MAGIC.to_le_bytes());
    }
    if size >= 5 {
        bytes[4] = version;
    }
    if version == MANIFEST_VERSION && size >= 8 {
        bytes[5] = PROTECTION_CLASS_STANDARD;
        bytes[6..8].copy_from_slice(&MANIFEST_FLAGS_MASK.to_le_bytes());
    }
    bytes
}

fn elf64(record: Option<&[u8]>, duplicate: bool) -> alloc::vec::Vec<u8> {
    let count = 2 + if record.is_some() { 1 } else { 0 } + if duplicate { 1 } else { 0 };
    let mut elf = alloc::vec![0u8; SHOFF + count * 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    put16(&mut elf, 16, 2);
    put16(&mut elf, 18, 243);
    put32(&mut elf, 20, 1);
    put16(&mut elf, 52, 64);
    put16(&mut elf, 58, 64);
    put16(&mut elf, 60, count as u16);
    put16(&mut elf, 62, 1);
    put64(&mut elf, 40, SHOFF as u64);
    elf[64..64 + NAMES.len()].copy_from_slice(NAMES);
    section64(&mut elf, 1, 1, 3, 64, NAMES.len());
    if let Some(bytes) = record {
        elf[MANIFEST_OFFSET..MANIFEST_OFFSET + bytes.len()].copy_from_slice(bytes);
        section64(&mut elf, 2, 11, 1, MANIFEST_OFFSET, bytes.len());
        if duplicate {
            section64(&mut elf, 3, 11, 1, MANIFEST_OFFSET, bytes.len());
        }
    }
    elf
}

fn elf32(record: &[u8]) -> alloc::vec::Vec<u8> {
    const SHOFF32: usize = 104;
    let mut elf = alloc::vec![0u8; SHOFF32 + 3 * 40];
    elf[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    put16(&mut elf, 16, 2);
    put16(&mut elf, 18, 243);
    put32(&mut elf, 20, 1);
    put32(&mut elf, 32, SHOFF32 as u32);
    put16(&mut elf, 40, 52);
    put16(&mut elf, 46, 40);
    put16(&mut elf, 48, 3);
    put16(&mut elf, 50, 1);
    elf[52..52 + NAMES.len()].copy_from_slice(NAMES);
    elf[84..84 + record.len()].copy_from_slice(record);
    section32(&mut elf, SHOFF32, 1, 1, 3, 52, NAMES.len());
    section32(&mut elf, SHOFF32, 2, 11, 1, 84, record.len());
    elf
}

fn section64(elf: &mut [u8], index: usize, name: u32, kind: u32, off: usize, size: usize) {
    let base = SHOFF + index * 64;
    put32(elf, base, name);
    put32(elf, base + 4, kind);
    put64(elf, base + 24, off as u64);
    put64(elf, base + 32, size as u64);
}

fn section32(
    elf: &mut [u8],
    shoff: usize,
    index: usize,
    name: u32,
    kind: u32,
    off: usize,
    size: usize,
) {
    let base = shoff + index * 40;
    put32(elf, base, name);
    put32(elf, base + 4, kind);
    put32(elf, base + 16, off as u32);
    put32(elf, base + 20, size as u32);
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn named_nobits_manifest() -> alloc::vec::Vec<u8> {
    let valid = manifest(MANIFEST_VERSION, 16);
    let mut image = elf64(Some(&valid), false);
    put32(&mut image, SHOFF + 2 * 64 + 4, SHT_NOBITS);
    image
}

fn test_named_nobits_manifest_is_malformed() {
    assert!(matches!(
        classify(&named_nobits_manifest()),
        ManifestSection::Malformed
    ));
}

#[derive(Debug, PartialEq)]
struct HartSchedulerSnapshot {
    hart_id: usize,
    current_cell_id: usize,
    current_task_id: usize,
    ready: alloc::collections::BTreeMap<u8, alloc::collections::VecDeque<usize>>,
}

#[derive(Debug, PartialEq)]
struct SchedulerSnapshot {
    tasks: alloc::vec::Vec<(usize, usize, CellId, TaskState)>,
    zombies: alloc::vec::Vec<(usize, CellId, TaskState)>,
    next_task_id: usize,
    last_global_sweep_tick: usize,
    harts: alloc::vec::Vec<HartSchedulerSnapshot>,
}

fn scheduler_snapshot() -> SchedulerSnapshot {
    let scheduler = crate::task::SCHEDULER.lock();
    let scheduler = scheduler
        .as_ref()
        .expect("manifest admission corpus requires an initialized scheduler");
    SchedulerSnapshot {
        tasks: scheduler
            .tasks
            .iter()
            .map(|(key, task)| (*key, task.id, task.cell_id, task.state.clone()))
            .collect(),
        zombies: scheduler
            .zombies
            .iter()
            .map(|task| (task.id, task.cell_id, task.state.clone()))
            .collect(),
        next_task_id: scheduler.next_task_id,
        last_global_sweep_tick: scheduler.last_global_sweep_tick,
        harts: crate::task::hart_local::HART_LOCALS
            .iter()
            .map(|hart| HartSchedulerSnapshot {
                hart_id: hart.hart_id,
                current_cell_id: hart.current_cell_id.load(Ordering::Acquire),
                current_task_id: hart.current_task_id.load(Ordering::Acquire),
                ready: hart.ready.lock().clone(),
            })
            .collect(),
    }
}

fn test_supported_classes_and_absence_policy() {
    let record = manifest(MANIFEST_VERSION, 16);
    assert!(matches!(
        classify(&elf32(&record)),
        ManifestSection::Valid { .. }
    ));
    assert!(matches!(
        classify(&elf64(None, false)),
        ManifestSection::Absent
    ));
    let legacy = super::legacy_path_caps("/bin/vfs");
    assert!(
        legacy.block_io,
        "manifest absence keeps the explicit legacy /bin policy"
    );
    assert_eq!(
        super::legacy_path_caps("/user/vfs"),
        crate::task::cap::CapSet::EMPTY
    );
}

fn test_valid_versions_classify_uniquely() {
    let v1 = manifest(1, 8);
    let v2 = manifest(MANIFEST_VERSION, 16);
    assert!(matches!(
        classify(&elf64(Some(&v1), false)),
        ManifestSection::Valid {
            version: ManifestVersion::V1,
            ..
        }
    ));
    assert!(matches!(
        classify(&elf64(Some(&v2), false)),
        ManifestSection::Valid {
            version: ManifestVersion::V2,
            ..
        }
    ));
}

fn test_malformed_images_deny_before_task_creation() {
    let valid = manifest(MANIFEST_VERSION, 16);
    let mut bad_magic = elf64(Some(&valid), false);
    bad_magic[0] = 0;
    let duplicate = elf64(Some(&valid), true);
    let truncated = elf64(Some(&manifest(MANIFEST_VERSION, 15)), false);
    let oversized = elf64(Some(&manifest(MANIFEST_VERSION, 17)), false);
    let mut unknown_version = manifest(3, 16);
    unknown_version[5] = PROTECTION_CLASS_STANDARD;
    let mut unknown_flag = valid.clone();
    unknown_flag[7] |= 0x10;
    let mut unknown_class = valid.clone();
    unknown_class[5] = 4;
    let mut reserved = valid.clone();
    reserved[12] = 1;
    let mut reserved_v1 = manifest(1, 8);
    reserved_v1[7] = 1;
    let mut bad_table = elf64(Some(&valid), false);
    put64(&mut bad_table, 40, u64::MAX);
    let mut bad_names = elf64(Some(&valid), false);
    put64(&mut bad_names, SHOFF + 64 + 24, u64::MAX);

    for (label, image) in [
        ("bad ELF", bad_magic),
        ("duplicate", duplicate),
        ("SHT_NOBITS", named_nobits_manifest()),
        ("truncated", truncated),
        ("oversized", oversized),
        ("unknown version", elf64(Some(&unknown_version), false)),
        ("unknown flag", elf64(Some(&unknown_flag), false)),
        ("unknown class", elf64(Some(&unknown_class), false)),
        ("reserved", elf64(Some(&reserved), false)),
        ("v1 reserved padding", elf64(Some(&reserved_v1), false)),
        ("bad section table", bad_table),
        ("bad name table", bad_names),
    ] {
        assert!(
            matches!(classify(&image), ManifestSection::Malformed),
            "{}",
            label
        );
        let before = scheduler_snapshot();
        assert_eq!(
            super::spawn_gated(
                &image,
                "/bin/manifest-negative",
                super::SpawnRequest::governed_boot(),
            ),
            Err(ViError::PermissionDenied),
            "{} must be denied",
            label
        );
        assert_eq!(
            scheduler_snapshot(),
            before,
            "{} changed scheduler state",
            label
        );
    }
}
