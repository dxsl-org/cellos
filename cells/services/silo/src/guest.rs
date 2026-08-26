//! One-time guest initialization and purpose-bound mailbox execution.

use crate::{run_loop, vmm};
use ostd::syscall::sys_get_random;
use service_silo::{
    layout::{
        decode_fault_response, FaultResponseMetadata, COMMAND_INITIALIZE, COMMAND_OFFSET,
        COMMAND_SIGN_TLS, DATA_OFFSET, MAILBOX_IPA, PAGE_LEN, REQUEST_SEQ_OFFSET, STATUS_OFFSET,
    },
    mailbox,
    protocol::PurposeGuest,
};

/// Exact guest fault signal plus its canonical mailbox diagnostic, if published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestFault {
    pub hvc_code: u64,
    pub response: Option<FaultResponseMetadata>,
}

/// Bounded guest session failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestError {
    EntropyUnavailable,
    MailboxWrite,
    MailboxRead,
    MalformedResponse,
    GuestFault(GuestFault),
    VmmFault(run_loop::SiloVmmFault),
    Reset,
}

/// Initialized guest session; any execution fault permanently closes it.
pub struct GuestSession {
    vm_id: usize,
    vcpu_id: usize,
    next_request_seq: u64,
    last_response_seq: u64,
    public_key: [u8; 65],
    faulted: bool,
}

impl GuestSession {
    /// Seed the guest exactly once from admitted kernel entropy.
    pub fn initialize(vm_id: usize, vcpu_id: usize) -> Result<Self, GuestError> {
        let mut seed = [0u8; 32];
        let mut filled = 0;
        for _ in 0..64 {
            if filled == seed.len() {
                break;
            }
            filled += sys_get_random(&mut seed[filled..]);
        }
        if filled != seed.len() {
            seed.fill(0);
            core::hint::black_box(&seed);
            return Err(GuestError::EntropyUnavailable);
        }
        let mut session = Self {
            vm_id,
            vcpu_id,
            next_request_seq: 1,
            last_response_seq: 0,
            public_key: [0; 65],
            faulted: false,
        };
        let response = session.exchange(COMMAND_INITIALIZE, &seed);
        seed.fill(0);
        core::hint::black_box(&seed);
        let response = response?;
        if response[STATUS_OFFSET] != 1
            || response[DATA_OFFSET + 65..].iter().any(|byte| *byte != 0)
        {
            return Err(GuestError::MalformedResponse);
        }
        session
            .public_key
            .copy_from_slice(&response[DATA_OFFSET..DATA_OFFSET + 65]);
        if session.public_key[0] != 4 {
            return Err(GuestError::MalformedResponse);
        }
        Ok(session)
    }

    /// Return the public P-256 point captured at one-time initialization.
    pub const fn public_key(&self) -> [u8; 65] {
        self.public_key
    }

    /// Execute exactly one TLS CertificateVerify operation.
    pub fn sign_tls13_client_certificate_verify(
        &mut self,
        transcript_hash: [u8; 32],
    ) -> Result<[u8; 64], GuestError> {
        let response = self.exchange(COMMAND_SIGN_TLS, &transcript_hash)?;
        if response[STATUS_OFFSET] != 2
            || response[DATA_OFFSET + 64..].iter().any(|byte| *byte != 0)
        {
            self.faulted = true;
            return Err(GuestError::MalformedResponse);
        }
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&response[DATA_OFFSET..DATA_OFFSET + 64]);
        Ok(signature)
    }

    fn exchange(&mut self, command: u8, input: &[u8]) -> Result<[u8; PAGE_LEN], GuestError> {
        if self.faulted {
            return Err(GuestError::Reset);
        }
        let request_seq = self.next_request_seq;
        self.next_request_seq = request_seq.checked_add(1).filter(|seq| *seq != 0)
            .ok_or(GuestError::Reset)?;
        let mut page = [0u8; PAGE_LEN];
        page[REQUEST_SEQ_OFFSET..REQUEST_SEQ_OFFSET + 8]
            .copy_from_slice(&request_seq.to_le_bytes());
        page[COMMAND_OFFSET] = command;
        page[DATA_OFFSET..DATA_OFFSET + input.len()].copy_from_slice(input);
        let written = vmm::write_guest_memory(self.vm_id, MAILBOX_IPA, &page);
        page[DATA_OFFSET..DATA_OFFSET + input.len()].fill(0);
        core::hint::black_box(&page);
        if written != PAGE_LEN {
            self.faulted = true;
            return Err(GuestError::MailboxWrite);
        }
        match run_loop::run_until_done(self.vm_id, self.vcpu_id) {
            run_loop::SiloRunResult::Done => {}
            run_loop::SiloRunResult::GuestFault(hvc_code) => {
                self.faulted = true;
                let response =
                    if vmm::read_guest_memory(self.vm_id, MAILBOX_IPA, &mut page) == PAGE_LEN {
                        Some(decode_fault_response(&page, request_seq, command))
                    } else {
                        None
                    };
                page.fill(0);
                core::hint::black_box(&page);
                return Err(GuestError::GuestFault(GuestFault { hvc_code, response }));
            }
            run_loop::SiloRunResult::VmmFault(fault) => {
                self.faulted = true;
                return Err(GuestError::VmmFault(fault));
            }
        }
        if vmm::read_guest_memory(self.vm_id, MAILBOX_IPA, &mut page) != PAGE_LEN {
            self.faulted = true;
            return Err(GuestError::MailboxRead);
        }
        let Some(response_seq) = mailbox::validate_response(
            &page,
            request_seq,
            command,
            self.last_response_seq,
        ) else {
            self.faulted = true;
            return Err(GuestError::MalformedResponse);
        };
        self.last_response_seq = response_seq;
        Ok(page)
    }
}

impl PurposeGuest for GuestSession {
    type Error = GuestError;

    fn public_key(&self) -> [u8; 65] {
        GuestSession::public_key(self)
    }

    fn sign_tls13_client_certificate_verify(
        &mut self,
        transcript_hash: [u8; 32],
    ) -> Result<[u8; 64], Self::Error> {
        GuestSession::sign_tls13_client_certificate_verify(self, transcript_hash)
    }
}

