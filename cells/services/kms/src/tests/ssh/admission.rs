//! Caller admission (refusal matrix rows 1-7): attested principal, Login and
//! Admin classes, fail-closed admin, leaf and relay readiness independence,
//! canonical envelopes, strict payload decoding.

use std::vec::Vec;

use api::caller_identity::{
    CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_NET_CONNECT, CALLER_FLAG_NET_L2, CALLER_FLAG_NET_LISTEN,
    CALLER_FLAG_NET_RESOLVE, CALLER_FLAG_VFS_MUTATE,
};
use types::kms::{KmsCapabilityReadiness, RelayP256StatusPayload};

use super::fixtures::*;
use super::store::FakePersist;
use super::*;
use crate::storage::FixtureRelayProvider;

const ADMIN_OPS: [KmsOpcode; 5] = [
    KmsOpcode::BeginSshKeyImport,
    KmsOpcode::ImportSshKeyChunk,
    KmsOpcode::CommitSshKeyImport,
    KmsOpcode::AbortSshKeyImport,
    KmsOpcode::DeleteSshKey,
];
const LOGIN_OPS: [KmsOpcode; 4] = [
    KmsOpcode::SignSshUserauth,
    KmsOpcode::BeginSshUnlock,
    KmsOpcode::SshUnlockChunk,
    KmsOpcode::AbortSshUnlock,
];
const ALL_OPS: [KmsOpcode; 10] = [
    KmsOpcode::BeginSshKeyImport,
    KmsOpcode::ImportSshKeyChunk,
    KmsOpcode::CommitSshKeyImport,
    KmsOpcode::AbortSshKeyImport,
    KmsOpcode::GetSshKeyStatus,
    KmsOpcode::DeleteSshKey,
    KmsOpcode::SignSshUserauth,
    KmsOpcode::BeginSshUnlock,
    KmsOpcode::SshUnlockChunk,
    KmsOpcode::AbortSshUnlock,
];
const GARBAGE: [u8; 112] = [0xA5; 112];

/// A service with a stored wrapped key and an open import slot of another
/// principal so every refusal below has state to leave alone.
fn loaded_service() -> KmsService {
    let mut service = service();
    let seed_owner = Actor::new(300, 1, 30, CALLER_FLAG_KMS_SSH_ADMIN);
    let blob = Craft::wrapped(&RFC8032_SEED_1, b"pw", &[1; 16], 1).bytes();
    import_blob(&mut service, &seed_owner, b"wk", &blob).unwrap();
    service
}

fn bystanders() -> (Actor, Actor) {
    (Actor::admin(), Actor::login())
}

fn call_with_garbage(
    service: &mut KmsService,
    opcode: KmsOpcode,
    actor: &Actor,
) -> Result<Vec<u8>, KmsErrorCode> {
    service.raw(opcode, &GARBAGE, actor)
}

#[test]
fn unattested_callers_are_refused_on_every_opcode() {
    let both = CALLER_FLAG_KMS_SSH_ADMIN | CALLER_FLAG_KMS_SSH_SIGN;
    for opcode in ALL_OPS {
        for (label, caller) in [
            ("missing trailer", None),
            (
                "generation 0",
                Some(CallerIdentity {
                    flags: both,
                    cell_id: 20,
                    generation: 0,
                    sender_tid: 7,
                }),
            ),
            (
                "sender mismatch",
                Some(CallerIdentity {
                    flags: both,
                    cell_id: 20,
                    generation: 3,
                    sender_tid: 70,
                }),
            ),
            (
                "cell 0",
                Some(CallerIdentity {
                    flags: both,
                    cell_id: 0,
                    generation: 3,
                    sender_tid: 7,
                }),
            ),
        ] {
            let mut service = loaded_service();
            let (admin, login) = bystanders();
            let before = snapshot(&mut service, &[&admin, &login]);
            let frame = KmsRequestV1::new(opcode, 41, &GARBAGE).unwrap().to_bytes();
            let response = service
                .handle(&frame, 7, caller, registry(None, Some(8)))
                .unwrap();
            assert_eq!(
                response.error_code().unwrap(),
                Some(KmsErrorCode::CallerUnattested),
                "{opcode:?} {label}"
            );
            assert!(response.payload().unwrap().is_empty());
            assert_eq!(snapshot(&mut service, &[&admin, &login]), before);
        }
    }
}

