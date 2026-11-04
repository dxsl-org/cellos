//! Parsers for device data-in. Callers pass only the bytes actually
//! transferred (buffer length minus residual).

use crate::error::ParseError;

/// Peripheral device type of a direct-access block device (SBC).
pub const DEVICE_TYPE_DISK: u8 = 0x00;
/// Peripheral qualifier: a device of this type is connected to the LUN.
pub const QUALIFIER_CONNECTED: u8 = 0x0;
/// Allocation length for standard INQUIRY: the SPC-4 mandatory 36 bytes.
pub const INQUIRY_LEN: usize = 36;

/// The INQUIRY fields that decide whether a LUN is a usable disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inquiry {
    /// PERIPHERAL QUALIFIER (byte 0 bits 7–5).
    pub qualifier: u8,
    /// PERIPHERAL DEVICE TYPE (byte 0 bits 4–0).
    pub device_type: u8,
}

impl Inquiry {
    /// Whether this LUN is a connected direct-access block device.
    pub fn is_disk(&self) -> bool {
        self.qualifier == QUALIFIER_CONNECTED && self.device_type == DEVICE_TYPE_DISK
    }
}

/// Parses standard INQUIRY data.
///
/// # Errors
/// [`ParseError::InquiryTooShort`] when fewer than the 5 header bytes
/// (through ADDITIONAL LENGTH) arrived.
pub fn inquiry(data: &[u8]) -> Result<Inquiry, ParseError> {
    if data.len() < 5 {
        return Err(ParseError::InquiryTooShort(data.len()));
    }
    Ok(Inquiry {
        qualifier: data[0] >> 5,
        device_type: data[0] & 0x1F,
    })
}

/// LUN entries of a REPORT LUNS response, borrowed from the data-in buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LunList<'a> {
    entries: &'a [u8],
    truncated: bool,
}

impl<'a> LunList<'a> {
    /// Number of complete 8-byte LUN entries available.
    pub fn len(&self) -> usize {
        self.entries.len() / 8
    }

    /// Whether no LUN entry is available.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether LUN LIST LENGTH announced more entries than the transferred
    /// bytes contain (the allocation length was too small).
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The available LUNs in device order, as 8-byte SAM LUNs.
    pub fn iter(&self) -> impl Iterator<Item = [u8; 8]> + 'a {
        self.entries.chunks_exact(8).map(|chunk| {
            let mut lun = [0u8; 8];
            lun.copy_from_slice(chunk);
            lun
        })
    }
}

/// Parses REPORT LUNS parameter data.
///
/// The entry count is bounded by both LUN LIST LENGTH (bytes 0–3) and the
/// transferred bytes; a list length that is not a multiple of 8 is rounded
/// down to whole entries.
///
/// # Errors
/// [`ParseError::ReportLunsTooShort`] when fewer than the 8 header bytes
/// arrived.
pub fn report_luns(data: &[u8]) -> Result<LunList<'_>, ParseError> {
    let Some((header, body)) = data.split_first_chunk::<8>() else {
        return Err(ParseError::ReportLunsTooShort(data.len()));
    };
    let header_len = [header[0], header[1], header[2], header[3]];
    let announced = usize::try_from(u32::from_be_bytes(header_len)).unwrap_or(usize::MAX);
    let usable = announced.min(body.len());
    let whole = usable - usable % 8;
    Ok(LunList {
        entries: &body[..whole],
        truncated: announced > body.len(),
    })
}

/// Capacity reported by READ CAPACITY(16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacity {
    /// Number of logical blocks (RETURNED LOGICAL BLOCK ADDRESS + 1).
    pub block_count: u64,
    /// Logical block length in bytes: a power of two, at least 512.
    pub block_size: u32,
}

/// Parses READ CAPACITY(16) parameter data.
///
/// # Errors
/// - [`ParseError::CapacityTooShort`]: fewer than 12 bytes arrived.
/// - [`ParseError::CapacityOverflow`]: last LBA is `u64::MAX`.
/// - [`ParseError::BadBlockSize`]: block length is not a power of two >= 512.
pub fn read_capacity_16(data: &[u8]) -> Result<Capacity, ParseError> {
    let Some(fields) = data.first_chunk::<12>() else {
        return Err(ParseError::CapacityTooShort(data.len()));
    };
    let (last, size) = fields.split_at(8);
    let mut last_lba = [0u8; 8];
    last_lba.copy_from_slice(last);
    let mut block_size = [0u8; 4];
    block_size.copy_from_slice(size);
    let block_size = u32::from_be_bytes(block_size);
    if block_size < 512 || !block_size.is_power_of_two() {
        return Err(ParseError::BadBlockSize(block_size));
    }
    let block_count = u64::from_be_bytes(last_lba)
        .checked_add(1)
        .ok_or(ParseError::CapacityOverflow)?;
    Ok(Capacity {
        block_count,
        block_size,
    })
}
