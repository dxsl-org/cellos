//! virtio-scsi request/response wire format (virtio 1.2 §5.6.6.1) and LUN
//! re-encoding. Pure: no device access, host-tested.
//!
//! Everything read back from the device is untrusted; decoding never panics.

use scsi_block::{Completion, Sense};

/// `lun[8] + id(le64) + task_attr + prio + crn`, before `cdb[cdb_size]`.
pub const REQ_FIXED: usize = 19;
/// `sense_len(le32) + residual(le32) + status_qualifier(le16) + status +
/// response`, before `sense[sense_size]`.
pub const RESP_FIXED: usize = 12;
/// `VIRTIO_SCSI_S_SIMPLE`.
const TASK_ATTR_SIMPLE: u8 = 0;
/// `VIRTIO_SCSI_S_OK`: the command reached the LUN and `status` is valid.
pub const RESPONSE_OK: u8 = 0;
/// Highest LUN the virtio-scsi flat-space LUN field can carry (14 bits).
const MAX_LUN: u16 = 0x3FFF;

/// Why the request queue produced no SAM status for a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VscsiError {
    /// The 8-byte SAM LUN uses an addressing method virtio-scsi cannot carry.
    UnsupportedLun([u8; 8]),
    /// The CDB is longer than the device's `cdb_size`.
    CdbTooLong(usize),
    /// The data phase exceeds the bounce buffer.
    DataTooLarge(usize),
    /// Non-OK virtio-scsi `response` (e.g. 3 = BAD_TARGET).
    Response(u8),
    /// No completion within the I/O budget; the queue is now poisoned.
    Timeout,
    /// The used ring returned something never submitted; the queue is now
    /// poisoned.
    BadCompletion,
    /// An earlier timeout or bad completion left the queue unusable.
    Poisoned,
}

impl VscsiError {
    /// Whether the device was (or may have been) notified of the command.
    pub fn issued(&self) -> bool {
        matches!(
            self,
            Self::Response(_) | Self::Timeout | Self::BadCompletion
        )
    }

    /// Whether the queue refuses every later command after this error.
    pub fn poisons(&self) -> bool {
        matches!(self, Self::Timeout | Self::BadCompletion | Self::Poisoned)
    }
}

/// Logical unit number of a single-level SAM LUN in peripheral (bus 0) or
/// flat addressing; `None` for any other form.
pub fn lun_number(sam: [u8; 8]) -> Option<u16> {
    if sam[2..] != [0; 6] {
        return None;
    }
    match sam[0] >> 6 {
        0 if sam[0] == 0 => Some(u16::from(sam[1])),
        1 => Some((u16::from(sam[0] & 0x3F) << 8) | u16::from(sam[1])),
        _ => None,
    }
}

/// virtio-scsi `lun` field for `sam` on `target`:
/// `[1, target, 0x40 | lun >> 8, lun & 0xff, 0, 0, 0, 0]`.
pub fn virtio_lun(target: u8, sam: [u8; 8]) -> Result<[u8; 8], VscsiError> {
    let lun = lun_number(sam)
        .filter(|&lun| lun <= MAX_LUN)
        .ok_or(VscsiError::UnsupportedLun(sam))?;
    Ok([1, target, 0x40 | (lun >> 8) as u8, lun as u8, 0, 0, 0, 0])
}

/// Encodes `virtio_scsi_req_cmd`'s device-readable part into `out` with the
/// CDB zero-padded to `cdb_size`; returns the encoded length.
///
/// `out` must hold `REQ_FIXED + cdb_size` bytes (the caller sizes it from
/// validated limits).
pub fn encode_req(
    out: &mut [u8],
    lun: [u8; 8],
    tag: u64,
    cdb: &[u8],
    cdb_size: usize,
) -> Result<usize, VscsiError> {
    if cdb.len() > cdb_size {
        return Err(VscsiError::CdbTooLong(cdb.len()));
    }
    let len = REQ_FIXED + cdb_size;
    let out = &mut out[..len];
    out[..8].copy_from_slice(&lun);
    out[8..16].copy_from_slice(&tag.to_le_bytes());
    out[16] = TASK_ATTR_SIMPLE;
    out[17] = 0; // prio
    out[18] = 0; // crn
    out[REQ_FIXED..REQ_FIXED + cdb.len()].copy_from_slice(cdb);
    out[REQ_FIXED + cdb.len()..].fill(0);
    Ok(len)
}

