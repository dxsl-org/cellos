// SPDX-License-Identifier: MPL-2.0
//! Private host-to-guest mailbox for the development Silo.

use core::sync::atomic::{compiler_fence, Ordering};

use crate::layout::{
    COMMAND_CREATE_ENROLLMENT_KEY, COMMAND_DESTROY_ENROLLMENT_KEY, COMMAND_INITIALIZE,
    COMMAND_OFFSET, COMMAND_PROMOTE_ENROLLMENT_KEY, COMMAND_SIGN_ENROLLMENT_CRI,
    COMMAND_SIGN_TLS, DATA_OFFSET, INPUT_LEN, MAILBOX_IPA, PAGE_LEN, REQUEST_SEQ_OFFSET,
    RESERVED_OFFSET, RESPONSE_SEQ_OFFSET, STATUS_OFFSET,
};
pub use crate::layout::{HVC_SILO_DONE, HVC_SILO_FAULT, HVC_SILO_READY};

const MAILBOX: *mut u8 = MAILBOX_IPA as usize as *mut u8;

/// Purpose-restricted guest commands; initialization is host-internal only.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum SiloCommand {
    Initialize,
    SignTls13ClientCertificateVerify,
    CreateEnrollmentKey,
    SignEnrollmentCri,
    DestroyEnrollmentKey,
    PromoteEnrollmentKey,
    Unknown,
}

impl From<u8> for SiloCommand {
    fn from(value: u8) -> Self {
        match value {
            COMMAND_INITIALIZE => Self::Initialize,
            COMMAND_SIGN_TLS => Self::SignTls13ClientCertificateVerify,
            COMMAND_CREATE_ENROLLMENT_KEY => Self::CreateEnrollmentKey,
            COMMAND_SIGN_ENROLLMENT_CRI => Self::SignEnrollmentCri,
            COMMAND_DESTROY_ENROLLMENT_KEY => Self::DestroyEnrollmentKey,
            COMMAND_PROMOTE_ENROLLMENT_KEY => Self::PromoteEnrollmentKey,
            _ => Self::Unknown,
        }
    }
}

/// Bounded request snapshot; the 4 KiB shared page is never copied to the stack.
pub struct MailboxRequest {
    pub request_seq: u64,
    pub response_seq: u64,
    pub command: u8,
    pub status: u8,
    pub reserved_zero: bool,
    pub canonical_data: bool,
    pub input: [u8; INPUT_LEN],
}

const _: () = assert!(core::mem::size_of::<MailboxRequest>() <= 128);


/// Snapshot the request fields while checking every canonical padding byte.
///
/// # Safety
/// The VMM must keep the guest as the sole mailbox reader while it runs.
pub unsafe fn read_request() -> MailboxRequest {
    compiler_fence(Ordering::Acquire);
    let request_seq = read_word(REQUEST_SEQ_OFFSET);
    let response_seq = read_word(RESPONSE_SEQ_OFFSET);
    let command = read_byte(COMMAND_OFFSET);
    let status = read_byte(STATUS_OFFSET);
    let mut reserved_zero = true;
    for offset in RESERVED_OFFSET..DATA_OFFSET {
        reserved_zero &= read_byte(offset) == 0;
    }
    let mut input = [0u8; INPUT_LEN];
    for (index, byte) in input.iter_mut().enumerate() {
        *byte = read_byte(DATA_OFFSET + index);
    }
    let mut canonical_data = true;
    for offset in DATA_OFFSET + INPUT_LEN..PAGE_LEN {
        canonical_data &= read_byte(offset) == 0;
    }
    MailboxRequest {
        request_seq,
        response_seq,
        command,
        status,
        reserved_zero,
        canonical_data,
        input,
    }
}

/// Zero the shared request and publish one canonical response in place.
///
/// # Safety
/// The guest must hold exclusive mailbox access until the following HVC.
pub unsafe fn publish_response(
    request_seq: u64,
    command: u8,
    status: u8,
    output: &[u8],
) {
    for offset in (0..PAGE_LEN).step_by(core::mem::size_of::<u64>()) {
        write_word(offset, 0);
    }
    write_word(REQUEST_SEQ_OFFSET, request_seq);
    write_word(RESPONSE_SEQ_OFFSET, request_seq);
    write_byte(COMMAND_OFFSET, command);
    write_byte(STATUS_OFFSET, status);
    for (index, byte) in output.iter().enumerate() {
        write_byte(DATA_OFFSET + index, *byte);
    }
    compiler_fence(Ordering::Release);
}

/// Exit to the host only after the canonical response is fully published.
///
/// x0 carries the private Silo function ID. x1 carries a bounded fault code and
/// is zero for successful READY/DONE signals.
///
/// # Safety
/// `function_id` must be one of the declared Silo HVC identifiers.
pub unsafe fn hvc_signal(function_id: u64, detail_code: u64) {
    core::arch::asm!(
        "hvc #0",
        inlateout("x0") function_id => _,
        inlateout("x1") detail_code => _,
        options(nostack),
    );
}

unsafe fn read_byte(offset: usize) -> u8 {
    core::ptr::read_volatile(MAILBOX.add(offset))
}

unsafe fn read_word(offset: usize) -> u64 {
    u64::from_le(core::ptr::read_volatile(MAILBOX.add(offset).cast::<u64>()))
}

unsafe fn write_byte(offset: usize, value: u8) {
    core::ptr::write_volatile(MAILBOX.add(offset), value);
}

unsafe fn write_word(offset: usize, value: u64) {
    core::ptr::write_volatile(MAILBOX.add(offset).cast::<u64>(), value.to_le());
}
