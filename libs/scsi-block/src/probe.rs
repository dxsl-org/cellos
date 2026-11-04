//! Target discovery: REPORT LUNS → INQUIRY → TEST UNIT READY →
//! READ CAPACITY(16).

use crate::cdb;
use crate::disk::ScsiDisk;
use crate::error::ScsiError;
use crate::exec::{run, DataPhase, ScsiExecutor, LUN0};
use crate::parse::{self, INQUIRY_LEN};
use crate::sense::{sense_key, SenseClass};

/// Most LUNs one probe records. Sizes the REPORT LUNS allocation length;
/// longer lists are cut here and flagged via [`ScsiDisk::luns_truncated`].
pub const MAX_REPORTED_LUNS: usize = 32;

const REPORT_LUNS_LEN: usize = 8 + 8 * MAX_REPORTED_LUNS;

/// Caller policy for [`ScsiDisk::probe`]. The layer never sleeps; pacing
/// between retries is the executor's business.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeConfig {
    /// TEST UNIT READY reissues allowed after Retry-class sense
    /// ([`SenseClass::Retry`]); the command runs at most `tur_retries + 1`
    /// times.
    pub tur_retries: u32,
    /// Largest data phase one READ/WRITE command may carry, in bytes
    /// (transport limit, e.g. virtio-scsi `max_sectors * 512`).
    pub max_transfer_bytes: u32,
}

/// LUNs a target reported, in device order, bounded by
/// [`MAX_REPORTED_LUNS`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct ReportedLuns {
    entries: [[u8; 8]; MAX_REPORTED_LUNS],
    count: usize,
    pub(crate) truncated: bool,
}

impl ReportedLuns {
    fn single(lun: [u8; 8]) -> Self {
        let mut entries = [[0u8; 8]; MAX_REPORTED_LUNS];
        entries[0] = lun;
        Self {
            entries,
            count: 1,
            truncated: false,
        }
    }

    pub(crate) fn as_slice(&self) -> &[[u8; 8]] {
        &self.entries[..self.count]
    }
}

impl<E: ScsiExecutor> ScsiDisk<E> {
    /// Finds the first direct-access disk LUN on `target` and readies it.
    ///
    /// Steps: REPORT LUNS on LUN 0 (an ILLEGAL REQUEST response or an empty
    /// list falls back to probing LUN 0 alone); INQUIRY on each LUN in
    /// order, skipping LUNs whose INQUIRY fails with a SCSI status or
    /// malformed data; TEST UNIT READY with up to `config.tur_retries`
    /// reissues on Retry-class sense; READ CAPACITY(16).
    ///
    /// # Errors
    /// - [`ScsiError::Transport`] from any command, immediately.
    /// - [`ScsiError::NoDisk`] when no LUN is a connected disk.
    /// - [`ScsiError::NotReady`] when TEST UNIT READY retries run out.
    /// - [`ScsiError::TransferLimitBelowBlockSize`] when
    ///   `config.max_transfer_bytes` cannot hold one block.
    /// - Any other [`ScsiError`] from REPORT LUNS, TEST UNIT READY, or
    ///   READ CAPACITY(16).
    pub fn probe(
        mut exec: E,
        target: u8,
        config: ProbeConfig,
    ) -> Result<Self, ScsiError<E::Error>> {
        let luns = report_luns(&mut exec, target)?;
        let lun = first_disk(&mut exec, target, luns.as_slice())?.ok_or(ScsiError::NoDisk)?;
        wait_ready(&mut exec, target, lun, config.tur_retries)?;

        let mut data = [0u8; cdb::READ_CAPACITY_16_LEN];
        let got = run(
            &mut exec,
            target,
            lun,
            &cdb::read_capacity_16(),
            DataPhase::In(&mut data),
        )?;
        let capacity = parse::read_capacity_16(&data[..got])?;

        let max_blocks = config.max_transfer_bytes / capacity.block_size;
        if max_blocks == 0 {
            return Err(ScsiError::TransferLimitBelowBlockSize {
                max_transfer_bytes: config.max_transfer_bytes,
                block_size: capacity.block_size,
            });
        }
        Ok(ScsiDisk {
            exec,
            target,
            lun,
            capacity,
            max_blocks,
            luns,
        })
    }
}

fn report_luns<E: ScsiExecutor>(
    exec: &mut E,
    target: u8,
) -> Result<ReportedLuns, ScsiError<E::Error>> {
    let mut data = [0u8; REPORT_LUNS_LEN];
    let cdb = cdb::report_luns(REPORT_LUNS_LEN as u32);
    let got = match run(exec, target, LUN0, &cdb, DataPhase::In(&mut data)) {
        Ok(got) => got,
        Err(ScsiError::CheckCondition(sense)) if sense.key == sense_key::ILLEGAL_REQUEST => {
            return Ok(ReportedLuns::single(LUN0));
        }
        Err(err) => return Err(err),
    };
    let list = parse::report_luns(&data[..got])?;
    if list.is_empty() {
        return Ok(ReportedLuns::single(LUN0));
    }
    let mut luns = ReportedLuns {
        entries: [[0u8; 8]; MAX_REPORTED_LUNS],
        count: 0,
        truncated: list.truncated(),
    };
    // The buffer holds at most MAX_REPORTED_LUNS entries, so zip never drops one.
    for (slot, lun) in luns.entries.iter_mut().zip(list.iter()) {
        *slot = lun;
        luns.count += 1;
    }
    Ok(luns)
}

fn first_disk<E: ScsiExecutor>(
    exec: &mut E,
    target: u8,
    luns: &[[u8; 8]],
) -> Result<Option<[u8; 8]>, ScsiError<E::Error>> {
    for &lun in luns {
        let mut data = [0u8; INQUIRY_LEN];
        let cdb = cdb::inquiry(INQUIRY_LEN as u16);
        let got = match run(exec, target, lun, &cdb, DataPhase::In(&mut data)) {
            Ok(got) => got,
            Err(ScsiError::Transport(err)) => return Err(ScsiError::Transport(err)),
            // One misbehaving LUN must not hide a healthy disk behind it.
            Err(_) => continue,
        };
        if parse::inquiry(&data[..got]).is_ok_and(|inq| inq.is_disk()) {
            return Ok(Some(lun));
        }
    }
    Ok(None)
}

fn wait_ready<E: ScsiExecutor>(
    exec: &mut E,
    target: u8,
    lun: [u8; 8],
    retries: u32,
) -> Result<(), ScsiError<E::Error>> {
    let mut attempts_left = retries;
    loop {
        match run(exec, target, lun, &cdb::test_unit_ready(), DataPhase::None) {
            Ok(_) => return Ok(()),
            Err(ScsiError::CheckCondition(sense)) if sense.class() == SenseClass::Retry => {
                if attempts_left == 0 {
                    return Err(ScsiError::NotReady(sense));
                }
                attempts_left -= 1;
            }
            Err(err) => return Err(err),
        }
    }
}
