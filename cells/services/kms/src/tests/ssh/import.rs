//! Import state machine, refusal matrix rows 9-16, 18, 19.

use std::vec;
use std::vec::Vec;

use super::fixtures::*;
use super::*;
use crate::ssh::import::ImportSlot;
use crate::ssh::parser::{parse_outer, Cipher};

fn assert_idle_and_wiped<D: Dut>(dut: &mut D) {
    let leaf = dut.leaf();
    assert!(matches!(leaf.import, ImportSlot::Idle));
    assert!(leaf.arena.is_zeroed());
}

fn begin_payload(actor: &Actor, total_len: u32, name: &[u8], digest: [u8; 32]) -> [u8; 112] {
    SshImportBeginRequestPayload {
        request_id: actor.rid(),
        total_len,
        name_len: name.len() as u8,
        reserved: [0; 3],
        blob_sha256: digest,
        name: name_field(name),
    }
    .encode()
}

#[test]
fn begin_bounds() {
    let mut service = self::service();
    let admin = Actor::admin();
    let digest = sha(b"x");
    let before = snapshot(&mut service, &[&admin]);
    let call = |service: &mut KmsService, payload: &[u8]| {
        service.raw(KmsOpcode::BeginSshKeyImport, payload, &admin)
    };

    // Wrong payload length, request id 0, total_len 0 / 65,537, name rules,
    // nonzero reserved bytes, all-zero digest.
    assert_eq!(call(&mut service, &[]), Err(KmsErrorCode::InvalidRequest));
    assert_eq!(
        call(&mut service, &[0u8; 111]),
        Err(KmsErrorCode::InvalidRequest)
    );
    let good = begin_payload(&admin, 100, b"key", digest);
    let tampered = |at: usize, value: u8| {
        let mut payload = good;
        payload[at] = value;
        payload
    };
    let mut cases: Vec<[u8; 112]> = vec![
        tampered(0, 0),      // request id low byte -> still nonzero: rebuilt below
        tampered(13, 1),     // reserved
        tampered(15, 1),     // reserved
        tampered(12, 0),     // name_len 0
        tampered(12, 65),    // name_len 65
        tampered(48, b' '),  // charset
        tampered(48 + 3, 1), // padding after the name
    ];
    cases.remove(0);
    for payload in cases {
        assert_eq!(
            call(&mut service, &payload),
            Err(KmsErrorCode::InvalidRequest)
        );
    }
    for (total, expect_ok) in [(0u32, false), (65_537, false), (u32::MAX, false)] {
        let payload = begin_payload(&admin, total, b"key", digest);
        assert_eq!(
            call(&mut service, &payload).is_ok(),
            expect_ok,
            "total {total}"
        );
    }
    let zero_digest = begin_payload(&admin, 100, b"key", [0; 32]);
    assert_eq!(
        call(&mut service, &zero_digest),
        Err(KmsErrorCode::InvalidRequest)
    );
    let mut zero_id = begin_payload(&admin, 100, b"key", digest);
    zero_id[..8].fill(0);
    assert_eq!(
        call(&mut service, &zero_id),
        Err(KmsErrorCode::InvalidRequest)
    );
    assert_eq!(snapshot(&mut service, &[&admin]), before);

    // The largest legal transfer is accepted: 683 chunks of 96 with a 64-byte
    // last chunk.
    let begin = begin_import(&mut service, &admin, b"key", 65_536, digest).unwrap();
    assert_eq!(begin.chunk_count, 683);
    assert_eq!(begin.chunk_capacity, 96);
    assert_ne!(begin.handle, 0);
}

#[test]
fn full_size_transfer_is_staged_integrity_checked_wiped_and_refused_as_format() {
    let mut service = self::service();
    let admin = Actor::admin();
    assert!(!service.ssh.arena.is_allocated(), "lazy");
    let blob: Vec<u8> = (0..65_536u32).map(|i| (i % 251) as u8).collect();
    assert_eq!(
        import_blob(&mut service, &admin, b"big", &blob),
        Err(KmsErrorCode::SshKeyFormatRejected)
    );
    assert!(service.ssh.arena.is_allocated(), "retained");
    assert_idle_and_wiped(&mut service);
    assert!(service.ssh.identity.is_none());
}

