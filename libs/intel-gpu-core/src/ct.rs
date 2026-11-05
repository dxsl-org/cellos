//! GuC command transport (CT): message headers from
//! `drivers/gpu/drm/xe/abi/guc_communication_ctb_abi.h` and
//! `abi/guc_messages_abi.h`, ring arithmetic from `h2g_write` / `g2h_read` in
//! `drivers/gpu/drm/xe/xe_guc_ct.c` (all MIT).
//!
//! The rings are dword arrays shared with the GuC. The caller owns the shared
//! memory and the descriptor (head/tail/status words) and must order its writes:
//! message dwords become visible before the descriptor `tail` that publishes them.

/// `struct guc_ct_buffer_desc` is 16 dwords.
pub const DESC_DWORDS: usize = 16;
pub const DESC_HEAD: usize = 0;
pub const DESC_TAIL: usize = 1;
pub const DESC_STATUS: usize = 2;
pub const STATUS_OVERFLOW: u32 = 1 << 0;
pub const STATUS_UNDERFLOW: u32 = 1 << 1;
pub const STATUS_MISMATCH: u32 = 1 << 2;
pub const STATUS_DISABLED: u32 = 1 << 3;

/// `CTB_H2G_BUFFER_SIZE` (16 KiB) and `CTB_G2H_BUFFER_SIZE` (128 KiB) in dwords.
pub const H2G_DWORDS: u32 = 16 * 1024 / 4;
pub const G2H_DWORDS: u32 = 128 * 1024 / 4;
/// `GUC_CTB_MAX_DWORDS`: payload dwords after the CT header.
pub const MAX_MSG_DWORDS: usize = 255;

/// `GUC_HXG_TYPE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HxgType {
    Request = 0,
    Event = 1,
    FastRequest = 2,
    NoResponseBusy = 3,
    NoResponseRetry = 5,
    ResponseFailure = 6,
    ResponseSuccess = 7,
}

impl HxgType {
    fn from_bits(bits: u32) -> Option<Self> {
        Some(match bits {
            0 => Self::Request,
            1 => Self::Event,
            2 => Self::FastRequest,
            3 => Self::NoResponseBusy,
            5 => Self::NoResponseRetry,
            6 => Self::ResponseFailure,
            7 => Self::ResponseSuccess,
            _ => return None,
        })
    }
}

/// CT header: FENCE 31:16, FORMAT 15:12 (= HXG, 0), NUM_DWORDS 7:0.
pub const fn ct_header(fence: u16, num_dwords: u8) -> u32 {
    (fence as u32) << 16 | num_dwords as u32
}

/// Host-originated HXG header: TYPE 30:28, ACTION|DATA0 27:0 (`MAKE_ACTION`).
pub const fn hxg_header(kind: HxgType, action: u32) -> u32 {
    (kind as u32) << 28 | (action & 0x0fff_ffff)
}

/// A decoded HXG header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hxg {
    pub from_guc: bool,
    pub kind: HxgType,
    /// AUX 27:0 (action + data0 for requests/events, data0 for responses).
    pub aux: u32,
}

pub fn decode_hxg(word: u32) -> Option<Hxg> {
    Some(Hxg {
        from_guc: word >> 31 == 1,
        kind: HxgType::from_bits((word >> 28) & 7)?,
        aux: word & 0x0fff_ffff,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtError {
    /// Message empty or longer than [`MAX_MSG_DWORDS`].
    BadLength,
    /// Head or tail outside the ring (`GUC_CTB_STATUS_OVERFLOW`).
    Overflow,
    /// A G2H header announces more dwords than were published (`UNDERFLOW`).
    Underflow,
    /// Not enough free space; retry after the GuC advances `head`.
    NoSpace,
    /// The message would cross the end: the tail was zero-filled and reset to 0.
    /// Publish the new tail, then retry (upstream returns `-EAGAIN`).
    Wrapped { new_tail: u32 },
    /// FORMAT field is not HXG.
    BadFormat,
}

/// Free dwords for a producer at `tail` (`CIRC_SPACE(tail, head, size)`).
pub fn space(size: u32, head: u32, tail: u32) -> u32 {
    head.wrapping_sub(tail.wrapping_add(1)) & (size - 1)
}

/// Write one H2G message. `action[0]` is the action word, the rest its data.
/// Returns the new tail to publish in the descriptor.
pub fn h2g_write(
    ring: &mut [u32],
    head: u32,
    tail: u32,
    fence: u16,
    kind: HxgType,
    action: &[u32],
) -> Result<u32, CtError> {
    let size = ring.len() as u32;
    debug_assert!(size.is_power_of_two());
    if action.is_empty() || action.len() > MAX_MSG_DWORDS {
        return Err(CtError::BadLength);
    }
    if head >= size || tail >= size {
        return Err(CtError::Overflow);
    }
    let full = action.len() as u32 + 1;
    if full > space(size, head, tail) {
        return Err(CtError::NoSpace);
    }
    if tail + full > size {
        ring[tail as usize..].fill(0);
        return Err(CtError::Wrapped { new_tail: 0 });
    }
    let t = tail as usize;
    ring[t] = ct_header(fence, action.len() as u8);
    ring[t + 1] = hxg_header(kind, action[0]);
    ring[t + 2..t + full as usize].copy_from_slice(&action[1..]);
    Ok((tail + full) % size)
}

/// Read one G2H message into `out` (header excluded). Returns `None` when the ring
/// is empty, else the fence, the payload length and the new head to publish.
pub fn g2h_read(
    ring: &[u32],
    head: u32,
    tail: u32,
    out: &mut [u32; MAX_MSG_DWORDS],
) -> Result<Option<(u16, usize, u32)>, CtError> {
    let size = ring.len() as u32;
    debug_assert!(size.is_power_of_two());
    if head >= size || tail >= size {
        return Err(CtError::Overflow);
    }
    if head == tail {
        return Ok(None);
    }
    let available = tail.wrapping_sub(head) & (size - 1);
    let header = ring[head as usize];
    if (header >> 12) & 0xf != 0 {
        return Err(CtError::BadFormat);
    }
    let len = (header & 0xff) as usize;
    if len as u32 + 1 > available {
        return Err(CtError::Underflow);
    }
    for (i, slot) in out.iter_mut().take(len).enumerate() {
        *slot = ring[((head + 1 + i as u32) % size) as usize];
    }
    Ok(Some((
        (header >> 16) as u16,
        len,
        (head + 1 + len as u32) % size,
    )))
}

#[cfg(test)]
mod tests;
