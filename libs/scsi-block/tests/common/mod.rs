//! Scripted executor shared by the integration tests.
#![allow(dead_code)]

use std::collections::VecDeque;

use scsi_block::{status, Completion, DataPhase, ProbeConfig, ScsiExecutor, Sense};

/// Probe policy most tests use: 3 TUR retries, 64 KiB transfers.
pub const CONFIG: ProbeConfig = ProbeConfig {
    tur_retries: 3,
    max_transfer_bytes: 64 * 1024,
};

/// TEST UNIT READY (GOOD) then READ CAPACITY(16) replies.
pub fn ready_tail(blocks: u64, size: u32) -> [Reply; 2] {
    [Reply::good(), Reply::data(&capacity_data(blocks, size))]
}

/// A fake whose target reports only LUN 0, a disk; later replies follow
/// TEST UNIT READY onward.
pub fn lun0_disk() -> FakeExecutor {
    FakeExecutor::new([
        Reply::data(&report_luns_data(&[lun(0)])),
        Reply::data(&inquiry_data(0, 0)),
    ])
}

/// Transport error the fake raises when told to or when its script runs out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FakeError;

/// One scripted device reply.
#[derive(Clone, Debug)]
pub struct Reply {
    pub status: u8,
    pub sense: Option<Sense>,
    pub data_in: Vec<u8>,
    /// `None`: residual = buffer length minus bytes copied in (or 0 for out).
    pub residual: Option<u32>,
    pub transport_error: bool,
}

impl Reply {
    pub fn good() -> Self {
        Self {
            status: status::GOOD,
            sense: None,
            data_in: Vec::new(),
            residual: None,
            transport_error: false,
        }
    }

    pub fn data(data: &[u8]) -> Self {
        Self {
            data_in: data.to_vec(),
            ..Self::good()
        }
    }

    pub fn check(key: u8, asc: u8, ascq: u8) -> Self {
        Self {
            status: status::CHECK_CONDITION,
            sense: Some(Sense { key, asc, ascq }),
            ..Self::good()
        }
    }

    pub fn status(status: u8) -> Self {
        Self {
            status,
            ..Self::good()
        }
    }

    pub fn residual(mut self, residual: u32) -> Self {
        self.residual = Some(residual);
        self
    }

    pub fn transport() -> Self {
        Self {
            transport_error: true,
            ..Self::good()
        }
    }
}

/// A command the fake received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub target: u8,
    pub lun: [u8; 8],
    pub cdb: Vec<u8>,
    pub data_out: Vec<u8>,
    pub data_len: usize,
}

#[derive(Default)]
pub struct FakeExecutor {
    pub script: VecDeque<Reply>,
    pub calls: Vec<Call>,
}

impl FakeExecutor {
    pub fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            script: replies.into_iter().collect(),
            calls: Vec::new(),
        }
    }

    pub fn opcodes(&self) -> Vec<u8> {
        self.calls.iter().map(|call| call.cdb[0]).collect()
    }
}

impl ScsiExecutor for FakeExecutor {
    type Error = FakeError;

    fn execute(
        &mut self,
        target: u8,
        lun: [u8; 8],
        cdb: &[u8],
        data: DataPhase<'_>,
    ) -> Result<Completion, FakeError> {
        let data_len = data.len();
        let reply = self.script.pop_front().ok_or(FakeError)?;
        let mut copied = 0usize;
        let mut data_out = Vec::new();
        match data {
            DataPhase::None => {}
            DataPhase::In(buf) => {
                copied = reply.data_in.len().min(buf.len());
                buf[..copied].copy_from_slice(&reply.data_in[..copied]);
            }
            DataPhase::Out(buf) => {
                copied = buf.len();
                data_out = buf.to_vec();
            }
        }
        self.calls.push(Call {
            target,
            lun,
            cdb: cdb.to_vec(),
            data_out,
            data_len,
        });
        if reply.transport_error {
            return Err(FakeError);
        }
        Ok(Completion {
            status: reply.status,
            sense: reply.sense,
            residual: reply.residual.unwrap_or((data_len - copied) as u32),
        })
    }
}

/// Standard INQUIRY data with the given qualifier and device type.
pub fn inquiry_data(qualifier: u8, device_type: u8) -> Vec<u8> {
    let mut data = vec![0u8; 36];
    data[0] = (qualifier << 5) | device_type;
    data[4] = 31;
    data
}

/// REPORT LUNS data listing `luns`.
pub fn report_luns_data(luns: &[[u8; 8]]) -> Vec<u8> {
    let mut data = ((luns.len() * 8) as u32).to_be_bytes().to_vec();
    data.extend_from_slice(&[0; 4]);
    for lun in luns {
        data.extend_from_slice(lun);
    }
    data
}

/// READ CAPACITY(16) data for `blocks` blocks of `block_size` bytes.
pub fn capacity_data(blocks: u64, block_size: u32) -> Vec<u8> {
    let mut data = vec![0u8; 32];
    data[..8].copy_from_slice(&(blocks - 1).to_be_bytes());
    data[8..12].copy_from_slice(&block_size.to_be_bytes());
    data
}

/// Single-level peripheral-addressed SAM LUN.
pub fn lun(n: u8) -> [u8; 8] {
    [0, n, 0, 0, 0, 0, 0, 0]
}
