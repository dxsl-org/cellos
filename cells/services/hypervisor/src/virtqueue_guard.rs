//! Pure validation rules for guest-provided split-virtqueue metadata.

pub fn pending_count(q_size: usize, last_avail: u16, avail: u16) -> Option<usize> {
    if q_size == 0 {
        return None;
    }
    let pending = avail.wrapping_sub(last_avail) as usize;
    (pending <= q_size).then_some(pending)
}

pub fn valid_descriptor(index: usize, q_size: usize) -> bool {
    index < q_size
}
