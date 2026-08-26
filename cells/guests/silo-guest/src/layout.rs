//! Shared Silo guest memory layout.

pub const PAGE_LEN: usize = 4096;
pub const GUEST_IPA_BASE: u64 = 0x4000_0000;
pub const GUEST_RAM_PAGES: usize = 16;
pub const GUEST_RAM_BYTES: usize = GUEST_RAM_PAGES * PAGE_LEN;
pub const MAX_GUEST_BYTES: usize = (GUEST_RAM_PAGES - 1) * PAGE_LEN;
pub const MAILBOX_IPA: u64 = GUEST_IPA_BASE + MAX_GUEST_BYTES as u64;

// Private development mailbox/HVC protocol. The service library includes this
// source file directly, so host and guest cannot drift independently.
pub const REQUEST_SEQ_OFFSET: usize = 0;
pub const RESPONSE_SEQ_OFFSET: usize = 8;
pub const COMMAND_OFFSET: usize = 16;
pub const STATUS_OFFSET: usize = 17;
pub const RESERVED_OFFSET: usize = 18;
pub const DATA_OFFSET: usize = 24;
pub const INPUT_LEN: usize = 96;

pub const COMMAND_INITIALIZE: u8 = 1;
pub const COMMAND_SIGN_TLS: u8 = 2;
/// Phase 3 enrollment extension: fresh per-generation key creation.
pub const COMMAND_CREATE_ENROLLMENT_KEY: u8 = 3;
/// Phase 3 enrollment extension: independent CRI reconstruction and signing.
pub const COMMAND_SIGN_ENROLLMENT_CRI: u8 = 4;
/// Phase 3 enrollment extension: explicit key destruction.
pub const COMMAND_DESTROY_ENROLLMENT_KEY: u8 = 5;
/// Phase 3 enrollment extension: atomic promotion to active signer.
pub const COMMAND_PROMOTE_ENROLLMENT_KEY: u8 = 6;

pub const HVC_SILO_READY: u64 = 0xC600_0080;
pub const HVC_SILO_DONE: u64 = 0xC600_0081;
pub const HVC_SILO_FAULT: u64 = 0xC600_0082;

const _: () = {
    assert!(REQUEST_SEQ_OFFSET + core::mem::size_of::<u64>() == RESPONSE_SEQ_OFFSET);
    assert!(RESPONSE_SEQ_OFFSET + core::mem::size_of::<u64>() == COMMAND_OFFSET);
    assert!(COMMAND_OFFSET + 1 == STATUS_OFFSET);
    assert!(STATUS_OFFSET + 1 == RESERVED_OFFSET);
    assert!(RESERVED_OFFSET <= DATA_OFFSET);
    assert!(DATA_OFFSET + INPUT_LEN <= PAGE_LEN);
};

/// Non-secret metadata from a guest-declared fault response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultResponseMetadata {
    pub request_seq: u64,
    pub response_seq: u64,
    pub status: u8,
    /// Present only when the complete response page is canonical.
    pub mailbox_code: Option<u8>,
}

/// Decode fault metadata without ever treating request seed bytes as a fault code.
pub fn decode_fault_response(
    page: &[u8; PAGE_LEN],
    expected_request_seq: u64,
    expected_command: u8,
) -> FaultResponseMetadata {
    let request_seq = word(page, REQUEST_SEQ_OFFSET);
    let response_seq = word(page, RESPONSE_SEQ_OFFSET);
    let status = page[STATUS_OFFSET];
    let canonical = request_seq == expected_request_seq
        && response_seq == expected_request_seq
        && page[COMMAND_OFFSET] == expected_command
        && status == 0xff
        && page[RESERVED_OFFSET..DATA_OFFSET]
            .iter()
            .all(|byte| *byte == 0)
        && page[DATA_OFFSET + 1..].iter().all(|byte| *byte == 0);
    FaultResponseMetadata {
        request_seq,
        response_seq,
        status,
        mailbox_code: if canonical {
            Some(page[DATA_OFFSET])
        } else {
            None
        },
    }
}

fn word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().ok().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_code_requires_a_canonical_response_page() {
        let request_seq = 7u64;
        let mut page = [0u8; PAGE_LEN];
        page[REQUEST_SEQ_OFFSET..REQUEST_SEQ_OFFSET + 8]
            .copy_from_slice(&request_seq.to_le_bytes());
        page[RESPONSE_SEQ_OFFSET..RESPONSE_SEQ_OFFSET + 8]
            .copy_from_slice(&request_seq.to_le_bytes());
        page[COMMAND_OFFSET] = COMMAND_INITIALIZE;
        page[STATUS_OFFSET] = 0xff;
        page[DATA_OFFSET] = 0x41;

        assert_eq!(
            decode_fault_response(&page, request_seq, COMMAND_INITIALIZE),
            FaultResponseMetadata {
                request_seq,
                response_seq: request_seq,
                status: 0xff,
                mailbox_code: Some(0x41),
            }
        );

        page[DATA_OFFSET + 1] = 0xa5;
        assert_eq!(
            decode_fault_response(&page, request_seq, COMMAND_INITIALIZE).mailbox_code,
            None,
        );
    }
}
