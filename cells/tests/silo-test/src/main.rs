//! KMS-mediated authorization probe for the contained development Silo lane.
#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);
api::declare_syscalls![Send, Recv, RecvTimeout, Log, LookupService];

use ostd::clients::kms::{KmsClient, KmsClientError};
use ostd::io::println;
use ostd::syscall::{sys_lookup_service, sys_recv_timeout, sys_send, SyscallResult};
use types::kms::KmsErrorCode;
use types::silo::{
    DevelopmentSiloError, DevelopmentSiloRequest, DevelopmentSiloResponse,
    DEVELOPMENT_PROFILE_DIGEST, DEVELOPMENT_RELAY_GENERATION, DEVELOPMENT_SILO_FRAME_LEN,
};

ostd::cell_main!(cell_main);

fn cell_main() {
    println("[silo-test] KMS-mediated containment probe starting");
    let kms = match KmsClient::connect() {
        Ok(client) => client,
        Err(error) => {
            println(&alloc::format!(
                "[silo-test] FAIL: KMS unavailable: {:?}",
                error
            ));
            return;
        }
    };
    let silo_denied = direct_silo_denied();
    report("direct live Silo purpose frame denied", silo_denied);

    let registration_denied = matches!(
        kms.register_service_net_instance(),
        Err(KmsClientError::Service(KmsErrorCode::PermissionDenied))
    );
    report("non-service-net registration denied", registration_denied);

    let status_denied = matches!(
        kms.get_relay_p256_status(),
        Err(KmsClientError::Service(
            KmsErrorCode::ServiceBindingRequired
        ))
    );
    report("direct relay status denied", status_denied);

    let sign_denied = matches!(
        kms.sign_tls13_client_certificate_verify(
            &[0x42; 32],
            DEVELOPMENT_RELAY_GENERATION,
            &DEVELOPMENT_PROFILE_DIGEST,
            1,
        ),
        Err(KmsClientError::Service(
            KmsErrorCode::ServiceBindingRequired
        ))
    );
    report(
        "direct TLS signing denied before provider access",
        sign_denied,
    );

    if silo_denied && registration_denied && status_denied && sign_denied {
        println("[silo-test] PASS: no direct Silo or unbound KMS signing path remains");
    } else {
        println("[silo-test] FAIL: containment contract violated");
    }
}
fn direct_silo_denied() -> bool {
    let Some(silo_tid) = sys_lookup_service(api::syscall::service::SILO) else {
        return false;
    };
    let request = DevelopmentSiloRequest::SignTls13ClientCertificateVerify {
        request_seq: 1,
        transcript_hash: [0x42; 32],
        relay_generation: DEVELOPMENT_RELAY_GENERATION,
        active_profile_digest: DEVELOPMENT_PROFILE_DIGEST,
        request_id: 1,
    };
    if !matches!(sys_send(silo_tid, &request.encode()), SyscallResult::Ok(_)) {
        return false;
    }
    let mut response = [0u8; DEVELOPMENT_SILO_FRAME_LEN];
    if !matches!(
        sys_recv_timeout(silo_tid, &mut response, 8),
        SyscallResult::Ok(sender) if sender == silo_tid
    ) {
        return false;
    }
    matches!(
        DevelopmentSiloResponse::decode(&response),
        Some(DevelopmentSiloResponse::Error {
            error: DevelopmentSiloError::Unauthorized,
            ..
        })
    )
}

fn report(name: &str, passed: bool) {
    println(&alloc::format!(
        "[silo-test] {}: {}",
        if passed { "PASS" } else { "FAIL" },
        name
    ));
}
