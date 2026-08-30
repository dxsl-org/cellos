use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use sha2::{Digest, Sha256};
use std::{vec, vec::Vec};

const SIG_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const EC_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const CURVE_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const CLIENT_OID: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02];
const NODE_OID: &[u8] = &[0x2b, 0x06, 0x01, 0x04, 0x01, 0x83, 0xb2, 0x03, 0x01, 0x01];

pub(super) struct CertificateSpec<'a> {
    pub subject: &'a [u8],
    pub issuer: &'a [u8],
    pub spki: &'a [u8],
    pub own_ski: &'a [u8],
    pub aki: Option<&'a [u8]>,
    pub signer: &'a SigningKey,
    pub serial: u8,
    pub ca: bool,
    pub path: Option<u8>,
    pub dns: &'a [u8],
    pub node: Option<[u8; 32]>,
    pub ca_eku: Option<&'a [u8]>,
}

pub(super) fn key(seed: u8) -> SigningKey {
    SigningKey::from_slice(&[seed; 32]).unwrap()
}

pub(super) fn name(cn: &[u8]) -> Vec<u8> {
    join(
        0x30,
        &[join(
            0x31,
            &[join(0x30, &[oid(&[0x55, 0x04, 0x03]), tlv(0x0c, cn)])],
        )],
    )
}

pub(super) fn spki(key: &SigningKey) -> Vec<u8> {
    let point = key.verifying_key().to_encoded_point(false);
    let mut bits = vec![0];
    bits.extend(point.as_bytes());
    join(
        0x30,
        &[join(0x30, &[oid(EC_OID), oid(CURVE_OID)]), tlv(0x03, &bits)],
    )
}

pub(super) fn ski(spki: &[u8]) -> [u8; 20] {
    let digest = Sha256::digest(spki);
    let mut out = [0; 20];
    out.copy_from_slice(&digest[..20]);
    out
}

pub(super) fn certificate(spec: CertificateSpec<'_>) -> Vec<u8> {
    let validity = join(
        0x30,
        &[tlv(0x17, b"200101000000Z"), tlv(0x17, b"350101000000Z")],
    );
    let tbs = join(
        0x30,
        &[
            tlv(0xa0, &tlv(0x02, &[2])),
            tlv(0x02, &[spec.serial]),
            alg(),
            spec.issuer.to_vec(),
            validity,
            spec.subject.to_vec(),
            spec.spki.to_vec(),
            extensions(&spec),
        ],
    );
    let signature: Signature = spec.signer.sign(&tbs);
    let mut bits = vec![0];
    bits.extend(signature.to_der().as_bytes());
    join(0x30, &[tbs, alg(), tlv(0x03, &bits)])
}

fn extensions(spec: &CertificateSpec<'_>) -> Vec<u8> {
    let basic = if spec.ca {
        let mut parts = vec![tlv(0x01, &[0xff])];
        if let Some(path) = spec.path {
            parts.push(tlv(0x02, &[path]));
        }
        join(0x30, &parts)
    } else {
        join(0x30, &[])
    };
    let mut all = vec![
        extension(&[0x55, 0x1d, 0x13], true, basic),
        extension(
            &[0x55, 0x1d, 0x0f],
            true,
            tlv(0x03, if spec.ca { &[2, 4] } else { &[7, 128] }),
        ),
        extension(&[0x55, 0x1d, 0x0e], false, tlv(0x04, spec.own_ski)),
    ];
    if let Some(aki) = spec.aki {
        all.push(extension(
            &[0x55, 0x1d, 0x23],
            false,
            join(0x30, &[tlv(0x80, aki)]),
        ));
    }
    if let Some(eku) = spec.ca_eku {
        all.push(extension(
            &[0x55, 0x1d, 0x25],
            false,
            join(0x30, &[oid(eku)]),
        ));
    }
    if spec.ca {
        let subtree = join(0x30, &[tlv(0x82, spec.dns)]);
        let constraints = join(0x30, &[tlv(0xa0, &subtree)]);
        all.push(extension(&[0x55, 0x1d, 0x1e], true, constraints));
    } else {
        all.push(extension(
            &[0x55, 0x1d, 0x25],
            false,
            join(0x30, &[oid(CLIENT_OID)]),
        ));
        all.push(extension(
            &[0x55, 0x1d, 0x11],
            false,
            join(0x30, &[tlv(0x82, spec.dns)]),
        ));
        all.push(extension(
            NODE_OID,
            false,
            spec.node
                .unwrap_or_else(|| Sha256::digest(spec.spki).into())
                .to_vec(),
        ));
    }
    join(0xa3, &[join(0x30, &all)])
}

fn extension(id: &[u8], critical: bool, value: Vec<u8>) -> Vec<u8> {
    let mut fields = vec![oid(id)];
    if critical {
        fields.push(tlv(0x01, &[0xff]));
    }
    fields.push(tlv(0x04, &value));
    join(0x30, &fields)
}

fn alg() -> Vec<u8> {
    join(0x30, &[oid(SIG_OID)])
}

fn oid(value: &[u8]) -> Vec<u8> {
    tlv(0x06, value)
}

fn join(tag: u8, parts: &[Vec<u8>]) -> Vec<u8> {
    let mut value = Vec::new();
    for part in parts {
        value.extend(part);
    }
    tlv(tag, &value)
}

fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if value.len() < 128 {
        out.push(value.len() as u8);
    } else if value.len() <= 255 {
        out.extend([0x81, value.len() as u8]);
    } else {
        out.extend([0x82, (value.len() >> 8) as u8, value.len() as u8]);
    }
    out.extend(value);
    out
}
