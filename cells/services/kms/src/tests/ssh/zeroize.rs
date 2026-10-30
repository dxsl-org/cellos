//! Zeroization on every exit (row 32) and no secret in replies or `Debug`
//! output (row 34).

use std::format;
use std::vec;
use std::vec::Vec;

use ::zeroize::{ZeroizeOnDrop, Zeroizing};
use ed25519_dalek::SigningKey;

use super::fixtures::*;
use super::*;
use crate::ssh::import::ImportSlot;
use crate::ssh::unlock::UnlockSlot;

fn assert_clean<D: Dut>(dut: &mut D, what: &str) {
    let leaf = dut.leaf();
    assert!(leaf.arena.is_zeroed(), "{what}: staging arena");
    assert!(
        leaf.secret.iter().all(|byte| *byte == 0),
        "{what}: unlock secret"
    );
}

#[test]
fn buffers_zero_after_every_exit() {
    let admin = Actor::admin();
    let foreign = Actor::new(101, 5, 11, CALLER_FLAG_KMS_SSH_ADMIN);
    let login = Actor::login();
    let seed = RFC8032_SEED_1;
    let public = public_of(&seed);
    let junk = [0xC3u8; 96];

    // Staging arena: commit success, commit failure, abort, violation,
    // replacement. The blob (which holds a plain seed) never outlives the call.
    let mut service = service();
    import_blob(&mut service, &admin, b"k", &id_ed25519()).unwrap();
    assert_clean(&mut service, "commit success");
    let mut service = self::service();
    import_blob(&mut service, &admin, b"k", &[9u8; 200]).unwrap_err();
    assert_clean(&mut service, "commit failure");
    let begin = begin_import(&mut service, &admin, b"k", 192, sha(b"a")).unwrap();
    import_chunk(&mut service, &admin, begin.handle, 0, &junk).unwrap();
    assert!(!service.ssh.arena.is_zeroed());
    abort_import(&mut service, &admin, begin.handle).unwrap();
    assert_clean(&mut service, "abort");
    let begin = begin_import(&mut service, &admin, b"k", 192, sha(b"a")).unwrap();
    import_chunk(&mut service, &admin, begin.handle, 0, &junk).unwrap();
    import_chunk(&mut service, &admin, begin.handle, 5, &junk).unwrap_err();
    assert_clean(&mut service, "violation");
    let begin = begin_import(&mut service, &admin, b"k", 192, sha(b"a")).unwrap();
    import_chunk(&mut service, &admin, begin.handle, 0, &junk).unwrap();
    begin_import(&mut service, &foreign, b"k", 192, sha(b"a")).unwrap();
    assert_clean(&mut service, "replacement");

    // Unlock secret: sign success, sign failure, abort, violation,
    // replacement, delete.
    let wrapped = Craft::wrapped(&seed, b"pw", &[2; 16], 1).bytes();
    let mut service = self::service();
    import_blob(&mut service, &admin, b"wk", &wrapped).unwrap();
    stage_secret(&mut service, &login, public, b"pw").unwrap();
    assert!(service.ssh.secret.iter().any(|byte| *byte != 0));
    sign(&mut service, &login, public, SESSION_ID, b"u").unwrap();
    assert_clean(&mut service, "sign success");
    stage_secret(&mut service, &login, public, b"bad").unwrap();
    sign(&mut service, &login, public, SESSION_ID, b"u").unwrap_err();
    assert_clean(&mut service, "sign failure");
    let handle = stage_secret(&mut service, &login, public, b"pw").unwrap();
    unlock_abort(&mut service, &login, handle).unwrap();
    assert_clean(&mut service, "unlock abort");
    let begin = unlock_begin(&mut service, &login, public, 200).unwrap();
    unlock_chunk(&mut service, &login, begin.handle, 0, &junk).unwrap();
    unlock_chunk(&mut service, &login, begin.handle, 9, &junk).unwrap_err();
    assert_clean(&mut service, "unlock violation");
    let begin = unlock_begin(&mut service, &login, public, 200).unwrap();
    unlock_chunk(&mut service, &login, begin.handle, 0, &junk).unwrap();
    unlock_begin(&mut service, &login, public, 8).unwrap();
    assert_clean(&mut service, "unlock replacement");
    stage_secret(&mut service, &login, public, b"pw").unwrap();
    delete_key(&mut service, &admin, b"wk", public).unwrap();
    assert_clean(&mut service, "delete");
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
    assert!(matches!(service.ssh.import, ImportSlot::Idle));
}

