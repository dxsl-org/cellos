use crate::{Error, Result};

pub const XMODEM_BLOCK_LEN: usize = 1024;
pub const XMODEM_FRAME_LEN: usize = 1029;
pub const XMODEM_STX: u8 = 0x02;
pub const XMODEM_EOT: u8 = 0x04;
pub const XMODEM_PADDING: u8 = 0x1a;

/// Computes sender-transcript length (STX frames plus one EOT) for a nonempty stream.
pub fn xmodem_encoded_len(logical_len: usize, max_blocks: u32) -> Result<usize> {
    let blocks = block_count(logical_len, max_blocks)?;
    blocks
        .checked_mul(XMODEM_FRAME_LEN)
        .and_then(|n| n.checked_add(1))
        .ok_or(Error::Overflow)
}

/// Encodes a deterministic XMODEM-1K sender transcript, padding only the last payload block.
pub fn encode_xmodem(logical: &[u8], out: &mut [u8], max_blocks: u32) -> Result<usize> {
    let blocks = block_count(logical.len(), max_blocks)?;
    let encoded_len = xmodem_encoded_len(logical.len(), max_blocks)?;
    if out.len() < encoded_len {
        return Err(Error::OutputTooSmall);
    }
    for index in 0..blocks {
        let frame_start = index.checked_mul(XMODEM_FRAME_LEN).ok_or(Error::Overflow)?;
        let block = (index + 1) as u8;
        out[frame_start] = XMODEM_STX;
        out[frame_start + 1] = block;
        out[frame_start + 2] = !block;
        let payload = &mut out[frame_start + 3..frame_start + 3 + XMODEM_BLOCK_LEN];
        payload.fill(XMODEM_PADDING);
        let source_start = index.checked_mul(XMODEM_BLOCK_LEN).ok_or(Error::Overflow)?;
        let source_block_end = source_start
            .checked_add(XMODEM_BLOCK_LEN)
            .ok_or(Error::Overflow)?;
        let source_end = core::cmp::min(source_block_end, logical.len());
        if source_start < source_end {
            payload[..source_end - source_start]
                .copy_from_slice(&logical[source_start..source_end]);
        }
        let crc = crc16_xmodem(payload).to_be_bytes();
        out[frame_start + 3 + XMODEM_BLOCK_LEN..frame_start + XMODEM_FRAME_LEN]
            .copy_from_slice(&crc);
    }
    out[encoded_len - 1] = XMODEM_EOT;
    Ok(encoded_len)
}

/// Decodes and authenticates the framing of an exact sender transcript into caller storage.
/// Returns the padded payload byte count. EOT is mandatory and must be the final byte.
/// On error `out` may contain earlier CRC-valid blocks and remains untrusted.
pub fn decode_xmodem(transcript: &[u8], out: &mut [u8], max_blocks: u32) -> Result<usize> {
    let max_blocks = usize::try_from(max_blocks).map_err(|_| Error::Overflow)?;
    if max_blocks == 0 {
        return Err(Error::LimitExceeded);
    }
    let mut pos = 0usize;
    let mut blocks = 0usize;
    loop {
        let marker = *transcript.get(pos).ok_or(Error::MissingEot)?;
        if marker == XMODEM_EOT {
            if blocks == 0 {
                return Err(Error::InvalidFrame);
            }
            if pos + 1 != transcript.len() {
                return Err(Error::TrailingData);
            }
            return blocks.checked_mul(XMODEM_BLOCK_LEN).ok_or(Error::Overflow);
        }
        if marker != XMODEM_STX {
            return Err(Error::InvalidFrame);
        }
        if blocks >= max_blocks {
            return Err(Error::LimitExceeded);
        }
        let frame_end = pos.checked_add(XMODEM_FRAME_LEN).ok_or(Error::Overflow)?;
        let frame = transcript.get(pos..frame_end).ok_or(Error::MissingEot)?;
        let expected = (blocks + 1) as u8;
        if frame[1] != expected || frame[2] != !expected {
            return Err(Error::InvalidBlock);
        }
        let payload = &frame[3..3 + XMODEM_BLOCK_LEN];
        let received =
            u16::from_be_bytes([frame[XMODEM_FRAME_LEN - 2], frame[XMODEM_FRAME_LEN - 1]]);
        if crc16_xmodem(payload) != received {
            return Err(Error::InvalidCrc);
        }
        let output_start = blocks
            .checked_mul(XMODEM_BLOCK_LEN)
            .ok_or(Error::Overflow)?;
        let output_end = output_start
            .checked_add(XMODEM_BLOCK_LEN)
            .ok_or(Error::Overflow)?;
        out.get_mut(output_start..output_end)
            .ok_or(Error::OutputTooSmall)?
            .copy_from_slice(payload);
        blocks += 1;
        pos = frame_end;
    }
}

/// CRC-16/XMODEM over one frame payload (polynomial 0x1021, initial value zero).
pub fn crc16_xmodem(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn block_count(logical_len: usize, max_blocks: u32) -> Result<usize> {
    let max_blocks = usize::try_from(max_blocks).map_err(|_| Error::Overflow)?;
    if logical_len == 0 || max_blocks == 0 {
        return Err(Error::LimitExceeded);
    }
    let blocks = logical_len
        .checked_add(XMODEM_BLOCK_LEN - 1)
        .ok_or(Error::Overflow)?
        / XMODEM_BLOCK_LEN;
    if blocks > max_blocks {
        return Err(Error::LimitExceeded);
    }
    Ok(blocks)
}
