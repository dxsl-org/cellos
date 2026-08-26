//! Pure mailbox response validation shared with the runtime guest session.

use crate::layout::{
    COMMAND_OFFSET, DATA_OFFSET, PAGE_LEN, REQUEST_SEQ_OFFSET, RESERVED_OFFSET,
    RESPONSE_SEQ_OFFSET,
};

/// Validate the common canonical response envelope and return its fresh sequence.
pub fn validate_response(
    page: &[u8],
    expected_request_seq: u64,
    expected_command: u8,
    last_response_seq: u64,
) -> Option<u64> {
    if page.len() != PAGE_LEN {
        return None;
    }
    let request_seq = word(page, REQUEST_SEQ_OFFSET)?;
    let response_seq = word(page, RESPONSE_SEQ_OFFSET)?;
    (request_seq == expected_request_seq
        && response_seq > last_response_seq
        && page[COMMAND_OFFSET] == expected_command
        && page[RESERVED_OFFSET..DATA_OFFSET]
            .iter()
            .all(|byte| *byte == 0))
    .then_some(response_seq)
}

fn word(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical() -> [u8; PAGE_LEN] {
        let mut page = [0u8; PAGE_LEN];
        page[REQUEST_SEQ_OFFSET..REQUEST_SEQ_OFFSET + 8].copy_from_slice(&4u64.to_le_bytes());
        page[RESPONSE_SEQ_OFFSET..RESPONSE_SEQ_OFFSET + 8].copy_from_slice(&7u64.to_le_bytes());
        page[COMMAND_OFFSET] = 2;
        page
    }

    #[test]
    fn accepts_only_canonical_fresh_mailbox_response() {
        let page = canonical();
        assert_eq!(validate_response(&page, 4, 2, 6), Some(7));
        assert_eq!(validate_response(&page[..PAGE_LEN - 1], 4, 2, 6), None);
        assert_eq!(validate_response(&page, 5, 2, 6), None);
        assert_eq!(validate_response(&page, 4, 1, 6), None);
        assert_eq!(validate_response(&page, 4, 2, 7), None);

        let mut padded = page;
        padded[RESERVED_OFFSET] = 1;
        assert_eq!(validate_response(&padded, 4, 2, 6), None);
    }
}