#[test]
fn secret_holders_zeroize_on_drop_by_type() {
    // Memory cannot be inspected after a drop in a crate that forbids unsafe
    // code, so the drop guarantee is pinned by type: every owner of seed bytes
    // implements `ZeroizeOnDrop`.
    fn zeroize_on_drop<T: ZeroizeOnDrop>() {}
    zeroize_on_drop::<Zeroizing<[u8; 32]>>(); // plain seed at rest and the unwrapped seed
    zeroize_on_drop::<Zeroizing<[u8; 1024]>>(); // staged and copied passphrase
    zeroize_on_drop::<Zeroizing<[u8; 2048]>>(); // decrypted private section
    zeroize_on_drop::<Zeroizing<[u8; 64]>>(); // KDF output and scratch
    zeroize_on_drop::<SigningKey>(); // dalek seed and expanded key
                                     // The retained arena is wiped by `Drop` and by `wipe`; the leaf's staged
                                     // secret is a `Zeroizing` field (checked above).
    let mut arena = crate::ssh::import::Arena::new();
    arena.ensure().unwrap();
    assert!(arena.is_zeroed());
}

/// Records every reply frame.
struct Recorder {
    service: KmsService,
    replies: Vec<[u8; 128]>,
}

impl Dut for Recorder {
    type P = NoPersist;

    fn leaf(&mut self) -> &mut SshLeaf<NoPersist> {
        &mut self.service.ssh
    }

    fn raw(
        &mut self,
        opcode: KmsOpcode,
        payload: &[u8],
        actor: &Actor,
    ) -> Result<Vec<u8>, KmsErrorCode> {
        let frame = KmsRequestV1::new(opcode, 41, payload).unwrap().to_bytes();
        let response = self
            .service
            .handle(&frame, actor.tid, actor.identity(), registry(None, Some(8)))
            .unwrap();
        self.replies.push(response.to_bytes());
        match response.error_code().unwrap() {
            Some(code) => Err(code),
            None => Ok(response.payload().unwrap().to_vec()),
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[test]
fn no_secret_in_replies_debug_or_logs() {
    let seed = [0x5Au8; 32];
    let pass = b"zq-unique-passphrase-9137";
    let salt = [0x33u8; 16];
    let wrapped = Craft::wrapped(&seed, pass, &salt, 1);
    let wrapped_blob = wrapped.bytes();
    let plain_blob = Craft::plain(&RFC8032_SEED_1).bytes();
    let public = public_of(&seed);

    let admin = Actor::admin();
    let login = Actor::login();
    let mut rec = Recorder {
        service: service(),
        replies: Vec::new(),
    };

    // Wrapped flow, with a wrong then a right passphrase.
    import_blob(&mut rec, &admin, b"wk", &wrapped_blob).unwrap();
    status(&mut rec, &login, b"wk").unwrap();
    stage_secret(&mut rec, &login, public, b"wrong-passphrase").unwrap();
    sign(&mut rec, &login, public, SESSION_ID, b"user").unwrap_err();
    stage_secret(&mut rec, &login, public, pass).unwrap();
    sign(&mut rec, &login, public, SESSION_ID, b"user").unwrap();
    let debug_wrapped = format!("{:?}", rec.service.ssh);
    delete_key(&mut rec, &admin, b"wk", public).unwrap();

    // Plain flow.
    import_blob(&mut rec, &admin, b"pk", &plain_blob).unwrap();
    sign(
        &mut rec,
        &login,
        public_of(&RFC8032_SEED_1),
        SESSION_ID,
        b"user",
    )
    .unwrap();
    let debug_plain = format!("{:?}", rec.service.ssh);
    // Failure replies as well.
    import_blob(&mut rec, &admin, b"x", &[1u8; 50]).unwrap_err();

    let ciphertext = wrapped.private.clone();
    let secrets: [(&str, &[u8]); 7] = [
        ("wrapped seed", &seed),
        ("plain seed", &RFC8032_SEED_1),
        ("passphrase", pass),
        ("wrong passphrase", b"wrong-passphrase"),
        ("ciphertext", &ciphertext[..32]),
        ("ciphertext tail", &ciphertext[ciphertext.len() - 32..]),
        ("salt", &salt),
    ];
    assert!(rec.replies.len() > 15);
    for reply in &rec.replies {
        for (label, secret) in secrets {
            assert!(!contains(reply, secret), "reply leaks {label}");
        }
    }
    for debug in [&debug_wrapped, &debug_plain] {
        assert!(debug.contains("<redacted>"));
        for (label, secret) in secrets {
            assert!(!contains(debug.as_bytes(), secret), "debug leaks {label}");
            let hexed = secret
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<std::string::String>();
            assert!(
                !debug.to_lowercase().contains(&hexed),
                "debug leaks {label} as hex"
            );
            let decimal = format!("{:?}", secret);
            assert!(!debug.contains(&decimal), "debug leaks {label} as a list");
        }
    }
    // The stored identity prints no key material either.
    let identity = rec.service.ssh.identity.as_ref().map(|i| format!("{i:?}"));
    if let Some(text) = identity {
        assert!(text.contains("<redacted>"));
    }
    let _ = vec![0u8; 0];
}
