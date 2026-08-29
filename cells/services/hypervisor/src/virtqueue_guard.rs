//! Pure validation rules for guest-provided split-virtqueue metadata.

pub const MAX_QUEUE_SIZE: usize = 256;

pub fn valid_queue_size(q_size: usize) -> bool {
    q_size != 0 && q_size <= MAX_QUEUE_SIZE && q_size.is_power_of_two()
}

pub fn checked_gpa(base: u64, offset: u64, len: u64) -> Option<u64> {
    let address = base.checked_add(offset)?;
    address.checked_add(len)?;
    Some(address)
}

pub fn valid_payload_range(gpa: u64, len: u32) -> bool {
    checked_gpa(gpa, 0, len as u64).is_some()
}

pub fn valid_queue_config(q_size: u16, desc_gpa: u64, avail_gpa: u64, used_gpa: u64) -> bool {
    let q_size = q_size as usize;
    if !valid_queue_size(q_size)
        || desc_gpa == 0
        || desc_gpa & 15 != 0
        || avail_gpa == 0
        || avail_gpa & 1 != 0
        || used_gpa == 0
        || used_gpa & 3 != 0
    {
        return false;
    }

    let q_size = q_size as u64;
    let desc_len = q_size.checked_mul(16);
    let avail_len = q_size.checked_mul(2).and_then(|len| len.checked_add(4));
    let used_len = q_size.checked_mul(8).and_then(|len| len.checked_add(4));
    matches!(
        (desc_len, avail_len, used_len),
        (Some(desc_len), Some(avail_len), Some(used_len))
            if checked_gpa(desc_gpa, 0, desc_len).is_some()
                && checked_gpa(avail_gpa, 0, avail_len).is_some()
                && checked_gpa(used_gpa, 0, used_len).is_some()
    )
}

pub fn descriptor_gpa(desc_gpa: u64, index: usize, q_size: usize) -> Option<u64> {
    if !valid_queue_size(q_size) || !valid_descriptor(index, q_size) {
        return None;
    }
    let offset = (index as u64).checked_mul(16)?;
    checked_gpa(desc_gpa, offset, 16)
}

pub fn avail_entry_gpa(avail_gpa: u64, index: u16, q_size: usize) -> Option<u64> {
    if !valid_queue_size(q_size) {
        return None;
    }
    let slot = index as usize % q_size;
    let offset = (slot as u64).checked_mul(2)?.checked_add(4)?;
    checked_gpa(avail_gpa, offset, 2)
}

pub fn used_entry_gpa(used_gpa: u64, index: u16, q_size: usize) -> Option<u64> {
    if !valid_queue_size(q_size) {
        return None;
    }
    let slot = index as usize % q_size;
    let offset = (slot as u64).checked_mul(8)?.checked_add(4)?;
    checked_gpa(used_gpa, offset, 8)
}

pub fn pending_count(q_size: usize, last_avail: u16, avail: u16) -> Option<usize> {
    if !valid_queue_size(q_size) {
        return None;
    }
    let pending = avail.wrapping_sub(last_avail) as usize;
    (pending <= q_size).then_some(pending)
}

pub fn valid_descriptor(index: usize, q_size: usize) -> bool {
    valid_queue_size(q_size) && index < q_size
}

pub fn valid_descriptor_flags(flags: u16) -> bool {
    flags & !0x3 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_sizes_are_nonzero_bounded_powers_of_two() {
        assert!(valid_queue_size(1));
        assert!(valid_queue_size(MAX_QUEUE_SIZE));
        assert!(!valid_queue_size(0));
        assert!(!valid_queue_size(3));
        assert!(!valid_queue_size(MAX_QUEUE_SIZE + 1));
    }

    #[test]
    fn queue_config_requires_aligned_nonzero_bases_and_bounded_spans() {
        assert!(valid_queue_config(8, 0x1000, 0x2000, 0x3000));
        assert!(!valid_queue_config(8, 0, 0x2000, 0x3000));
        assert!(!valid_queue_config(8, 0x1008, 0x2000, 0x3000));
        assert!(!valid_queue_config(8, 0x1000, 0x2001, 0x3000));
        assert!(!valid_queue_config(8, 0x1000, 0x2000, 0x3002));
        assert!(!valid_queue_config(3, 0x1000, 0x2000, 0x3000));
        assert!(!valid_queue_config(1, u64::MAX - 15, 0x2000, 0x3000));
        assert!(!valid_queue_config(1, 0x1000, u64::MAX - 5, 0x3000));
        assert!(!valid_queue_config(1, 0x1000, 0x2000, u64::MAX - 11));
    }

    #[test]
    fn ring_and_descriptor_arithmetic_is_checked() {
        assert_eq!(checked_gpa(0x1000, 8, 4), Some(0x1008));
        assert_eq!(checked_gpa(u64::MAX, 1, 0), None);
        assert_eq!(checked_gpa(u64::MAX - 1, 0, 2), None);
        assert_eq!(descriptor_gpa(0x1000, 7, 8), Some(0x1070));
        assert_eq!(descriptor_gpa(0x1000, 8, 8), None);
        assert_eq!(descriptor_gpa(u64::MAX - 15, 0, 1), None);
        assert_eq!(avail_entry_gpa(0x2000, 9, 8), Some(0x2006));
        assert_eq!(used_entry_gpa(0x3000, 9, 8), Some(0x300c));
    }

    #[test]
    fn pending_delta_cannot_exceed_the_queue() {
        assert_eq!(pending_count(8, 10, 18), Some(8));
        assert_eq!(pending_count(8, 10, 19), None);
        assert_eq!(pending_count(8, u16::MAX - 2, 3), Some(6));
        assert_eq!(pending_count(0, 0, 0), None);
    }

    #[test]
    fn descriptor_indices_and_payload_ranges_are_bounded() {
        assert!(valid_descriptor(7, 8));
        assert!(!valid_descriptor(8, 8));
        assert!(!valid_descriptor(0, 0));
        assert!(valid_descriptor_flags(0));
        assert!(valid_descriptor_flags(3));
        assert!(!valid_descriptor_flags(4));
        assert!(valid_payload_range(0x1000, 4096));
        assert!(!valid_payload_range(u64::MAX - 3, 8));
    }
}