#[test]
fn non_login_refused_on_21_24() {
    let flag_sets = [
        0u32,
        CALLER_FLAG_KMS_SSH_ADMIN,
        CALLER_FLAG_VFS_MUTATE,
        CALLER_FLAG_CONFIG_WRITE,
        CALLER_FLAG_NET_RESOLVE
            | CALLER_FLAG_NET_CONNECT
            | CALLER_FLAG_NET_LISTEN
            | CALLER_FLAG_NET_L2,
    ];
    for flags in flag_sets {
        for opcode in LOGIN_OPS {
            let mut service = loaded_service();
            let actor = Actor::new(55, 2, 15, flags);
            let (admin, login) = bystanders();
            let before = snapshot(&mut service, &[&admin, &login, &actor]);
            assert_eq!(
                call_with_garbage(&mut service, opcode, &actor),
                Err(KmsErrorCode::PermissionDenied),
                "{opcode:?} flags {flags:#x}"
            );
            assert_eq!(snapshot(&mut service, &[&admin, &login, &actor]), before);
        }
    }
}

#[test]
fn login_refused_on_admin_ops() {
    for opcode in ADMIN_OPS {
        let mut service = loaded_service();
        let login = Actor::login();
        let before = snapshot(&mut service, &[&login]);
        assert_eq!(
            call_with_garbage(&mut service, opcode, &login),
            Err(KmsErrorCode::PermissionDenied),
            "{opcode:?}"
        );
        assert_eq!(snapshot(&mut service, &[&login]), before);
    }
}

#[test]
fn admin_refused_on_sign_ops() {
    for opcode in LOGIN_OPS {
        let mut service = loaded_service();
        let admin = Actor::admin();
        let before = snapshot(&mut service, &[&admin]);
        assert_eq!(
            call_with_garbage(&mut service, opcode, &admin),
            Err(KmsErrorCode::PermissionDenied),
            "{opcode:?}"
        );
        assert_eq!(snapshot(&mut service, &[&admin]), before);
    }
}

#[test]
fn status_is_open_to_admin_and_login_only() {
    let mut service = loaded_service();
    let status_payload = SshStatusRequestPayload {
        name_len: 0,
        name: [0; 64],
    }
    .encode();
    for actor in [Actor::admin(), Actor::login()] {
        assert!(service
            .raw(KmsOpcode::GetSshKeyStatus, &status_payload, &actor)
            .is_ok());
    }
    for flags in [
        0u32,
        CALLER_FLAG_KMS_SSH_ADMIN | CALLER_FLAG_KMS_SSH_SIGN,
        CALLER_FLAG_VFS_MUTATE,
    ] {
        let actor = Actor::new(77, 1, 17, flags);
        assert_eq!(
            service.raw(KmsOpcode::GetSshKeyStatus, &status_payload, &actor),
            Err(KmsErrorCode::PermissionDenied)
        );
    }
}

#[test]
fn supervisor_is_not_an_admin_caller_and_both_flags_are_refused_everywhere() {
    // D-5.3: admin needs the K3 admin flag; being the registered supervisor
    // grants nothing. No kernel path grants the flag until K7.
    let supervisor_tid = 8usize;
    let supervisor = Actor::new(1, 1, supervisor_tid, 0);
    let both = Actor::new(
        2,
        2,
        9,
        CALLER_FLAG_KMS_SSH_ADMIN | CALLER_FLAG_KMS_SSH_SIGN,
    );
    for opcode in ALL_OPS {
        let mut service = loaded_service();
        let frame = KmsRequestV1::new(opcode, 41, &GARBAGE).unwrap().to_bytes();
        for (actor, label) in [(&supervisor, "supervisor"), (&both, "both flags")] {
            let response = service
                .handle(
                    &frame,
                    actor.tid,
                    actor.identity(),
                    registry(Some(8), Some(supervisor_tid)),
                )
                .unwrap();
            assert_eq!(
                response.error_code().unwrap(),
                Some(KmsErrorCode::PermissionDenied),
                "{opcode:?} {label}"
            );
        }
    }
    // Control: the same frame from a flagged admin is decoded (and refused for
    // its garbage payload), proving the refusal above was the class check.
    let mut service = loaded_service();
    assert_eq!(
        call_with_garbage(&mut service, KmsOpcode::BeginSshKeyImport, &Actor::admin()),
        Err(KmsErrorCode::InvalidRequest)
    );
}

