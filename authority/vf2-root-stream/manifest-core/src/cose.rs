use crate::cbor_read::Reader;
use crate::cbor_write::Writer;
#[cfg(feature = "signing")]
use crate::decode_payload;
use crate::{sha256, Error, Result, EXTERNAL_AAD, MAX_SIG_STRUCTURE_LEN};
use ed25519_compact::{PublicKey, Signature};

const COSE_SIGN1_TAG: u64 = 18;
const PROTECTED_LEN: usize = 38;

/// Computes the required full raw-key key ID.
pub fn key_id(public_key: &[u8; 32]) -> [u8; 32] {
    sha256(public_key)
}

/// Derives an RFC 8032 public key from a nonzero 32-byte seed.
#[cfg(feature = "signing")]
pub fn public_key_from_seed(seed: &[u8; 32]) -> Result<[u8; 32]> {
    use ed25519_compact::{KeyPair, Seed};
    if seed.iter().all(|byte| *byte == 0) {
        return Err(Error::InvalidSeed);
    }
    let pair = KeyPair::from_seed(Seed::from(*seed));
    let mut out = [0; 32];
    out.copy_from_slice(&pair.pk[..]);
    Ok(out)
}

/// Signs an already-canonical manifest payload into the exact tagged COSE_Sign1 profile.
/// `signature_scratch` must hold `MAX_SIG_STRUCTURE_LEN`; the seed must be nonzero.
#[cfg(feature = "signing")]
pub fn sign_cose(
    payload: &[u8],
    seed: &[u8; 32],
    out: &mut [u8],
    signature_scratch: &mut [u8],
) -> Result<usize> {
    use ed25519_compact::{KeyPair, Seed};
    decode_payload(payload)?;
    if signature_scratch.len() < MAX_SIG_STRUCTURE_LEN {
        return Err(Error::ScratchTooSmall);
    }
    let required = 109usize
        .checked_add(bstr_head_len(payload.len()))
        .and_then(|n| n.checked_add(payload.len()))
        .ok_or(Error::Overflow)?;
    if out.len() < required {
        return Err(Error::OutputTooSmall);
    }
    if seed.iter().all(|byte| *byte == 0) {
        return Err(Error::InvalidSeed);
    }
    let pair = KeyPair::from_seed(Seed::from(*seed));
    let mut public_key = [0; 32];
    public_key.copy_from_slice(&pair.pk[..]);
    let mut protected = [0; PROTECTED_LEN];
    encode_protected(&key_id(&public_key), &mut protected)?;
    let message_len = sig_structure(&protected, payload, signature_scratch)?;
    let signature = pair.sk.sign(&signature_scratch[..message_len], None);
    let mut w = Writer::new(out);
    w.tag(COSE_SIGN1_TAG)?;
    w.array(4)?;
    w.bstr(&protected)?;
    w.map(0)?;
    w.bstr(payload)?;
    w.bstr(signature.as_ref())?;
    Ok(w.len())
}

/// Verifies the exact tagged COSE_Sign1 profile and returns its borrowed payload.
/// `signature_scratch` must be at least `MAX_SIG_STRUCTURE_LEN`; no semantic payload
/// parsing is done until the signature succeeds.
pub fn verify_cose<'a>(
    cose: &'a [u8],
    public_key: &[u8; 32],
    signature_scratch: &mut [u8],
) -> Result<&'a [u8]> {
    let mut r = Reader::new(cose);
    if r.tag()? != COSE_SIGN1_TAG || r.array()? != 4 {
        return Err(Error::InvalidCose);
    }
    let protected = r.bstr()?;
    if protected.len() != PROTECTED_LEN {
        return Err(Error::InvalidCose);
    }
    let mut expected = [0; PROTECTED_LEN];
    encode_protected(&key_id(public_key), &mut expected)?;
    if protected != expected {
        return Err(classify_protected(protected, &expected));
    }
    if r.map()? != 0 {
        return Err(Error::InvalidCose);
    }
    let payload = r.bstr()?;
    let signature_bytes = r.bstr()?;
    if signature_bytes.len() != 64 {
        return Err(Error::InvalidCose);
    }
    r.done()?;
    let pk = PublicKey::from_slice(public_key).map_err(|_| Error::InvalidPublicKey)?;
    let signature = Signature::from_slice(signature_bytes).map_err(|_| Error::Signature)?;
    let message_len = sig_structure(protected, payload, signature_scratch)?;
    pk.verify(&signature_scratch[..message_len], &signature)
        .map_err(|_| Error::Signature)?;
    Ok(payload)
}

fn encode_protected(kid: &[u8; 32], out: &mut [u8; PROTECTED_LEN]) -> Result<()> {
    let mut w = Writer::new(out);
    w.map(2)?;
    w.uint(1)?;
    w.raw(&[0x27])?;
    w.uint(4)?;
    w.bstr(kid)?;
    if w.len() != PROTECTED_LEN {
        return Err(Error::InvalidCose);
    }
    Ok(())
}

fn sig_structure(protected: &[u8], payload: &[u8], out: &mut [u8]) -> Result<usize> {
    if payload.len() > crate::MAX_PAYLOAD_LEN {
        return Err(Error::LimitExceeded);
    }
    if out.len() < MAX_SIG_STRUCTURE_LEN {
        return Err(Error::ScratchTooSmall);
    }
    let mut w = Writer::new(out);
    w.array(4)?;
    w.tstr("Signature1")?;
    w.bstr(protected)?;
    w.bstr(EXTERNAL_AAD)?;
    w.bstr(payload)?;
    Ok(w.len())
}

#[cfg(feature = "signing")]
fn bstr_head_len(length: usize) -> usize {
    if length < 24 {
        1
    } else if length <= u8::MAX as usize {
        2
    } else if length <= u16::MAX as usize {
        3
    } else if length <= u32::MAX as usize {
        5
    } else {
        9
    }
}

fn classify_protected(actual: &[u8], expected: &[u8]) -> Error {
    if actual.len() > 3 && actual[0..3] != expected[0..3] {
        Error::WrongAlgorithm
    } else {
        Error::WrongKeyId
    }
}
