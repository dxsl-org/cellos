// SPDX-License-Identifier: MPL-2.0
#![no_std]
#![no_main]

mod crypto;
mod layout;
mod mailbox;

const _: () = assert!(
    layout::GUEST_RAM_BYTES == layout::MAX_GUEST_BYTES + layout::PAGE_LEN
);

use core::arch::global_asm;
use crypto::{CryptoResult, SiloState};
use mailbox::{MailboxRequest, SiloCommand, HVC_SILO_DONE, HVC_SILO_FAULT, HVC_SILO_READY};
use layout::INPUT_LEN;

global_asm!(include_str!("arch/entry.s"));

const FAULT_PANIC: u64 = 0x7f;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { mailbox::hvc_signal(HVC_SILO_FAULT, FAULT_PANIC) };
    halt()
}

#[no_mangle]
pub extern "C" fn silo_main() -> ! {
    let mut state = SiloState::uninit();
    let mut last_request_seq = 0u64;
    loop {
        unsafe { core::arch::asm!("wfi") };
        let mut request = unsafe { mailbox::read_request() };
        let command = SiloCommand::from(request.command);
        let result = dispatch(&mut state, &mut request, command, &mut last_request_seq);
        if command == SiloCommand::Initialize {
            request.input.fill(0);
            core::hint::black_box(&request.input);
        }
        let (status, signal, detail_code, output): (u8, u64, u64, &[u8]) = match &result {
            CryptoResult::Ready(public) => (1, HVC_SILO_READY, 0, public),
            CryptoResult::Signature(signature) => (2, HVC_SILO_DONE, 0, signature),
            CryptoResult::Ack => (3, HVC_SILO_DONE, 0, &[]),
            CryptoResult::Absent => (4, HVC_SILO_DONE, 0, &[]),
            CryptoResult::Fault(code) => (
                0xff,
                HVC_SILO_FAULT,
                u64::from(*code),
                core::slice::from_ref(code),
            ),
        };
        unsafe {
            mailbox::publish_response(
                request.request_seq,
                request.command,
                status,
                output,
            );
            mailbox::hvc_signal(signal, detail_code);
        }
    }
}

fn dispatch(
    state: &mut SiloState,
    request: &mut MailboxRequest,
    command: SiloCommand,
    last_request_seq: &mut u64,
) -> CryptoResult {
    if request.request_seq == 0
        || request.request_seq <= *last_request_seq
        || request.response_seq != 0
        || request.status != 0
        || !request.reserved_zero
    {
        return CryptoResult::Fault(0x40);
    }
    *last_request_seq = request.request_seq;
    if command != SiloCommand::Unknown && !request.canonical_data {
        return CryptoResult::Fault(0x41);
    }
    match command {
        SiloCommand::Initialize => state.initialize_once(&mut request.input[..32].try_into().expect("seed")),
        SiloCommand::SignTls13ClientCertificateVerify => {
            state.sign_tls13_client_certificate_verify(
                request.input[..32].try_into().expect("hash"),
            )
        }
        SiloCommand::CreateEnrollmentKey => {
            let parsed = enrollment_input(&request.input);
            request.input[8..40].fill(0);
            core::hint::black_box(&request.input);
            match parsed {
                Ok((generation, mut nonce)) => {
                    state.create_enrollment_key(generation, &mut nonce)
                }
                Err(fault) => fault,
            }
        }
        SiloCommand::SignEnrollmentCri => {
            let len = request.input[8] as usize;
            if len > 64 || request.input[9 + len..].iter().any(|byte| *byte != 0) {
                return CryptoResult::Fault(0x27);
            }
            let generation =
                u64::from_le_bytes(request.input[..8].try_into().expect("generation"));
            state.sign_enrollment_cri(generation, &request.input[9..9 + len])
        }
        SiloCommand::DestroyEnrollmentKey => {
            match generation_input(&request.input) {
                Ok(generation) => state.destroy_enrollment_key(generation),
                Err(fault) => fault,
            }
        }
        SiloCommand::PromoteEnrollmentKey => {
            match generation_input(&request.input) {
                Ok(generation) => state.promote_enrollment_key(generation),
                Err(fault) => fault,
            }
        }
        SiloCommand::Unknown => CryptoResult::Fault(0x42),
    }
}

fn generation_input(input: &[u8; INPUT_LEN]) -> Result<u64, CryptoResult> {
    if input[8..].iter().any(|byte| *byte != 0) {
        return Err(CryptoResult::Fault(0x27));
    }
    let generation = u64::from_le_bytes(input[..8].try_into().expect("generation"));
    if generation == 0 {
        return Err(CryptoResult::Fault(0x27));
    }
    Ok(generation)
}

/// Command 3 carries one LE u64 generation and a required 32-byte nonce.
fn enrollment_input(input: &[u8; INPUT_LEN]) -> Result<(u64, [u8; 32]), CryptoResult> {
    if input[40..].iter().any(|byte| *byte != 0) {
        return Err(CryptoResult::Fault(0x27));
    }
    let generation = u64::from_le_bytes(input[..8].try_into().expect("generation"));
    let nonce: [u8; 32] = input[8..40].try_into().expect("nonce");
    if generation == 0 || nonce.iter().all(|byte| *byte == 0) {
        return Err(CryptoResult::Fault(0x27));
    }
    Ok((generation, nonce))
}

fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
