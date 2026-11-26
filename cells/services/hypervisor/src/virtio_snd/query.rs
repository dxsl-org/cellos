//! Item information requests (virtio 1.2 §5.14.6.1): JACK_INFO, PCM_INFO and
//! CHMAP_INFO over the device's fixed items.

extern crate alloc;

use alloc::vec::Vec;

use super::control::{
    le32, CHANNELS, CHMAPS, FMT_S16, JACKS, RATE_48000, R_JACK_INFO, R_PCM_INFO, STREAMS, S_BAD_MSG,
};

const D_OUTPUT: u8 = 0;
const CHMAP_FL: u8 = 3;
const CHMAP_FR: u8 = 4;
/// Largest per-item stride a query may ask for; bounds the reply.
const MAX_INFO_SIZE: usize = 1024;
const QUERY_LEN: usize = 16;
const PCM_INFO_LEN: usize = 32;
const CHMAP_INFO_LEN: usize = 24;

/// The items of an info query `code`, `size` bytes each (ours truncated or
/// zero-padded to it), or the status refusing it: a range past the device's
/// items, an unbounded stride, or a reply that does not fit `room`.
pub(super) fn query(code: u32, request: &[u8], room: usize) -> Result<Vec<u8>, u32> {
    if request.len() != QUERY_LEN {
        return Err(S_BAD_MSG);
    }
    let field = |at| le32(request, at).unwrap_or(0);
    let (start, count, size) = (field(4), field(8), field(12) as usize);
    let total = match code {
        R_JACK_INFO => JACKS,
        R_PCM_INFO => STREAMS,
        _ => CHMAPS,
    };
    if start.checked_add(count).is_none_or(|end| end > total)
        || size > MAX_INFO_SIZE
        || (count > 0 && size == 0)
    {
        return Err(S_BAD_MSG);
    }
    let len = (count as usize) * size;
    if room < 4 + len {
        return Err(S_BAD_MSG);
    }
    let mut items = Vec::with_capacity(len);
    for _ in start..start + count {
        let mut item = match code {
            R_PCM_INFO => pcm_info().to_vec(),
            // `total` is 0 for jacks, so only a channel map reaches here.
            _ => chmap_info().to_vec(),
        };
        item.resize(size, 0);
        items.extend_from_slice(&item);
    }
    Ok(items)
}

fn pcm_info() -> [u8; PCM_INFO_LEN] {
    let mut info = [0u8; PCM_INFO_LEN];
    // hda_fn_nid 0, features 0.
    info[8..16].copy_from_slice(&(1u64 << FMT_S16).to_le_bytes());
    info[16..24].copy_from_slice(&(1u64 << RATE_48000).to_le_bytes());
    info[24] = D_OUTPUT;
    info[25] = CHANNELS;
    info[26] = CHANNELS;
    info
}

fn chmap_info() -> [u8; CHMAP_INFO_LEN] {
    let mut info = [0u8; CHMAP_INFO_LEN];
    info[4] = D_OUTPUT;
    info[5] = CHANNELS;
    info[6] = CHMAP_FL;
    info[7] = CHMAP_FR;
    info
}
