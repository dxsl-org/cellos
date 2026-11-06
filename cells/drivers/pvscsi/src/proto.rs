//! Request encoding, LUN re-encoding and completion decoding (pure;
//! host-tested). Everything the device writes back is untrusted: decoding
//! never panics, and a malformed completion is an error.

use crate::error::PvscsiError;
use crate::rings::{CmpDesc, ReqDesc, CMP_LEN, REQ_LEN, SENSE_LEN};
use core::mem::offset_of;
use scsi_block::{status, Completion, Sense};

/// Request `flags`: data direction.
const FLAG_DIR_NONE: u32 = 1 << 2;
const FLAG_DIR_TOHOST: u32 = 1 << 3;
const FLAG_DIR_TODEVICE: u32 = 1 << 4;
/// SCSI-2 SIMPLE QUEUE TAG message code.
const TAG_SIMPLE: u8 = 0x20;

/// Host adapter (`host_status`) codes the cell distinguishes.
const BTSTAT_SUCCESS: u16 = 0x00;
const BTSTAT_DATA_UNDERRUN: u16 = 0x0C;
const BTSTAT_SELTIMEO: u16 = 0x11;
const BTSTAT_DATARUN: u16 = 0x12;

/// The device LUN field for SAM LUN `sam`: single-level peripheral (`00 nn`)
/// or flat (`01 nn`, LUN < 256) addressing becomes `[0, nn, 0, …]`.
pub(crate) fn device_lun(sam: [u8; 8]) -> Result<[u8; 8], PvscsiError> {
    let single_level = sam[2..] == [0; 6];
    match sam[0] {
        0x00 | 0x40 if single_level => Ok([0, sam[1], 0, 0, 0, 0, 0, 0]),
        _ => Err(PvscsiError::UnsupportedLun(sam)),
    }
}

/// One request, as the cell fills it.
pub(crate) struct Request<'a> {
    pub context: u64,
    pub target: u8,
    pub lun: [u8; 8],
    pub cdb: &'a [u8],
    /// Data direction: `Some(true)` device→host, `Some(false)` host→device.
    pub to_host: Option<bool>,
    pub data_addr: u64,
    pub data_len: usize,
    pub sense_addr: u64,
}

/// Encode `request` into a 128-byte request descriptor.
pub(crate) fn encode_request(request: &Request<'_>) -> Result<[u8; REQ_LEN], PvscsiError> {
    if request.cdb.len() > 16 {
        return Err(PvscsiError::CdbTooLong(request.cdb.len()));
    }
    let lun = device_lun(request.lun)?;
    let (flags, data_addr, data_len) = match (request.data_len, request.to_host) {
        (0, _) | (_, None) => (FLAG_DIR_NONE, 0, 0),
        (len, Some(true)) => (FLAG_DIR_TOHOST, request.data_addr, len as u64),
        (len, Some(false)) => (FLAG_DIR_TODEVICE, request.data_addr, len as u64),
    };
    let mut d = [0u8; REQ_LEN];
    let mut put = |at: usize, bytes: &[u8]| d[at..at + bytes.len()].copy_from_slice(bytes);
    put(offset_of!(ReqDesc, context), &request.context.to_le_bytes());
    put(offset_of!(ReqDesc, data_addr), &data_addr.to_le_bytes());
    put(offset_of!(ReqDesc, data_len), &data_len.to_le_bytes());
    put(
        offset_of!(ReqDesc, sense_addr),
        &request.sense_addr.to_le_bytes(),
    );
    put(
        offset_of!(ReqDesc, sense_len),
        &(SENSE_LEN as u32).to_le_bytes(),
    );
    put(offset_of!(ReqDesc, flags), &flags.to_le_bytes());
    put(offset_of!(ReqDesc, cdb), request.cdb);
    put(offset_of!(ReqDesc, cdb_len), &[request.cdb.len() as u8]);
    put(offset_of!(ReqDesc, lun), &lun);
    put(offset_of!(ReqDesc, tag), &[TAG_SIMPLE]);
    put(offset_of!(ReqDesc, target), &[request.target]);
    Ok(d)
}

/// Decode a 32-byte completion for the request `context` that asked for
/// `requested` data bytes; `sense` is the sense area as the device left it.
pub(crate) fn decode_completion(
    cmp: &[u8; CMP_LEN],
    context: u64,
    requested: usize,
    sense: &[u8],
) -> Result<Completion, PvscsiError> {
    let bytes = |at: usize, n: usize| {
        let mut word = [0u8; 8];
        word[..n].copy_from_slice(&cmp[at..at + n]);
        u64::from_le_bytes(word)
    };
    if bytes(offset_of!(CmpDesc, context), 8) != context {
        return Err(PvscsiError::BadCompletion);
    }
    let reported = bytes(offset_of!(CmpDesc, data_len), 8);
    let residual = match bytes(offset_of!(CmpDesc, host_status), 2) as u16 {
        BTSTAT_SUCCESS => 0,
        // Short transfer. QEMU's model never fills `data_len`, so 0 means
        // "amount unknown": the bounce buffer was zeroed before the command,
        // and the disk layer parses what arrived by its own length fields.
        BTSTAT_DATARUN | BTSTAT_DATA_UNDERRUN => match reported {
            0 => 0,
            n if n <= requested as u64 => requested as u64 - n,
            _ => return Err(PvscsiError::BadCompletion),
        },
        BTSTAT_SELTIMEO => return Err(PvscsiError::NoTarget),
        other => return Err(PvscsiError::HostStatus(other)),
    };
    let Ok(status) = u8::try_from(bytes(offset_of!(CmpDesc, scsi_status), 2)) else {
        return Err(PvscsiError::BadCompletion);
    };
    let sense_len = bytes(offset_of!(CmpDesc, sense_len), 4) as usize;
    let sense = (status == status::CHECK_CONDITION && sense_len > 0)
        .then(|| Sense::decode(&sense[..sense_len.min(sense.len())]))
        .flatten();
    Ok(Completion {
        status,
        sense,
        residual: residual as u32,
    })
}

#[cfg(test)]
#[path = "proto_tests.rs"]
mod tests;