#[test]
fn no_persister_call_before_admission_passes() {
    let mut leaf = SshLeaf::new(FakePersist::default());
    for opcode in ALL_OPS {
        for actor in [Actor::new(55, 2, 15, 0), Actor::login(), Actor::admin()] {
            // Wrong class for at least one of the three; none may touch the
            // persister before its class check passes.
            let allowed = matches!(
                (opcode, actor.flags),
                (o, f) if (ADMIN_OPS.contains(&o) && f == CALLER_FLAG_KMS_SSH_ADMIN)
                    || (LOGIN_OPS.contains(&o) && f == CALLER_FLAG_KMS_SSH_SIGN)
                    || (o == KmsOpcode::GetSshKeyStatus && f != 0)
            );
            if allowed {
                continue;
            }
            let _ = leaf.raw(opcode, &GARBAGE, &actor);
            assert_eq!(
                leaf.persist.calls,
                Vec::<&str>::new(),
                "{opcode:?} {:#x}",
                actor.flags
            );
        }
    }
    // Unattested too.
    let frame = leaf.handle(KmsOpcode::SignSshUserauth, &GARBAGE, 7, None);
    assert_eq!(frame.err(), Some(KmsErrorCode::CallerUnattested));
    assert!(leaf.persist.calls.is_empty());
}