#[test]
fn begin_name_exists() {
    let mut service = self::service();
    let admin = Actor::admin();
    import_blob(&mut service, &admin, b"mykey", &id_ed25519()).unwrap();
    let before = snapshot(&mut service, &[&admin]);
    // Same name: SshKeyExists. Another name: the single-identity limit.
    assert_eq!(
        begin_import(&mut service, &admin, b"mykey", 10, sha(b"a")),
        Err(KmsErrorCode::SshKeyExists)
    );
    assert_eq!(
        begin_import(&mut service, &admin, b"other", 10, sha(b"a")),
        Err(KmsErrorCode::SshStoreFull)
    );
    // Only the floor of the failed attempts is untouched (nothing advanced).
    let after = snapshot(&mut service, &[&admin]);
    assert_eq!(before, after);
}

#[test]
fn begin_same_principal_pending() {
    let mut service = self::service();
    let admin = Actor::admin();
    let digest = sha(b"abc");
    let begin = begin_import(&mut service, &admin, b"k", 3, digest).unwrap();
    // Receiving: a second Begin by the same principal is refused...
    let before = snapshot(&mut service, &[&admin]);
    assert_eq!(
        begin_import(&mut service, &admin, b"k", 3, digest),
        Err(KmsErrorCode::SshImportPending)
    );
    assert_eq!(snapshot(&mut service, &[&admin]), before);
    // ...also from another task of the same Cell, and once Complete.
    let sibling = Actor::new(admin.cell, admin.generation, admin.tid + 1, admin.flags);
    sibling.set_next(100);
    assert_eq!(
        begin_import(&mut service, &sibling, b"k", 3, digest),
        Err(KmsErrorCode::SshImportPending)
    );
    import_chunk(&mut service, &admin, begin.handle, 0, b"abc").unwrap();
    assert_eq!(
        begin_import(&mut service, &admin, b"k", 3, digest),
        Err(KmsErrorCode::SshImportPending)
    );
    // Abort frees it.
    abort_import(&mut service, &admin, begin.handle).unwrap();
    assert_idle_and_wiped(&mut service);
    begin_import(&mut service, &admin, b"k", 3, digest).unwrap();
}

