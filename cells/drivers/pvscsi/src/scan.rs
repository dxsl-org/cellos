//! Target scan: the first direct-access disk on targets `0..TARGETS`.

use crate::error::PvscsiError;
use crate::queue::Adapter;
use crate::rings::DATA_LEN;
use ostd::io::print_fmt;
use scsi_block::{ProbeConfig, ScsiDisk, ScsiError, MAX_REPORTED_LUNS};

pub type Disk = ScsiDisk<Adapter>;

/// Targets scanned: VMware's PVSCSI bus offers 16 by default (QEMU's 64);
/// the first disk wins.
const TARGETS: u8 = 16;
/// TEST UNIT READY reissues after Retry-class sense (the power-on UNIT
/// ATTENTION after reset is the common case).
const TUR_RETRIES: u32 = 8;

/// Probes targets in order and returns the first disk; logs every other LUN
/// of its target as ignored. `None` (adapter dropped, device reset) when no
/// target has a disk or the adapter stopped working.
///
/// `ScsiDisk::probe` consumes its executor even on failure, so the scan
/// probes through `&mut adapter` to pick the target, then probes that target
/// once more with the owned adapter.
pub fn first_disk(mut adapter: Adapter) -> Option<Disk> {
    let config = ProbeConfig {
        tur_retries: TUR_RETRIES,
        max_transfer_bytes: DATA_LEN as u32,
    };
    let mut chosen = None;
    for target in 0..TARGETS {
        match ScsiDisk::probe(&mut adapter, target, config) {
            Ok(_) => {
                chosen = Some(target);
                break;
            }
            // Nothing at the target, or no disk behind it: keep scanning quietly.
            Err(ScsiError::Transport(PvscsiError::NoTarget) | ScsiError::NoDisk) => {}
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[pvscsi] target {target}: probe failed: {error:?}\n"
                ));
                if matches!(error, ScsiError::Transport(e) if e.poisons()) {
                    return None;
                }
            }
        }
    }
    let target = chosen?;
    let disk = match ScsiDisk::probe(adapter, target, config) {
        Ok(disk) => disk,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "[pvscsi] target {target}: re-probe failed: {error:?}\n"
            ));
            return None;
        }
    };
    for lun in disk.other_luns() {
        let _ = print_fmt(format_args!(
            "[pvscsi] ignoring lun target={target} lun={lun:02x?}\n"
        ));
    }
    if disk.luns_truncated() {
        let _ = print_fmt(format_args!(
            "[pvscsi] ignoring luns past the first {MAX_REPORTED_LUNS} on target={target}\n"
        ));
    }
    Some(disk)
}