#[test]
fn leaf_and_relay_readiness_independent() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    let net = crate::ServiceRegistrySnapshot {
        net_broker_tid: None,
        supervisor_tid: Some(8),
        net_tid: Some(7),
    };
    let net_caller = Some(CallerIdentity {
        flags: 0,
        cell_id: 50,
        generation: 4,
        sender_tid: 7,
    });
    let send = |service: &mut KmsService, opcode: KmsOpcode, payload: &[u8]| {
        let frame = KmsRequestV1::new(opcode, 41, payload).unwrap().to_bytes();
        service.handle(&frame, 7, net_caller, net).unwrap()
    };
    assert_eq!(
        send(&mut service, KmsOpcode::RegisterServiceNetInstance, &[])
            .error_code()
            .unwrap(),
        None
    );
    let relay_status = |service: &mut KmsService| {
        let response = send(service, KmsOpcode::GetRelayP256Status, &[]);
        RelayP256StatusPayload::decode(response.payload().unwrap()).unwrap()
    };
    let before = relay_status(&mut service);
    assert_eq!(before.readiness, KmsCapabilityReadiness::Ready);

    // Leaf unavailable while the relay is Ready: every opcode that needs a
    // serving leaf is SshKeyUnavailable; the relay is undisturbed.
    service.ssh.state = crate::ssh::LeafState::Unavailable;
    let admin = Actor::admin();
    let login = Actor::login();
    let begin = SshImportBeginRequestPayload {
        request_id: 1,
        total_len: 10,
        name_len: 1,
        reserved: [0; 3],
        blob_sha256: [1; 32],
        name: name_field(b"k"),
    }
    .encode();
    assert_eq!(
        service.raw(KmsOpcode::BeginSshKeyImport, &begin, &admin),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(
        service.raw(
            KmsOpcode::SignSshUserauth,
            &sign_request(&login, [1; 32], SESSION_ID, b"u").encode(),
            &login
        ),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    // The leaf query alone reports the true (not ready) state.
    let leaf = status(&mut service, &login, b"").unwrap();
    assert_eq!(leaf.readiness, KmsCapabilityReadiness::Unavailable);
    assert_eq!(relay_status(&mut service), before);

    // Leaf serving while the relay is unavailable: SSH ops proceed, relay ops
    // keep their own answer.
    let mut service = self::service();
    let leaf = status(&mut service, &login, b"").unwrap();
    assert_eq!(leaf.readiness, KmsCapabilityReadiness::Ready);
    assert_eq!(leaf.provider, KmsProviderKind::None);
    let response = send(&mut service, KmsOpcode::GetRelayP256Status, &[]);
    assert_ne!(
        response.error_code().unwrap(),
        None,
        "relay ops are unchanged"
    );
    import_blob(&mut service, &admin, b"k", &id_ed25519()).unwrap();
}

#[test]
fn noncanonical_envelopes_dropped() {
    let mut service = loaded_service();
    let login = Actor::login();
    let valid = KmsRequestV1::new(KmsOpcode::GetSshKeyStatus, 41, &[0u8; 65])
        .unwrap()
        .to_bytes();
    let attested = login.identity();
    let reg = registry(None, Some(8));
    assert!(service.handle(&valid, login.tid, attested, reg).is_some());
    // Opcode 25, nonzero flags/reserved/tail, version 2: no reply at all.
    for (at, value) in [(1usize, 25u8), (2, 1), (10, 1), (15, 1), (127, 1), (0, 2)] {
        let mut frame = valid;
        frame[at] = value;
        assert!(
            service.handle(&frame, login.tid, attested, reg).is_none(),
            "byte {at}"
        );
    }
}

#[test]
fn malformed_payloads_invalid_request() {
    let lens: [(KmsOpcode, usize, Actor); 10] = [
        (KmsOpcode::BeginSshKeyImport, 112, Actor::admin()),
        (KmsOpcode::ImportSshKeyChunk, 112, Actor::admin()),
        (KmsOpcode::CommitSshKeyImport, 48, Actor::admin()),
        (KmsOpcode::AbortSshKeyImport, 8, Actor::admin()),
        (KmsOpcode::GetSshKeyStatus, 65, Actor::login()),
        (KmsOpcode::DeleteSshKey, 105, Actor::admin()),
        (KmsOpcode::SignSshUserauth, 104, Actor::login()),
        (KmsOpcode::BeginSshUnlock, 48, Actor::login()),
        (KmsOpcode::SshUnlockChunk, 112, Actor::login()),
        (KmsOpcode::AbortSshUnlock, 8, Actor::login()),
    ];
    for (opcode, len, actor) in lens {
        let mut service = loaded_service();
        let before = snapshot(&mut service, &[&actor]);
        let mut bad_lengths = std::vec![0usize, len - 1];
        if len < 112 {
            bad_lengths.push(len + 1);
        }
        for bad in bad_lengths {
            assert_eq!(
                service.raw(opcode, &std::vec![0u8; bad], &actor),
                Err(KmsErrorCode::InvalidRequest),
                "{opcode:?} len {bad}"
            );
        }
        // Right length, wrong content: nonzero reserved bytes / padding where
        // the codec demands zero (ops with such fields).
        assert_eq!(snapshot(&mut service, &[&actor]), before, "{opcode:?}");
    }
    // Nonzero padding after a variable-length field: name, username, chunk.
    let mut service = loaded_service();
    let admin = Actor::admin();
    let login = Actor::login();
    let mut status_payload = SshStatusRequestPayload {
        name_len: 2,
        name: name_field(b"wk"),
    }
    .encode();
    status_payload[10] = 1;
    assert_eq!(
        service.raw(KmsOpcode::GetSshKeyStatus, &status_payload, &login),
        Err(KmsErrorCode::InvalidRequest)
    );
    let mut delete = SshDeleteRequestPayload {
        request_id: 1,
        public_key: public_of(&RFC8032_SEED_1),
        name_len: 2,
        name: name_field(b"wk"),
    }
    .encode();
    delete[41 + 5] = 1;
    assert_eq!(
        service.raw(KmsOpcode::DeleteSshKey, &delete, &admin),
        Err(KmsErrorCode::InvalidRequest)
    );
    let mut chunk = chunk_payload(1, 0, &[1, 2, 3]);
    chunk[16 + 10] = 1;
    assert_eq!(
        service.raw(KmsOpcode::ImportSshKeyChunk, &chunk, &admin),
        Err(KmsErrorCode::InvalidRequest)
    );
    // Invalid names and chunk bounds.
    let mut bad_name = SshStatusRequestPayload {
        name_len: 2,
        name: name_field(b"wk"),
    }
    .encode();
    bad_name[1] = b'/';
    assert_eq!(
        service.raw(KmsOpcode::GetSshKeyStatus, &bad_name, &login),
        Err(KmsErrorCode::InvalidRequest)
    );
}