/// Decodes `virtio_scsi_resp_cmd` (`RESP_FIXED + sense_size` bytes in
/// `resp`) into a [`Completion`]. Sense is decoded from
/// `min(sense_len, sense bytes present)` bytes when `sense_len > 0`.
pub fn decode_resp(resp: &[u8]) -> Result<Completion, VscsiError> {
    let Some(fixed) = resp.get(..RESP_FIXED) else {
        return Err(VscsiError::BadCompletion);
    };
    let word =
        |at: usize| u32::from_le_bytes([fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]]);
    let response = fixed[11];
    if response != RESPONSE_OK {
        return Err(VscsiError::Response(response));
    }
    let sense_area = &resp[RESP_FIXED..];
    let sense_len = (word(0) as usize).min(sense_area.len());
    let sense = (sense_len > 0)
        .then(|| Sense::decode(&sense_area[..sense_len]))
        .flatten();
    Ok(Completion {
        status: fixed[10],
        sense,
        residual: word(4),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use scsi_block::{sense_key, status};

    #[test]
    fn peripheral_and_flat_luns_map_to_virtio_layout() {
        assert_eq!(virtio_lun(0, [0; 8]), Ok([1, 0, 0x40, 0, 0, 0, 0, 0]));
        assert_eq!(
            virtio_lun(3, [0, 7, 0, 0, 0, 0, 0, 0]),
            Ok([1, 3, 0x40, 7, 0, 0, 0, 0])
        );
        assert_eq!(
            virtio_lun(1, [0x41, 0x23, 0, 0, 0, 0, 0, 0]),
            Ok([1, 1, 0x41, 0x23, 0, 0, 0, 0])
        );
        assert_eq!(lun_number([0x7F, 0xFF, 0, 0, 0, 0, 0, 0]), Some(MAX_LUN));
    }

    #[test]
    fn unsupported_luns_are_refused() {
        for sam in [
            [0x01, 0, 0, 0, 0, 0, 0, 0], // peripheral, bus 1
            [0x80, 1, 0, 0, 0, 0, 0, 0], // logical unit addressing
            [0xC0, 1, 0, 0, 0, 0, 0, 0], // extended addressing
            [0, 1, 0, 1, 0, 0, 0, 0],    // second level present
        ] {
            assert_eq!(virtio_lun(0, sam), Err(VscsiError::UnsupportedLun(sam)));
        }
    }

    #[test]
    fn request_header_is_padded_to_cdb_size() {
        let mut out = [0xAAu8; 64];
        let lun = [1, 2, 0x40, 5, 0, 0, 0, 0];
        let len = encode_req(
            &mut out,
            lun,
            0x0102_0304_0506_0708,
            &[0x12, 0, 0, 0, 36, 0],
            32,
        )
        .unwrap();
        assert_eq!(len, REQ_FIXED + 32);
        assert_eq!(&out[..8], &lun);
        assert_eq!(&out[8..16], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(&out[16..19], &[0, 0, 0]);
        assert_eq!(&out[19..25], &[0x12, 0, 0, 0, 36, 0]);
        assert!(out[25..len].iter().all(|&b| b == 0));
        assert_eq!(out[len], 0xAA);
        assert_eq!(
            encode_req(&mut out, lun, 0, &[0; 17], 16),
            Err(VscsiError::CdbTooLong(17))
        );
    }

    fn resp(sense_len: u32, residual: u32, status: u8, response: u8, sense: &[u8]) -> [u8; 40] {
        let mut out = [0u8; 40];
        out[..4].copy_from_slice(&sense_len.to_le_bytes());
        out[4..8].copy_from_slice(&residual.to_le_bytes());
        out[10] = status;
        out[11] = response;
        out[RESP_FIXED..RESP_FIXED + sense.len()].copy_from_slice(sense);
        out
    }

    #[test]
    fn good_and_check_condition_responses_decode() {
        let done = decode_resp(&resp(0, 16, status::GOOD, 0, &[])).unwrap();
        assert_eq!(
            (done.status, done.sense, done.residual),
            (status::GOOD, None, 16)
        );

        let fixed = [
            0x70,
            0,
            sense_key::UNIT_ATTENTION,
            0,
            0,
            0,
            0,
            10,
            0,
            0,
            0,
            0,
            0x29,
            0,
        ];
        let done = decode_resp(&resp(200, 0, status::CHECK_CONDITION, 0, &fixed)).unwrap();
        let sense = done.sense.unwrap();
        assert_eq!((sense.key, sense.asc), (sense_key::UNIT_ATTENTION, 0x29));
    }

    #[test]
    fn non_ok_response_and_truncated_buffer_are_errors() {
        let bad_target = decode_resp(&resp(0, 0, 0, 3, &[]));
        assert_eq!(bad_target, Err(VscsiError::Response(3)));
        assert!(VscsiError::Response(3).issued());
        assert!(!VscsiError::Poisoned.issued());
        assert!(VscsiError::Timeout.poisons() && !VscsiError::Response(3).poisons());
        assert_eq!(decode_resp(&[0; 11]), Err(VscsiError::BadCompletion));
    }
}
