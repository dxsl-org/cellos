//! Target scan: the first direct-access disk on targets
//! `0..=Limits::last_target`.

use crate::config::Limits;
use crate::proto::{lun_number, VscsiError};
use crate::queue::RequestQueue;
use ostd::io::print_fmt;
use scsi_block::{ProbeConfig, ScsiDisk, ScsiError, MAX_REPORTED_LUNS};

pub type Disk = ScsiDisk<RequestQueue>;

/// TEST UNIT READY reissues after Retry-class sense (the power-on UNIT
/// ATTENTION after reset is the common case).
const TUR_RETRIES: u32 = 8;

/// Probes targets in order and returns the first disk; logs every other LUN
/// of its target as ignored. `None` (queue dropped, device reset) when no
/// target has a disk or the queue stopped working.
///
/// `ScsiDisk::probe` consumes its executor even on failure, so the scan
/// probes through `&mut queue` to pick the target, then probes that target
/// once more with the owned queue.
pub fn first_disk(mut queue: RequestQueue, limits: &Limits) -> Option<Disk> {
    let config = ProbeConfig {
        tur_retries: TUR_RETRIES,
        max_transfer_bytes: limits.max_transfer_bytes,
    };
    let mut chosen = None;
    for target in 0..=limits.last_target {
        match ScsiDisk::probe(&mut queue, target, config) {
            Ok(_) => {
                chosen = Some(target);
                break;
            }
            // Absent target or no disk behind it: keep scanning quietly.
            Err(ScsiError::Transport(VscsiError::Response(_)) | ScsiError::NoDisk) => {}
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[virtio-scsi] target {target}: probe failed: {error:?}\n"
                ));
                if matches!(error, ScsiError::Transport(e) if e.poisons()) {
                    return None;
                }
            }
        }
    }
    let target = chosen?;
    let disk = match ScsiDisk::probe(queue, target, config) {
        Ok(disk) => disk,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "[virtio-scsi] target {target}: re-probe failed: {error:?}\n"
            ));
            return None;
        }
    };
    for lun in disk.other_luns() {
        match lun_number(lun) {
            Some(number) => {
                let _ = print_fmt(format_args!(
                    "[virtio-scsi] ignoring lun target={target} lun={number}\n"
                ));
            }
            None => {
                let _ = print_fmt(format_args!(
                    "[virtio-scsi] ignoring lun target={target} lun={lun:02x?}\n"
                ));
            }
        }
    }
    if disk.luns_truncated() {
        let _ = print_fmt(format_args!(
            "[virtio-scsi] ignoring luns past the first {MAX_REPORTED_LUNS} on target={target}\n"
        ));
    }
    Some(disk)
}
