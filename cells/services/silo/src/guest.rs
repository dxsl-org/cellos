//! One-time guest initialization and purpose-bound mailbox execution.

use crate::{run_loop, vmm};
use ostd::syscall::sys_get_random;
use service_silo::{
    layout::{
        decode_fault_response, FaultResponseMetadata, COMMAND_CREATE_ENROLLMENT_KEY,
        COMMAND_DESTROY_ENROLLMENT_KEY, COMMAND_INITIALIZE, COMMAND_OFFSET,
        COMMAND_PROMOTE_ENROLLMENT_KEY, COMMAND_SIGN_ENROLLMENT_CRI, COMMAND_SIGN_TLS, DATA_OFFSET,
        INPUT_LEN, MAILBOX_IPA, PAGE_LEN, REQUEST_SEQ_OFFSET, STATUS_OFFSET,
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
    EnrollmentKeyAbsent,
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

    /// Return the current active P-256 public point.
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

    /// Create the fresh non-exportable key for one pending generation.
    ///
    /// The 32-byte nonce is fresh admitted entropy per call and is zeroized
    /// on the host side right after the mailbox write.
    pub fn create_enrollment_key(
        &mut self,
        pending_generation: u64,
        nonce: &[u8; 32],
    ) -> Result<[u8; 65], GuestError> {
        let mut input = [0u8; INPUT_LEN];
        input[..8].copy_from_slice(&pending_generation.to_le_bytes());
        input[8..40].copy_from_slice(nonce);
        let response = self.exchange(COMMAND_CREATE_ENROLLMENT_KEY, &input);
        input[8..40].fill(0);
        core::hint::black_box(&input);
        let response = response?;
        if response[STATUS_OFFSET] != 1
            || response[DATA_OFFSET + 65..].iter().any(|byte| *byte != 0)
        {
            self.faulted = true;
            return Err(GuestError::MalformedResponse);
        }
        let mut sec1 = [0u8; 65];
        sec1.copy_from_slice(&response[DATA_OFFSET..DATA_OFFSET + 65]);
        Ok(sec1)
    }

    /// Reconstruct the canonical CRI inside the guest and sign it raw.
    pub fn sign_enrollment_cri(
        &mut self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> Result<[u8; 64], GuestError> {
        if hostname.is_empty() || hostname.len() > 64 {
            return Err(GuestError::MalformedResponse);
        }
        let mut input = [0u8; INPUT_LEN];
        input[..8].copy_from_slice(&pending_generation.to_le_bytes());
        input[8] = hostname.len() as u8;
        input[9..9 + hostname.len()].copy_from_slice(hostname);
        let response = self.exchange(COMMAND_SIGN_ENROLLMENT_CRI, &input)?;
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

    /// Atomically promote the pending key to the active TLS signer; the
    /// guest retires the previous active key and returns its new public
    /// point, which becomes the session's cached status key.
    pub fn promote_enrollment_key(
        &mut self,
        pending_generation: u64,
    ) -> Result<[u8; 65], GuestError> {
        let mut input = [0u8; INPUT_LEN];
        input[..8].copy_from_slice(&pending_generation.to_le_bytes());
        let response = self.exchange(COMMAND_PROMOTE_ENROLLMENT_KEY, &input)?;
        if response[STATUS_OFFSET] != 1
            || response[DATA_OFFSET + 65..].iter().any(|byte| *byte != 0)
        {
            self.faulted = true;
            return Err(GuestError::MalformedResponse);
        }
        let mut sec1 = [0u8; 65];
        sec1.copy_from_slice(&response[DATA_OFFSET..DATA_OFFSET + 65]);
        if sec1[0] != 4 {
            return Err(GuestError::MalformedResponse);
        }
        self.public_key = sec1;
        Ok(sec1)
    }

    /// Destroy the pending generation key explicitly inside the guest.
    pub fn destroy_enrollment_key(&mut self, pending_generation: u64) -> Result<(), GuestError> {
        let mut input = [0u8; INPUT_LEN];
        input[..8].copy_from_slice(&pending_generation.to_le_bytes());
        let response = self.exchange(COMMAND_DESTROY_ENROLLMENT_KEY, &input)?;
        match response[STATUS_OFFSET] {
            3 => Ok(()),
            4 => Err(GuestError::EnrollmentKeyAbsent),
            _ => {
                self.faulted = true;
                Err(GuestError::MalformedResponse)
            }
        }
    }

    fn exchange(&mut self, command: u8, input: &[u8]) -> Result<[u8; PAGE_LEN], GuestError> {
        if self.faulted {
            return Err(GuestError::Reset);
        }
        let request_seq = self.next_request_seq;
        self.next_request_seq = request_seq
            .checked_add(1)
            .filter(|seq| *seq != 0)
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
        let Some(response_seq) =
            mailbox::validate_response(&page, request_seq, command, self.last_response_seq)
        else {
            self.faulted = true;
            return Err(GuestError::MalformedResponse);
        };
        self.last_response_seq = response_seq;
        Ok(page)
    }
}

impl PurposeGuest for GuestSession {
    type Error = GuestError;
    fn classify_destroy_error(error: &Self::Error) -> types::silo::DevelopmentSiloError {
        use types::silo::DevelopmentSiloError;
        match error {
            GuestError::EnrollmentKeyAbsent => DevelopmentSiloError::NoEnrollmentKey,
            GuestError::GuestFault(_) | GuestError::MalformedResponse => {
                DevelopmentSiloError::GuestFault
            }
            GuestError::EntropyUnavailable
            | GuestError::MailboxWrite
            | GuestError::MailboxRead
            | GuestError::VmmFault(_)
            | GuestError::Reset => DevelopmentSiloError::Unavailable,
        }
    }

    fn public_key(&self) -> [u8; 65] {
        GuestSession::public_key(self)
    }

    fn sign_tls13_client_certificate_verify(
        &mut self,
        transcript_hash: [u8; 32],
    ) -> Result<[u8; 64], Self::Error> {
        GuestSession::sign_tls13_client_certificate_verify(self, transcript_hash)
    }

    fn create_enrollment_key(
        &mut self,
        pending_generation: u64,
        nonce: &[u8; 32],
    ) -> Result<[u8; 65], Self::Error> {
        GuestSession::create_enrollment_key(self, pending_generation, nonce)
    }

    fn sign_enrollment_cri(
        &mut self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> Result<[u8; 64], Self::Error> {
        GuestSession::sign_enrollment_cri(self, pending_generation, hostname)
    }

    fn destroy_enrollment_key(&mut self, pending_generation: u64) -> Result<(), Self::Error> {
        GuestSession::destroy_enrollment_key(self, pending_generation)
    }

    fn promote_enrollment_key(&mut self, pending_generation: u64) -> Result<[u8; 65], Self::Error> {
        GuestSession::promote_enrollment_key(self, pending_generation)
    }
}