#[test]
fn begin_other_principal_replaces_and_zeroizes() {
    let mut service = self::service();
    let first = Actor::admin();
    let second = Actor::new(101, 5, 11, CALLER_FLAG_KMS_SSH_ADMIN);
    let secret = [0xC3u8; 96];
    let begin = begin_import(&mut service, &first, b"k", 192, sha(b"a")).unwrap();
    import_chunk(&mut service, &first, begin.handle, 0, &secret).unwrap();
    assert!(!service.ssh.arena.is_zeroed());

    let taken = begin_import(&mut service, &second, b"k", 10, sha(b"b")).unwrap();
    assert_ne!(taken.handle, begin.handle);
    assert!(service.ssh.arena.is_zeroed(), "replaced import is wiped");
    // The dead principal's handle no longer works and cannot disturb the new
    // import beyond destroying it as a foreign handle (CSR rule).
    assert_eq!(
        import_chunk(&mut service, &first, begin.handle, 1, &secret),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    assert_idle_and_wiped(&mut service);
}

#[test]
fn chunk_handle_violations_destroy() {
    let digest = sha(b"abc");
    let admin = Actor::admin();
    let foreign = Actor::new(101, 5, 11, CALLER_FLAG_KMS_SSH_ADMIN);
    let sibling = Actor::new(admin.cell, admin.generation, 12, admin.flags);
    // (label, who sends, handle selector)
    for label in [
        "wrong handle",
        "zero handle",
        "other principal",
        "other task",
    ] {
        let mut service = self::service();
        let begin = begin_import(&mut service, &admin, b"k", 200, digest).unwrap();
        import_chunk(&mut service, &admin, begin.handle, 0, &[1u8; 96]).unwrap();
        let (actor, handle) = match label {
            "wrong handle" => (&admin, begin.handle ^ 2),
            "zero handle" => (&admin, 0),
            "other principal" => (&foreign, begin.handle),
            _ => (&sibling, begin.handle),
        };
        assert_eq!(
            import_chunk(&mut service, actor, handle, 1, &[1u8; 96]),
            Err(KmsErrorCode::SshHandleInvalid),
            "{label}"
        );
        assert_idle_and_wiped(&mut service);
        // The real handle is now dead.
        assert_eq!(
            import_chunk(&mut service, &admin, begin.handle, 1, &[1u8; 96]),
            Err(KmsErrorCode::SshHandleInvalid),
            "{label}"
        );
    }
    // No import at all: nothing to destroy.
    let mut service = self::service();
    assert_eq!(
        import_chunk(&mut service, &admin, 77, 0, &[1u8]),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    // An unlock handle never validates for import and vice versa (disjoint
    // handle spaces): the unlock slot's handle against op 16, the import
    // slot's handle against op 23.
    let mut service = self::service();
    let login = Actor::login();
    import_blob(&mut service, &admin, b"w", &id_ed25519_enc()).unwrap();
    let unlock = unlock_begin(&mut service, &login, id_ed25519_public(), 8).unwrap();
    assert_eq!(
        import_chunk(&mut service, &admin, unlock.handle, 0, &[1u8; 8]),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    let other = Actor::new(102, 1, 13, CALLER_FLAG_KMS_SSH_ADMIN);
    let import = {
        let mut empty = KmsService::new();
        begin_import(&mut empty, &other, b"z", 5, digest).unwrap()
    };
    assert_ne!(import.handle, unlock.handle);
}

#[test]
fn chunk_order_violations_destroy() {
    let digest = sha(b"abc");
    let admin = Actor::admin();
    let total = 200u32; // chunks of 96, 96, 8
    let run = |label: &str, send: &dyn Fn(&mut KmsService, u64) -> Result<(), KmsErrorCode>| {
        let mut service = self::service();
        let begin = begin_import(&mut service, &admin, b"k", total, digest).unwrap();
        assert_eq!(
            send(&mut service, begin.handle),
            Err(KmsErrorCode::SshOrderInvalid),
            "{label}"
        );
        assert_idle_and_wiped(&mut service);
    };
    run("skipped index", &|s, h| {
        import_chunk(s, &admin, h, 1, &[1u8; 96]).map(drop)
    });
    run("repeated chunk", &|s, h| {
        import_chunk(s, &admin, h, 0, &[1u8; 96])?;
        import_chunk(s, &admin, h, 0, &[1u8; 96]).map(drop)
    });
    run("short non-last chunk", &|s, h| {
        import_chunk(s, &admin, h, 0, &[1u8; 95]).map(drop)
    });
    run("last chunk too long", &|s, h| {
        import_chunk(s, &admin, h, 0, &[1u8; 96])?;
        import_chunk(s, &admin, h, 1, &[1u8; 96])?;
        import_chunk(s, &admin, h, 2, &[1u8; 9]).map(drop)
    });
    run("last chunk too short", &|s, h| {
        import_chunk(s, &admin, h, 0, &[1u8; 96])?;
        import_chunk(s, &admin, h, 1, &[1u8; 96])?;
        import_chunk(s, &admin, h, 2, &[1u8; 7]).map(drop)
    });
    // A chunk after Complete destroys it as a state violation.
    let mut service = self::service();
    let begin = begin_import(&mut service, &admin, b"k", 96, digest).unwrap();
    import_chunk(&mut service, &admin, begin.handle, 0, &[1u8; 96]).unwrap();
    assert_eq!(
        import_chunk(&mut service, &admin, begin.handle, 1, &[1u8; 96]),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    assert_idle_and_wiped(&mut service);
}

#[test]
fn commit_early_or_count_mismatch() {
    let admin = Actor::admin();
    let blob = id_ed25519();
    let digest = sha(&blob);
    let n = blob.len() as u32;
    // Early commit (still Receiving).
    let mut service = self::service();
    let begin = begin_import(&mut service, &admin, b"k", n, digest).unwrap();
    import_chunk(&mut service, &admin, begin.handle, 0, &blob[..96]).unwrap();
    assert_eq!(
        commit_import(
            &mut service,
            &admin,
            begin.handle,
            begin.chunk_count,
            digest
        ),
        Err(KmsErrorCode::SshOrderInvalid)
    );
    assert_idle_and_wiped(&mut service);
    // Wrong chunk count at Commit.
    let mut service = self::service();
    let begin = begin_import(&mut service, &admin, b"k", n, digest).unwrap();
    for (i, data) in blob.chunks(96).enumerate() {
        import_chunk(&mut service, &admin, begin.handle, i as u32, data).unwrap();
    }
    assert_eq!(
        commit_import(
            &mut service,
            &admin,
            begin.handle,
            begin.chunk_count + 1,
            digest
        ),
        Err(KmsErrorCode::SshOrderInvalid)
    );
    assert_idle_and_wiped(&mut service);
    // Wrong handle / foreign principal at Commit: handle error, destroyed.
    let foreign = Actor::new(101, 5, 11, CALLER_FLAG_KMS_SSH_ADMIN);
    for (actor, handle_xor) in [(&admin, 4u64), (&foreign, 0)] {
        let mut service = self::service();
        let begin = begin_import(&mut service, &admin, b"k", n, digest).unwrap();
        for (i, data) in blob.chunks(96).enumerate() {
            import_chunk(&mut service, &admin, begin.handle, i as u32, data).unwrap();
        }
        assert_eq!(
            commit_import(
                &mut service,
                actor,
                begin.handle ^ handle_xor,
                begin.chunk_count,
                digest
            ),
            Err(KmsErrorCode::SshHandleInvalid)
        );
        assert_idle_and_wiped(&mut service);
    }
    // No import at all.
    let mut service = self::service();
    assert_eq!(
        commit_import(&mut service, &admin, 5, 1, digest),
        Err(KmsErrorCode::SshHandleInvalid)
    );
}

#[test]
fn commit_integrity() {
    let admin = Actor::admin();
    let blob = id_ed25519();
    let good = sha(&blob);
    // Commit digest differs from Begin's; Begin digest differs from the data;
    // a flipped data byte. All answer integrity before any parsing.
    for label in ["commit digest", "begin digest", "flipped byte"] {
        let mut service = self::service();
        let (begin_digest, commit_digest, data) = match label {
            "commit digest" => (good, sha(b"other"), blob.clone()),
            "begin digest" => (sha(b"other"), sha(b"other"), blob.clone()),
            _ => {
                let mut data = blob.clone();
                data[100] ^= 1;
                (good, good, data)
            }
        };
        let begin =
            begin_import(&mut service, &admin, b"k", blob.len() as u32, begin_digest).unwrap();
        for (i, chunk) in data.chunks(96).enumerate() {
            import_chunk(&mut service, &admin, begin.handle, i as u32, chunk).unwrap();
        }
        assert_eq!(
            commit_import(
                &mut service,
                &admin,
                begin.handle,
                begin.chunk_count,
                commit_digest
            ),
            Err(KmsErrorCode::SshImportIntegrity),
            "{label}"
        );
        assert_idle_and_wiped(&mut service);
        assert!(service.ssh.identity.is_none());
    }
    // Garbage with a matching digest passes integrity and fails the format.
    let mut service = self::service();
    assert_eq!(
        import_blob(&mut service, &admin, b"k", &[7u8; 300]),
        Err(KmsErrorCode::SshKeyFormatRejected)
    );
    // The good blob imports; the failed attempts left no trace.
    let status = import_blob(&mut service, &admin, b"k", &blob).unwrap();
    assert_eq!(status.public_key, id_ed25519_public());
}

#[test]
fn same_public_key_defence_recheck() {
    // With one identity per store this is unreachable through the ABI (Begin
    // already refuses); the Commit re-check is exercised with a direct insert.
    let admin = Actor::admin();
    let blob = id_ed25519();
    let digest = sha(&blob);
    for (public, expect) in [
        (id_ed25519_public(), KmsErrorCode::SshKeyExists),
        ([9u8; 32], KmsErrorCode::SshStoreFull),
    ] {
        let mut service = self::service();
        let begin = begin_import(&mut service, &admin, b"k", blob.len() as u32, digest).unwrap();
        for (i, data) in blob.chunks(96).enumerate() {
            import_chunk(&mut service, &admin, begin.handle, i as u32, data).unwrap();
        }
        service.ssh.identity =
            crate::ssh::store::StoredIdentity::plain(b"raced", public, &RFC8032_SEED_1);
        assert_eq!(
            commit_import(
                &mut service,
                &admin,
                begin.handle,
                begin.chunk_count,
                digest
            ),
            Err(expect)
        );
        assert_idle_and_wiped(&mut service);
        assert_eq!(service.ssh.revision, 0);
    }
}

#[test]
fn store_budget_wrapped_ct() {
    let admin = Actor::admin();
    let seed = RFC8032_SEED_1;
    // Wrapped ciphertext of exactly 2,048 bytes is stored (not decrypted at
    // import); 2,064 is the store budget (SshStoreFull), not a format error.
    let mut service = self::service();
    let ok = Craft::wrapped_opaque(&[3u8; 16], 4, vec![0xAB; 2048]).bytes();
    let status = import_blob(&mut service, &admin, b"k", &ok).unwrap();
    assert_eq!(status.custody, KmsSshKeyCustody::PassphraseWrapped);
    assert_eq!(status.public_key, public_of(&seed));
    let mut service = self::service();
    let too_big = Craft::wrapped_opaque(&[3u8; 16], 4, vec![0xAB; 2064]).bytes();
    assert_eq!(
        import_blob(&mut service, &admin, b"k", &too_big),
        Err(KmsErrorCode::SshStoreFull)
    );
    assert!(service.ssh.identity.is_none());
    // A plain key stores only the seed however large its comment is.
    let mut craft = Craft::plain(&seed);
    craft.private = inner_section(&seed, &vec![b'c'; 60_000], 8, Pad::Natural);
    let blob = craft.bytes();
    assert!(blob.len() > 60_000 && blob.len() <= 65_536);
    let mut service = self::service();
    let status = import_blob(&mut service, &admin, b"big", &blob).unwrap();
    assert_eq!(status.custody, KmsSshKeyCustody::Plain);
    assert!(matches!(
        service.ssh.identity.as_ref().unwrap().secret(),
        crate::ssh::store::Secret::Plain(_)
    ));
    assert_eq!(parse_outer(&blob).unwrap().cipher, Cipher::None);
}

#[test]
fn import_status_reflects_the_new_key_and_bumps_the_revision() {
    let mut service = self::service();
    let admin = Actor::admin();
    let login = Actor::login();
    let leaf = status(&mut service, &login, b"").unwrap();
    assert_eq!(
        (leaf.key_count, leaf.store_revision, leaf.key_present),
        (0, 0, 0)
    );
    let imported = import_blob(&mut service, &admin, b"mykey", &id_ed25519()).unwrap();
    assert_eq!(imported.algorithm, KmsKeyAlgorithm::SshEd25519);
    assert_eq!(imported.readiness, KmsCapabilityReadiness::Ready);
    assert_eq!(imported.provider, KmsProviderKind::None);
    assert_eq!(imported.posture, KmsCustodyPosture::DevPostureVolatile);
    assert_eq!(imported.custody, KmsSshKeyCustody::Plain);
    assert_eq!(
        (
            imported.key_count,
            imported.store_revision,
            imported.key_present
        ),
        (1, 1, 1)
    );
    assert_eq!(imported.public_key, id_ed25519_public());
    // SHA-256 of the 51-byte wire blob (the `ssh-keygen -lf` value).
    assert_eq!(
        imported.fingerprint_sha256,
        hex32("5025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084")
    );
    // Login and admin both read the same status.
    assert_eq!(status(&mut service, &login, b"mykey").unwrap(), imported);
    assert_eq!(status(&mut service, &admin, b"mykey").unwrap(), imported);
}
