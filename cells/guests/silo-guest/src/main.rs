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
        SiloCommand::Initialize => state.initialize_once(&mut request.input),
        SiloCommand::SignTls13ClientCertificateVerify => {
            state.sign_tls13_client_certificate_verify(request.input)
        }
        SiloCommand::Unknown => CryptoResult::Fault(0x42),
    }
}

fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
